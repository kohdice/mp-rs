//! Flowcharts (`flowchart` / `graph` diagrams): parse, collapse the subgraphs marked
//! collapsed, lay out in layers, route the links, then draw.

mod collapse;
mod draw;
mod label;
mod layout;
mod outline;
mod parse;
mod route;
mod styling;
mod unit;

use unicode_width::UnicodeWidthStr;

use crate::diagram::Failure;
use crate::style::Line;

use self::layout::Axis;
use self::route::Scene;

/// Cells a drawing may span, width times height. Without a bound, a long label in front
/// of a tall layer stretches every row to the label's width, and drawing, label
/// placement and the canvas all take memory proportional to that area; a million cells
/// is far beyond what a terminal shows while holding the canvas to tens of megabytes.
/// The bound is this crate's own, not upstream's, which renders any size the browser
/// can hold.
const MAX_CELLS: usize = 1_000_000;

/// `value` as a cell offset that may be negative; `None` past `isize::MAX`.
fn signed(value: usize) -> Option<isize> {
    isize::try_from(value).ok()
}

/// Parses `source`, collapses its collapsed subgraphs, draws each subgraph laid out in a
/// direction of its own as one box (see [`unit::embed_units`]) and the chart around them
/// as [`render_chart`] does, under the frontmatter's title when it gives one, at the
/// default spacing and then tighter ones until the drawing fits; unsupported when none
/// does. A syntax error comes from parsing.
pub(super) fn render(source: &str, width: Option<usize>) -> Result<Vec<Line>, Failure> {
    let front = parse::front_matter(source)?;
    let chart = parse::parse(front.body, front.first_line)?;
    let chart = collapse::collapse(chart).ok_or(Failure::Unsupported)?;
    // A unit's drawing tightens along with the chart around it, so each step draws the
    // units again rather than embedding drawings made at the default spacing.
    for level in 0..layout::LEVELS {
        let embedded = unit::embed_units(chart.clone(), 0, None, level)?;
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
    chart: parse::Flowchart,
    width: Option<usize>,
    title: Option<&str>,
    level: usize,
) -> Result<Option<Vec<Line>>, Failure> {
    let title_width = title.map(UnicodeWidthStr::width);
    let growth = layout::grow_boxes(&chart).ok_or(Failure::Unsupported)?;
    let mut chart = chart.with_growth(growth);
    if chart.nodes.is_empty() {
        return Err(Failure::Unsupported);
    }
    let axis = chart.direction.axis();
    let spacing = axis.spacing(level);
    // How far apart labelled links must enter a box depends on the order they arrive
    // in, which only a layout tells.
    if chart.edges.iter().any(|edge| edge.label.is_some()) {
        let layered =
            layout::lay_out(&chart, axis, spacing.sibling_gap).ok_or(Failure::Unsupported)?;
        let growth = layout::grow_for_labels(&chart, &layered).ok_or(Failure::Unsupported)?;
        chart = chart.with_growth(growth);
    }
    let ends_at_frame = |end: parse::End| matches!(end, parse::End::Subgraph(_));
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
            let layered =
                layout::lay_out(&chart, axis, spacing.sibling_gap).ok_or(Failure::Unsupported)?;
            match layout::grow_frames(&chart, &layered) {
                Some(growth) => chart = chart.with_growth(growth),
                None => {
                    settled = Some(layered);
                    break;
                }
            }
        }
        settled.ok_or(Failure::Unsupported)?
    } else {
        layout::lay_out(&chart, axis, spacing.sibling_gap).ok_or(Failure::Unsupported)?
    };
    let (scene, routed) =
        route::route(&chart, &layered, axis, spacing.layer_gap).ok_or(Failure::Unsupported)?;
    // Labels only add cells, so a scene already too large is not worth placing them on.
    if !fits(&scene, axis, width, title_width) {
        return Ok(None);
    }
    let (labels, cross_shift) = route::place_labels(&routed);
    let scene = route::Scene { labels, ..route::shift_scene(scene, 0, cross_shift) };
    if !fits(&scene, axis, width, title_width) {
        return Ok(None);
    }
    let drawing = draw::draw(&scene, chart.direction);
    Ok(Some(match title {
        Some(title) => draw::titled(drawing, title),
        None => drawing,
    }))
}

/// Whether the scene, under a title row `title_width` columns wide and a blank row when
/// there is a title, is at most `width` columns wide on screen and spans at most
/// [`MAX_CELLS`].
fn fits(scene: &Scene<'_>, axis: Axis, width: Option<usize>, title_width: Option<usize>) -> bool {
    let (main, cross) = scene.extent(axis);
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
