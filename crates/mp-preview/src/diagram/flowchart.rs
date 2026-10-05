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

/// Parses `source` and draws it as [`render_chart`] does; a syntax error comes from
/// parsing.
pub(super) fn render(source: &str, width: Option<usize>) -> Result<Vec<Line>, Failure> {
    render_chart(parse::parse(source)?, width)
}

/// Lays the chart out with the default spacing and then tighter spacings, stopping at
/// the first drawing at most `width` columns wide and [`MAX_CELLS`] in size;
/// unsupported when none fits. Upstream's SVG (`useMaxWidth`) shrinks to fit a narrower
/// container and never grows past its natural size; a terminal has a fixed number of
/// columns and cells that cannot shrink, so the drawing tightens its gaps and otherwise
/// falls back. A subgraph laid out in a direction of its own is drawn
/// first and placed as one box.
fn render_chart(chart: parse::Flowchart, width: Option<usize>) -> Result<Vec<Line>, Failure> {
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
    // The layout depends on the sibling gap alone, so spacings that differ only in the
    // layer gap reuse it.
    let mut cached: Option<(usize, layout::Layered)> = None;
    for spacing in axis.spacings() {
        if cached.as_ref().is_none_or(|(gap, _)| *gap != spacing.sibling_gap) {
            let layered =
                layout::lay_out(&chart, axis, spacing.sibling_gap).ok_or(Failure::Unsupported)?;
            cached = Some((spacing.sibling_gap, layered));
        }
        let Some((_, layered)) = &cached else { continue };
        let (mut scene, routed) =
            route::route(&chart, layered, axis, spacing.layer_gap).ok_or(Failure::Unsupported)?;
        // Labels only add cells, so a scene already too large is not worth placing
        // them on.
        if !fits(&scene, axis, width) {
            continue;
        }
        let (labels, cross_shift) = route::place_labels(&routed);
        route::shift(&mut scene, &mut [], 0, cross_shift).ok_or(Failure::Unsupported)?;
        scene.labels = labels;
        if fits(&scene, axis, width) {
            return Ok(draw::draw(&scene, chart.direction));
        }
    }
    Err(Failure::Unsupported)
}

/// Whether the scene is at most `width` columns wide on screen and spans at most
/// [`MAX_CELLS`].
fn fits(scene: &Scene<'_>, axis: Axis, width: Option<usize>) -> bool {
    let (main, cross) = scene.extent(axis);
    if main.checked_mul(cross).is_none_or(|cells| cells > MAX_CELLS) {
        return false;
    }
    let columns = match axis {
        Axis::Horizontal => main,
        Axis::Vertical => cross,
    };
    width.is_none_or(|width| columns <= width)
}
