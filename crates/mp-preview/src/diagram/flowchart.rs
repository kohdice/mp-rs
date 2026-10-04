//! Flowcharts (`flowchart` / `graph` diagrams): parse, lay out in layers, route the
//! links, then draw.

mod draw;
mod layout;
mod outline;
mod parse;
mod route;

use crate::diagram::Failure;
use crate::style::Line;

use self::layout::Axis;
use self::route::Scene;

/// Cells a drawing may span, width times height. Without a bound, a long label in front
/// of a tall layer stretches every row to the label's width, and drawing, label
/// placement and the canvas all take memory proportional to that area; a million cells
/// is far beyond what a terminal shows while holding the canvas to tens of megabytes.
const MAX_CELLS: usize = 1_000_000;

/// `value` as a cell offset that may be negative; `None` past `isize::MAX`.
fn signed(value: usize) -> Option<isize> {
    isize::try_from(value).ok()
}

/// Lays the chart out with the default spacing and then tighter spacings, stopping at
/// the first drawing at most `width` columns wide and [`MAX_CELLS`] in size;
/// unsupported when none fits.
pub(super) fn render(source: &str, width: Option<usize>) -> Result<Vec<Line>, Failure> {
    let chart = parse::parse(source)?;
    if chart.nodes.is_empty() {
        return Err(Failure::Unsupported);
    }
    let axis = chart.direction.axis();
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
        let mut scene =
            route::route(&chart, layered, axis, spacing.layer_gap).ok_or(Failure::Unsupported)?;
        // Labels only add cells, so a scene already too large is not worth placing
        // them on.
        if !fits(&scene, axis, width) {
            continue;
        }
        route::place_labels(&mut scene, axis);
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
