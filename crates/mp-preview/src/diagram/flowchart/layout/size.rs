//! The cells a node's box and a subgraph frame take, and the spacing between layers and
//! between siblings.

use unicode_width::UnicodeWidthStr;

use crate::diagram::flowchart::outline::outline;
use crate::diagram::flowchart::parse::{Body, Node};
use crate::style::Line;

/// Cells from a frame's top-left corner to the blank before its title when nothing is
/// in the way: `┌─`.
pub(in crate::diagram::flowchart) const TITLE_CORNER_OFFSET: usize = 2;

#[derive(Debug)]
pub(in crate::diagram::flowchart) struct Spacing {
    /// Cells between neighbouring layers, unless the tracks (the cells where lines turn
    /// between the layers, see `assign_tracks` in `route`) or labels need more.
    pub layer_gap: usize,
    /// Cells between boxes stacked in the same layer.
    pub sibling_gap: usize,
}

/// The label plus the cells its shape takes around it, or the shape's own width when it
/// hides the label; one cell for a hidden node, and the drawing's widest line for a node
/// holding one.
pub(in crate::diagram::flowchart) fn box_width(node: &Node) -> usize {
    match &node.body {
        Body::Box { label, shape } => outline(*shape).width(label),
        Body::Hidden => 1,
        Body::Drawing { lines, .. } => {
            let line_width = |line: &Line| line.iter().map(|span| span.text.width()).sum::<usize>();
            lines.iter().map(line_width).max().unwrap_or(0)
        }
    }
}

/// The rows of a node's box; one for a hidden node, and the drawing's lines for a node
/// holding one.
pub(super) fn box_height(node: &Node) -> usize {
    match &node.body {
        Body::Box { label, shape } => outline(*shape).height(label),
        Body::Hidden => 1,
        Body::Drawing { lines, .. } => lines.len(),
    }
}

/// The rows from a node's top row to its middle label row (see
/// [`Label::middle_row`](crate::diagram::flowchart::label::Label::middle_row)), or to the
/// middle row of a drawing.
pub(super) fn label_row(node: &Node) -> usize {
    match &node.body {
        Body::Box { label, shape } => outline(*shape).middle_label_row(label),
        Body::Hidden => 0,
        Body::Drawing { lines, .. } => lines.len() / 2,
    }
}

/// The fewest screen rows from a subgraph frame's top or bottom border to its boxes,
/// counting the border.
pub(super) const FRAME_ROWS: usize = 1;
/// The fewest screen columns from a subgraph frame's left or right border to its boxes,
/// counting the border and one blank column.
pub(super) const FRAME_COLS: usize = 2;

/// The narrowest frame, counting both borders, that fits a title `title_width` cells
/// wide whose leading blank is `title_offset` cells right of the top-left corner,
/// followed by a blank, `─` and `┐`.
pub(in crate::diagram::flowchart) fn frame_width(title_width: usize, title_offset: usize) -> usize {
    title_offset + title_width + 4
}

/// The cells from a frame's top-left corner to the blank before a title `title_width`
/// cells wide: [`TITLE_CORNER_OFFSET`], for `┌─` before it, or further right until a
/// `─` separates the blanks around the title from every link crossing the top border,
/// given the cells from the corner where links cross it. A crossing just before or
/// after the blanks would read as `┼ title` or `title ┼`.
pub(in crate::diagram::flowchart) fn title_offset(
    mut crossings: Vec<usize>,
    title_width: usize,
) -> usize {
    crossings.sort_unstable();
    crossings.dedup();
    // Each move puts the `─` before the title right after a crossing, so the crossings
    // the title still meets all lie further right, in the order already sorted.
    let mut offset = TITLE_CORNER_OFFSET;
    for crossing in crossings {
        if crossing > offset + title_width + 2 {
            break;
        }
        if crossing + 1 >= offset {
            offset = crossing + 2;
        }
    }
    offset
}
