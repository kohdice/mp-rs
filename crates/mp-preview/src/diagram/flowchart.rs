//! Flowcharts (`flowchart` / `graph` diagrams): parse, lay out in layers, route the
//! links, then draw.

mod draw;
mod label;
mod layout;
mod outline;
mod parse;
mod route;
mod unit;

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

/// Parses `source` and draws it as [`render_chart`] does, under the frontmatter's title when it gives one; a syntax error comes from
/// parsing.
pub(super) fn render(source: &str, width: Option<usize>) -> Result<Vec<Line>, Failure> {
    let front = parse::front_matter(source)?;
    let chart = parse::parse(front.body, front.first_line)?;
    render_chart(chart, width, front.title.as_deref())
}

/// Lays the chart out with the default spacing and then tighter spacings, stopping at
/// the first drawing at most `width` columns wide and [`MAX_CELLS`] in size;
/// unsupported when none fits. Upstream's SVG (`useMaxWidth`) shrinks to fit a narrower
/// container and never grows past its natural size; a terminal has a fixed number of
/// columns and cells that cannot shrink, so the drawing tightens its gaps and otherwise
/// falls back. A subgraph laid out in a direction of its own is drawn
/// first and placed as one box. A `title` is drawn above the drawing and counts toward
/// both bounds.
fn render_chart(
    chart: parse::Flowchart,
    width: Option<usize>,
    title: Option<&str>,
) -> Result<Vec<Line>, Failure> {
    let title_width = title.map(draw::title_width);
    let mut chart = unit::embed_units(chart)?;
    layout::grow_boxes(&mut chart).ok_or(Failure::Unsupported)?;
    if chart.nodes.is_empty() {
        return Err(Failure::Unsupported);
    }
    let axis = chart.direction.axis();
    // How far apart labelled links must enter a box depends on the order they arrive
    // in, which only a layout tells.
    if chart.edges.iter().any(|edge| edge.label.is_some())
        && let Some(spacing) = axis.spacings().first()
    {
        let layered =
            layout::lay_out(&chart, axis, spacing.sibling_gap).ok_or(Failure::Unsupported)?;
        layout::grow_for_labels(&mut chart, &layered).ok_or(Failure::Unsupported)?;
    }
    let ends_at_frame = |end: parse::End| matches!(end, parse::End::Subgraph(_));
    let frame_ends =
        chart.edges.iter().any(|edge| ends_at_frame(edge.from) || ends_at_frame(edge.to));
    // The layout depends on the sibling gap alone, so spacings that differ only in the
    // layer gap reuse it.
    let mut cached: Option<(usize, layout::Layered)> = None;
    for spacing in axis.spacings() {
        if cached.as_ref().is_none_or(|(gap, _)| *gap != spacing.sibling_gap) {
            // How many cells the links meeting a frame need depends on the cells its
            // members take in the layer they meet it in, which only a layout at this
            // sibling gap tells; a frame grows by no more than this layout needs.
            // A layout made with the frames as they end up grown, when measuring gave one.
            let mut settled = None;
            if frame_ends {
                for subgraph in &mut chart.subgraphs {
                    subgraph.spread = 0;
                }
                // A frame grown around members of an outer one moves those members apart,
                // so the outer frame is measured again until no frame changes. A frame's
                // members are moved only by the frames nested in it, so frames settle
                // innermost first, a level a round, within one round more than there are
                // frames; the bound only guards that, keeping the growth found last.
                for _ in 0..=chart.subgraphs.len() {
                    let layered = layout::lay_out(&chart, axis, spacing.sibling_gap)
                        .ok_or(Failure::Unsupported)?;
                    if !layout::grow_frames(&mut chart, &layered) {
                        settled = Some(layered);
                        break;
                    }
                }
            }
            let layered = match settled {
                Some(layered) => layered,
                None => layout::lay_out(&chart, axis, spacing.sibling_gap)
                    .ok_or(Failure::Unsupported)?,
            };
            cached = Some((spacing.sibling_gap, layered));
        }
        let Some((_, layered)) = &cached else { continue };
        let (mut scene, routed) =
            route::route(&chart, layered, axis, spacing.layer_gap).ok_or(Failure::Unsupported)?;
        // Labels only add cells, so a scene already too large is not worth placing
        // them on.
        if !fits(&scene, axis, width, title_width) {
            continue;
        }
        let (labels, cross_shift) = route::place_labels(&routed);
        route::shift(&mut scene, &mut [], 0, cross_shift).ok_or(Failure::Unsupported)?;
        scene.labels = labels;
        if fits(&scene, axis, width, title_width) {
            let drawing = draw::draw(&scene, chart.direction);
            return Ok(match title {
                Some(title) => draw::titled(drawing, title),
                None => drawing,
            });
        }
    }
    Err(Failure::Unsupported)
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
