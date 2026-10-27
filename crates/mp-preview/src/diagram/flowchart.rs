//! Flowcharts (`flowchart` / `graph` diagrams): parse, collapse the subgraphs marked
//! collapsed, drop the links between a subgraph and what lies inside it, lay out in
//! layers, route the links, then draw.

mod chart;
mod collapse;
mod draw;
mod label;
mod layout;
mod outline;
mod own_frame;
mod parse;
mod route;
mod styling;
mod unit;

use unicode_width::UnicodeWidthStr;

use crate::diagram::Failure;
use crate::style::Line;

use self::layout::Axis;

/// Cells a drawing may span, width times height. Without a bound, a long label in front
/// of a tall layer stretches every row to the label's width, and drawing, label
/// placement and the canvas all take memory proportional to that area; a million cells
/// is far beyond what a terminal shows while holding the canvas to tens of megabytes.
/// The bound is this crate's own, not upstream's, which renders any size the browser
/// can hold.
const MAX_CELLS: usize = 1_000_000;

/// Mermaid's default `maxTextSize`
/// (<https://mermaid.js.org/config/schema-docs/config.html>), in UTF-16 units of the
/// source without its frontmatter, directives and comment lines (see
/// [`parse::text_size`]), as upstream counts it.
const MAX_TEXT_SIZE: usize = 50_000;

/// The source drawn instead of one longer than [`MAX_TEXT_SIZE`]: upstream's
/// `MAX_TEXTLENGTH_EXCEEDED_MSG` in `packages/mermaid/src/mermaidAPI.ts`, which
/// `renderDiagram` draws in its place, under the frontmatter's title.
const TEXT_SIZE_EXCEEDED: &str =
    "graph TB;a[Maximum text size in diagram exceeded];style a fill:#faa";

/// `value` as a cell offset that may be negative; `None` past `isize::MAX`.
fn signed(value: usize) -> Option<isize> {
    isize::try_from(value).ok()
}

/// Draws `source` under the frontmatter's title when it gives one, at the default
/// spacing and then tighter ones until the drawing fits in `width`; unsupported when none
/// does. A source longer than [`MAX_TEXT_SIZE`] is drawn as [`TEXT_SIZE_EXCEEDED`]. A
/// syntax error comes from parsing.
pub(super) fn render(source: &str, width: Option<usize>) -> Result<Vec<Line>, Failure> {
    // Upstream's `cleanupText` rewrites the whole text before the frontmatter is read.
    let source = parse::with_single_quoted_attributes(source);
    let front = parse::front_matter(&source)?;
    // A string has at least as many bytes as UTF-16 units, and `text_size` only removes
    // text, so a body no longer in bytes than the limit is not measured.
    let too_long = front.body.len() > MAX_TEXT_SIZE && parse::text_size(front.body) > MAX_TEXT_SIZE;
    let (body, first_line) =
        if too_long { (TEXT_SIZE_EXCEEDED, 1) } else { (front.body, front.first_line) };
    let chart = parse::parse(body, first_line)?;
    let chart = collapse::collapse(chart).ok_or(Failure::Unsupported)?;
    let chart = own_frame::drop_own_frame_links(chart)?;
    // A unit's drawing tightens along with the chart around it, so each step draws the
    // units again rather than embedding drawings made at the default spacing.
    let draw_unit = |chart, level| render_chart(chart, None, None, level);
    for level in 0..layout::LEVELS {
        let Some(embedded) = unit::embed_units(chart.clone(), 0, None, level, &draw_unit)? else {
            continue;
        };
        if let Some(drawing) = render_chart(embedded, width, front.title.as_deref(), level)? {
            return Ok(drawing);
        }
    }
    Err(Failure::Unsupported)
}

