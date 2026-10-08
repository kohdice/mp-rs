//! How far a link's label reaches across the flow from its line, and where on its laid-out
//! path the label goes.

use super::Axis;
use crate::diagram::flowchart::label::Label;
use crate::diagram::flowchart::parse::Edge;

/// A cell where a drawn link between something outside a frame and one of its members, or
/// a frame nested in it, crosses one of the frame's borders across the flow: where the
/// link enters the member or nested frame, with the reach of its label there, or where it
/// leaves the member, with no reach recorded. The cell is fixed by the link's other end,
/// so the frame's own link ends keep clear of it.
pub(super) type Crossing = (isize, Reach);

/// The cells a link's label takes before and after the cell where the link meets a
/// border, across the flow; `None` for a link without a label there.
pub(in crate::diagram::flowchart) type Reach = Option<(usize, usize)>;

/// The cells a label `width` cells wide takes on either side of the cell it is centred
/// on: an odd cell left over goes before it, as the label leans left.
pub(in crate::diagram::flowchart) fn label_reach(width: usize) -> (usize, usize) {
    (width / 2, width.saturating_sub(1) - width / 2)
}

/// The cells `label` takes across the flow before and after the cell of the line it lies
/// on: in a vertical layout its rows are centred on the line's column (see
/// [`label_reach`]); in a horizontal one the line runs through its middle row (see
/// [`Label::middle_row`]) and the other rows lie above and below it.
pub(in crate::diagram::flowchart) fn label_cross_reach(
    axis: Axis,
    label: &Label,
) -> (usize, usize) {
    match axis {
        Axis::Vertical => label_reach(label.width()),
        Axis::Horizontal => {
            let middle = label.middle_row();
            (middle, label.height() - 1 - middle)
        }
    }
}

/// Where the routing places a link's label on its line: at Mermaid's label rank, half way
/// between the link's laid-out ends.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(in crate::diagram::flowchart) enum LabelSpot {
    /// In the gap that this segment of the laid-out path crosses: the middle segment of a
    /// path with an odd number of segments (a link between neighbouring layers has one).
    Gap(usize),
    /// On the passing slot at this index of the laid-out path: the middle slot of a path
    /// with an even number of segments.
    Slot(usize),
}

/// Where the label of a link laid out along `path` goes (see [`LabelSpot`]). `None` for a
/// self loop, whose label goes outside the loop (see
/// [`self_loop_label_at`](super::self_loop_label_at)).
pub(in crate::diagram::flowchart) fn label_spot(path: &[usize]) -> Option<LabelSpot> {
    let segments = path.len().checked_sub(1).filter(|&segments| segments > 0)?;
    let middle = segments / 2;
    Some(if segments % 2 == 1 { LabelSpot::Gap(middle) } else { LabelSpot::Slot(middle) })
}

/// The cells a passing slot carrying a label takes, and where its port lies.
#[derive(Clone, Copy, Debug)]
pub(super) struct Footprint {
    /// The cells the slot takes along the flow.
    pub main: usize,
    /// The cells the slot takes across the flow.
    pub cross: usize,
    /// The offset of the slot's port, where the line runs, from its first cell across the
    /// flow.
    pub port_offset: usize,
}

/// The footprint of a passing slot carrying `label`. Mermaid makes the label's dummy node
/// a box the size of the label. In a horizontal layout the label runs
/// along the line with a line cell on either side, so it takes its widest row plus 2
/// cells along the flow and its rows across; in a vertical one it takes its rows along
/// the flow and its widest row across. Across the flow the label lies on the port as
/// [`label_cross_reach`] puts it, with a blank cell on either side.
pub(super) fn labelled_slot_footprint(axis: Axis, label: &Label) -> Footprint {
    let (main, across) = match axis {
        Axis::Horizontal => (label.width() + 2, label.height()),
        Axis::Vertical => (label.height(), label.width()),
    };
    let (before, _) = label_cross_reach(axis, label);
    Footprint { main, cross: across + 2, port_offset: before + 1 }
}

/// The reach of the label `edge` carries where `segment` of its laid-out `path` enters
/// the next slot: only a label in the gap that segment crosses ([`LabelSpot::Gap`]) has
/// a reach there, as [`label_cross_reach`] gives it. A one-row label in a horizontal
/// layout runs along the line on that cell's row, so it reaches no further across the
/// flow, but still counts as a label: [`end_gaps`](super::ports::end_gaps) keeps a blank
/// row between a labelled line and the end next to it, so that two labels never read as
/// one block of text.
pub(in crate::diagram::flowchart) fn entry_reach(
    axis: Axis,
    edge: &Edge,
    path: &[usize],
    segment: usize,
) -> Reach {
    if label_spot(path) != Some(LabelSpot::Gap(segment)) {
        return None;
    }
    edge.label.as_ref().map(|label| label_cross_reach(axis, label))
}