/// Draws the chart at the spacing of tightening step `level` (see [`Axis::spacing`]);
/// `Ok(None)` when the drawing is wider than `width` columns or spans more than
/// [`MAX_CELLS`]. Upstream's SVG (`useMaxWidth`) shrinks to fit a narrower container
/// and never grows past its natural size; a terminal has a fixed number of columns and
/// cells that cannot shrink, so [`render`] tries the next step instead and otherwise
/// falls back. A `title` is drawn above the drawing and counts toward both bounds.
fn render_chart(
    chart: chart::Flowchart,
    width: Option<usize>,
    title: Option<&str>,
    level: usize,
) -> Result<Option<Vec<Line>>, Failure> {
    let title_width = title.map(UnicodeWidthStr::width);
    if chart.nodes.is_empty() {
        return Err(Failure::Unsupported);
    }
    let direction = chart.direction;
    let axis = direction.axis();
    let spacing = axis.spacing(level);
    let ranked = layout::Ranked::new(chart).ok_or(Failure::Unsupported)?;
    let chart = ranked.chart();
    let mut growth = layout::grow_boxes(&ranked).ok_or(Failure::Unsupported)?;
    let lay_out = |growth: &layout::Growth| {
        layout::lay_out(&ranked, growth, spacing.sibling_gap).ok_or(Failure::Unsupported)
    };
    // A layout of the chart as it now grows, kept for the next step to start from.
    let mut current = None;
    // How far apart labelled links must enter a box depends on the order they arrive
    // in, which only a layout tells, and grown boxes keep that order unless a layer is
    // reordered where a stray moves out of a frame (see `grow_for_labels`). The rounds
    // allow the first growth, one more for each frame and a layout confirming the last;
    // as for the frames below, the bound is a safety valve, and boxes still growing at
    // it fall back.
    if chart.edges.iter().any(|edge| edge.label.is_some()) {
        let mut settled = None;
        for _ in 0..chart.subgraphs.len() + 2 {
            let layered = lay_out(&growth)?;
            let grown =
                layout::grow_for_labels(chart, &growth, &layered).ok_or(Failure::Unsupported)?;
            if grown == growth {
                settled = Some(layered);
                break;
            }
            growth = grown;
        }
        current = Some(settled.ok_or(Failure::Unsupported)?);
    }
    let ends_at_frame = |end: chart::End| matches!(end, chart::End::Subgraph(_));
    let frame_ends =
        chart.edges.iter().any(|edge| ends_at_frame(edge.from) || ends_at_frame(edge.to));
    // How many cells the links meeting a frame need depends on the cells its members
    // take in the layer they meet it in, which only a layout at this sibling gap tells;
    // a frame grows by no more than this layout needs.
    let layered = if frame_ends {
        // A frame grown around members of an outer one moves those members apart, so
        // the outer frame is measured again until no frame changes. Frames usually
        // settle innermost first, a level a round, but an outer frame's growth also
        // moves the links crossing the frames nested in it, so nothing guarantees they
        // settle at all: the bound is a safety valve, and frames still changing at it
        // fall back rather than share border cells.
        let mut settled = None;
        for _ in 0..=chart.subgraphs.len() {
            let layered = match current.take() {
                Some(layered) => layered,
                None => lay_out(&growth)?,
            };
            match layout::grow_frames(&growth, &layered) {
                Some(grown) => growth = grown,
                None => {
                    settled = Some(layered);
                    break;
                }
            }
        }
        settled.ok_or(Failure::Unsupported)?
    } else {
        match current {
            Some(layered) => layered,
            None => lay_out(&growth)?,
        }
    };
    let routed =
        route::route(chart, &growth, &layered, ranked.memberships(), axis, spacing.layer_gap)
            .ok_or(Failure::Unsupported)?;
    // Labels only add cells, so a scene already too large is not worth placing them on.
    if !fits(routed.extent_without_labels(axis), axis, width, title_width) {
        return Ok(None);
    }
    let scene = route::place_labels(routed).ok_or(Failure::Unsupported)?;
    if !fits(scene.extent(axis), axis, width, title_width) {
        return Ok(None);
    }
    let drawing = draw::draw(&scene, direction);
    Ok(Some(match title {
        Some(title) => draw::titled(drawing, title),
        None => drawing,
    }))
}

/// Whether a scene spanning `(main, cross)` cells along and across the flow, under a
/// title row `title_width` columns wide and a blank row when there is a title, is at most
/// `width` columns wide on screen and spans at most [`MAX_CELLS`].
fn fits(
    (main, cross): (usize, usize),
    axis: Axis,
    width: Option<usize>,
    title_width: Option<usize>,
) -> bool {
    let (rows, columns) = match axis {
        Axis::Horizontal => (cross, main),
        Axis::Vertical => (main, cross),
    };
    let (title_rows, columns) = title_width.map_or((0, columns), |width| (2, columns.max(width)));
    let rows = rows.checked_add(title_rows);
    if rows.and_then(|rows| rows.checked_mul(columns)).is_none_or(|cells| cells > MAX_CELLS) {
        return false;
    }
    width.is_none_or(|width| columns <= width)
}
