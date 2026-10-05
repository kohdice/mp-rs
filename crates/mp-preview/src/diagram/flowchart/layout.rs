//! Assigning flowchart nodes to layers along the flow and to positions across it.

use std::collections::HashMap;
use std::ops::RangeInclusive;

use unicode_width::UnicodeWidthStr;

use crate::style::Line;

use super::label::Label;
use super::outline::outline;
use super::parse::{Body, Direction, Edge, End, Flowchart, Node};
use super::signed;

/// Cells a self loop runs through beyond its box along the flow: its legs leave and
/// re-enter the box's far border on the first, and its run joins them on the second.
pub(super) const SELF_LOOP_CELLS: usize = 2;

/// Cells across the flow from a self loop's first leg to its second, which leaves the
/// cell between them free, as Mermaid centres a loop's span on its node.
pub(super) const SELF_LOOP_SPAN: usize = 2;

/// The cells along the flow a self loop carrying `label` takes after its box, before
/// the tracks there: its legs and its run, then, outside the loop beyond the run, the
/// label's rows in a vertical layout, or in a horizontal one a blank cell and the label
/// running along the flow.
pub(super) fn self_loop_cells(axis: Axis, label: Option<&Label>) -> usize {
    match (axis, label) {
        (_, None) => SELF_LOOP_CELLS,
        (Axis::Vertical, Some(label)) => SELF_LOOP_CELLS + label.height(),
        (Axis::Horizontal, Some(label)) => SELF_LOOP_CELLS + 1 + label.width(),
    }
}

/// Where `label`, on a self loop whose first leg is at `leg` across the flow, starts: the
/// cells along the flow after the box's far border, and the cell across the flow, which
/// may lie before cell 0. Mermaid puts the label outside the loop beyond its run, centred
/// on the loop: here directly beyond the run in a vertical layout, with the usual lean
/// left (see [`label_reach`]), and one blank cell beyond it with its middle row on the
/// row between the legs in a horizontal one (see [`label_cross_reach`]).
pub(super) fn self_loop_label_at(axis: Axis, leg: usize, label: &Label) -> Option<(usize, isize)> {
    let centre = signed(leg + SELF_LOOP_SPAN / 2)?;
    let (before, _) = label_cross_reach(axis, label);
    let along = match axis {
        Axis::Vertical => SELF_LOOP_CELLS,
        Axis::Horizontal => SELF_LOOP_CELLS + 1,
    };
    Some((along, centre - signed(before)?))
}

/// Cells from a frame's top-left corner to the blank before its title when nothing is
/// in the way: `┌─`.
pub(super) const TITLE_CORNER_OFFSET: usize = 2;

/// Whether the flow runs along the columns (`LR`, `RL`) or the rows (`TD`, `BT`).
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(super) enum Axis {
    Horizontal,
    Vertical,
}

impl Direction {
    pub(super) fn axis(self) -> Axis {
        match self {
            Self::LeftToRight | Self::RightToLeft => Axis::Horizontal,
            Self::TopDown | Self::BottomUp => Axis::Vertical,
        }
    }

    /// Whether links point towards the top-left, against the order rows and columns
    /// are drawn in.
    pub(super) fn points_backward(self) -> bool {
        matches!(self, Self::RightToLeft | Self::BottomUp)
    }
}

impl Axis {
    /// The cells a box spans along the flow.
    pub(super) fn box_main_size(self, node: &Node) -> usize {
        match self {
            Self::Horizontal => box_width(node),
            Self::Vertical => box_height(node),
        }
    }

    /// The cells a box spans across the flow, grown by its spread on either side.
    pub(super) fn box_cross_size(self, node: &Node) -> usize {
        let natural = match self {
            Self::Horizontal => box_height(node),
            Self::Vertical => box_width(node),
        };
        natural + 2 * node.spread
    }

    /// The offset across the flow from a box's first cell to the middle of the borders
    /// that links leave and enter: the middle label row (see [`Label::middle_row`]), or
    /// the centre column.
    pub(super) fn port_offset(self, node: &Node) -> usize {
        match self {
            Self::Horizontal => label_row(node) + node.spread,
            Self::Vertical => self.box_cross_size(node) / 2,
        }
    }

    /// The cells across the flow, counted from a box's first cell, where links may meet
    /// its borders along the flow: in a horizontal layout the middle label row and the
    /// rows the box grew by around it, as many on either side as its spread, which leaves
    /// out a cylinder's arc row; in a vertical one every column between the corners.
    pub(super) fn port_range(self, node: &Node) -> RangeInclusive<usize> {
        match (self, &node.body) {
            // A drawing's frame is never grown: its links meet any row between the corners.
            (Self::Horizontal, Body::Drawing(_)) => 1..=box_height(node).saturating_sub(2),
            (Self::Horizontal, Body::Box { .. } | Body::Hidden) => {
                let first = label_row(node);
                first..=first + 2 * node.spread
            }
            (Self::Vertical, Body::Hidden) => 0..=2 * node.spread,
            (Self::Vertical, Body::Box { .. } | Body::Drawing(_)) => {
                1..=self.box_cross_size(node).saturating_sub(2)
            }
        }
    }

    /// The stride between the entry cells of links arriving at one box: every row in a
    /// horizontal layout, every other column in a vertical one so that two arrowheads
    /// never sit in neighbouring columns.
    fn entry_spacing(self) -> usize {
        match self {
            Self::Horizontal => 1,
            Self::Vertical => 2,
        }
    }

    /// The fewest cells along and across the flow from a subgraph frame's borders to its
    /// boxes, counting the border.
    pub(super) fn frame_margins(self) -> (usize, usize) {
        match self {
            Self::Horizontal => (FRAME_COLS, FRAME_ROWS),
            Self::Vertical => (FRAME_ROWS, FRAME_COLS),
        }
    }

    /// The default spacing first, then tighter ones to try in turn when the drawing is
    /// too wide or spans more than [`MAX_CELLS`](super::MAX_CELLS).
    pub(super) fn spacings(self) -> &'static [Spacing] {
        // A sibling gap of one is the least that keeps neighbouring boxes from touching.
        match self {
            Self::Horizontal => &[
                // `────►`, with room on the line for a two-cell label such as `no`
                // between a line cell on either side and the arrowhead before the gap
                // has to widen.
                Spacing { layer_gap: 5, sibling_gap: 1 },
                Spacing { layer_gap: 3, sibling_gap: 1 },
                // `─►`, the narrowest gap that still shows a line before the arrowhead.
                Spacing { layer_gap: 2, sibling_gap: 1 },
            ],
            Self::Vertical => &[
                Spacing { layer_gap: 3, sibling_gap: 2 },
                Spacing { layer_gap: 3, sibling_gap: 1 },
            ],
        }
    }
}

#[derive(Debug)]
pub(super) struct Spacing {
    /// Cells between neighbouring layers, unless tracks or labels need more.
    pub layer_gap: usize,
    /// Cells between boxes stacked in the same layer.
    pub sibling_gap: usize,
}

/// The label plus the cells its shape takes around it; one cell for a hidden node, and
/// the drawing's widest line for a node holding one.
pub(super) fn box_width(node: &Node) -> usize {
    match &node.body {
        Body::Box { label, shape } => label.width() + outline(*shape).padding(),
        Body::Hidden => 1,
        Body::Drawing(drawing) => {
            let line_width = |line: &Line| line.iter().map(|span| span.text.width()).sum::<usize>();
            drawing.iter().map(line_width).max().unwrap_or(0)
        }
    }
}

/// The rows of a node's box; one for a hidden node, and the drawing's lines for a node
/// holding one.
fn box_height(node: &Node) -> usize {
    match &node.body {
        Body::Box { label, shape } => outline(*shape).height(label.height()),
        Body::Hidden => 1,
        Body::Drawing(drawing) => drawing.len(),
    }
}

/// The rows from a node's top row to its middle label row (see [`Label::middle_row`]),
/// or to the middle row of a drawing.
fn label_row(node: &Node) -> usize {
    match &node.body {
        Body::Box { label, shape } => outline(*shape).label_row() + label.middle_row(),
        Body::Hidden => 0,
        Body::Drawing(drawing) => drawing.len() / 2,
    }
}

/// The fewest screen rows from a subgraph frame's top or bottom border to its boxes,
/// counting the border.
const FRAME_ROWS: usize = 1;
/// The fewest screen columns from a subgraph frame's left or right border to its boxes,
/// counting the border and one blank column.
const FRAME_COLS: usize = 2;

/// The narrowest frame, counting both borders, that fits a title `title_width` cells
/// wide whose leading blank is `title_offset` cells right of the top-left corner,
/// followed by a blank, `─` and `┐`.
pub(super) fn frame_width(title_width: usize, title_offset: usize) -> usize {
    title_offset + title_width + 4
}

/// The cells from a frame's top-left corner to the blank before a title `title_width`
/// cells wide: [`TITLE_CORNER_OFFSET`], for `┌─` before it, or further right until a
/// `─` separates the blanks around the title from every link crossing the top border,
/// given the cells from the corner where links cross it. A crossing just before or
/// after the blanks would read as `┼ title` or `title ┼`.
pub(super) fn title_offset(mut crossings: Vec<usize>, title_width: usize) -> usize {
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

/// The cell where each link meets one border of a slot, counted from the slot's first
/// cell and listed in the order of `others`, the positions across the flow of the
/// links' other ends: the links take the border's port `cells` spread symmetrically
/// about `port` (see [`symmetric_offsets`]), in the order of their other ends, ties
/// keeping the order of `others`. Neighbouring ends are [`end_gaps`] apart, given the
/// cells each end's label takes on either side of it, `reach`, indexed like `others`
/// (empty when no end has a label). [`grow_boxes`] and [`grow_for_labels`] make every
/// box border wide enough; should the cells still not hold them, the links share them
/// out from `port` outwards, several to a cell.
pub(super) fn port_cells<P: Ord>(
    axis: Axis,
    others: &[P],
    reach: &[Reach],
    cells: &RangeInclusive<usize>,
    port: usize,
) -> Option<Vec<usize>> {
    let mut order: Vec<usize> = (0..others.len()).collect();
    order.sort_by_key(|&end| others.get(end));
    let ordered: Vec<Reach> = order.iter().map(|&end| reach.get(end).copied().flatten()).collect();
    let ranked = ranked_ports(axis, port, cells, &end_gaps(axis, &ordered), others.len())?;
    scatter_by_order(others, &ranked)
}

/// The cells of `ranked`, listed in their order along a border, given back in the order
/// of `keys`: the end at each rank is the one that rank takes when `keys` are sorted, ties
/// keeping the order of `keys`.
pub(super) fn scatter_by_order<P: Ord>(keys: &[P], ranked: &[usize]) -> Option<Vec<usize>> {
    let mut order: Vec<usize> = (0..keys.len()).collect();
    order.sort_by_key(|&end| keys.get(end));
    let mut cells_of = vec![0; keys.len()];
    for (&end, &cell) in order.iter().zip(ranked) {
        *cells_of.get_mut(end)? = cell;
    }
    Some(cells_of)
}

/// The cells where the links leaving a box meet its far border along the flow, counted
/// from the box's first cell, as [`exit_cells`] spreads them.
#[derive(Debug)]
pub(super) struct Exits {
    /// The cells of the links to other slots, in rank order (see [`scatter_by_order`]).
    pub targets: Vec<usize>,
    /// The cell of each self loop's first leg; its second is [`SELF_LOOP_SPAN`] further.
    pub legs: Vec<usize>,
}

/// The cells where the links leaving a box meet its far border along the flow, as
/// [`port_cells`] gives them: first `targets` links to other slots, then a self loop
/// for each of `loop_labels`, the labels of the box's self loops in the order of their
/// edges, each taking two cells [`SELF_LOOP_SPAN`] apart (see [`exit_gaps`]). `None`
/// when a loop's legs do not get two cells of `cells` exactly that far apart, as on a
/// border too short to hold them that cannot grow, such as a drawing's frame.
pub(super) fn exit_cells(
    axis: Axis,
    targets: usize,
    loop_labels: &[Option<&Label>],
    cells: &RangeInclusive<usize>,
    port: usize,
) -> Option<Exits> {
    let gaps = exit_gaps(axis, targets, loop_labels);
    let ranked = ranked_ports(axis, port, cells, &gaps, targets + 2 * loop_labels.len())?;
    let legs = ranked
        .get(targets..)?
        .chunks(2)
        .map(|pair| match *pair {
            [first, second] if second == first + SELF_LOOP_SPAN && cells.contains(&second) => {
                Some(first)
            }
            _ => None,
        })
        .collect::<Option<Vec<_>>>()?;
    let targets = ranked.get(..targets)?.to_vec();
    Some(Exits { targets, legs })
}

/// The distances between neighbouring ends on a box's far border along the flow, in the
/// order [`exit_cells`] lists them: `targets` links to other slots, then a self loop of
/// two legs for each of `loop_labels`. The legs of one loop are [`SELF_LOOP_SPAN`]
/// apart; any other neighbours are [`end_gaps`] apart, as the ends of labelled links
/// entering a box are. In a vertical layout a loop's label lies across the flow, centred
/// on the cell between the legs, so each leg carries the label's reach measured from it;
/// that keeps one blank cell between the label and the line or label next to it. In a
/// horizontal one the label's middle row is the row between the legs, and a leg whose
/// row the label reaches carries the rows reaching on past it, so that the end next to
/// the leg keeps a blank row from the label too.
fn exit_gaps(axis: Axis, targets: usize, loop_labels: &[Option<&Label>]) -> Vec<usize> {
    let mut ordered: Vec<Reach> = vec![None; targets];
    let half = SELF_LOOP_SPAN / 2;
    for label in loop_labels {
        let legs = match (axis, label) {
            (Axis::Vertical, Some(label)) => {
                let (before, after) = label_cross_reach(axis, label);
                [
                    Some((before.saturating_sub(half), after + half)),
                    Some((before + half, after.saturating_sub(half))),
                ]
            }
            (Axis::Horizontal, Some(label)) => {
                let (before, after) = label_cross_reach(axis, label);
                [
                    (before >= half).then(|| (before - half, 0)),
                    (after >= half).then(|| (0, after - half)),
                ]
            }
            (_, None) => [None, None],
        };
        ordered.extend(legs);
    }
    let mut gaps = end_gaps(axis, &ordered);
    for index in 0..loop_labels.len() {
        if let Some(gap) = gaps.get_mut(targets + 2 * index) {
            *gap = SELF_LOOP_SPAN;
        }
    }
    gaps
}

/// The cell of each of `count` link ends in their order along a border: spread
/// symmetrically about `port` with `gaps` between them (see [`symmetric_offsets`]), or,
/// when those cells do not all lie within `cells`, shared out from `port` outwards,
/// [`Axis::entry_spacing`] apart and several to a cell.
fn ranked_ports(
    axis: Axis,
    port: usize,
    cells: &RangeInclusive<usize>,
    gaps: &[usize],
    count: usize,
) -> Option<Vec<usize>> {
    let ports = symmetric_ports(port, cells, count, gaps)
        .unwrap_or_else(|| spread_ports(port, cells, axis.entry_spacing(), count));
    (0..count).map(|rank| ports.get(rank * ports.len() / count).copied()).collect()
}

/// The cells a link's label takes before and after the cell where the link meets a
/// border, across the flow; `None` for a link without a label there.
pub(super) type Reach = Option<(usize, usize)>;

/// The cells a label `width` cells wide takes on either side of the cell it is centred
/// on: an odd cell left over goes before it, as the label leans left.
pub(super) fn label_reach(width: usize) -> (usize, usize) {
    (width / 2, width.saturating_sub(1) - width / 2)
}

/// The cells `label` takes across the flow before and after the cell of the line it lies
/// on: in a vertical layout its rows are centred on the line's column (see
/// [`label_reach`]); in a horizontal one the line runs through its middle row (see
/// [`Label::middle_row`]) and the other rows lie above and below it.
pub(super) fn label_cross_reach(axis: Axis, label: &Label) -> (usize, usize) {
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
pub(super) enum LabelSpot {
    /// In the gap that this segment of the laid-out path crosses: the middle segment of a
    /// path with an odd number of segments (a link between neighbouring layers has one).
    Gap(usize),
    /// On the passing slot at this index of the laid-out path: the middle slot of a path
    /// with an even number of segments.
    Slot(usize),
}

/// Where the label of a link laid out along `path` goes (see [`LabelSpot`]). `None` for a
/// self loop, whose label goes outside the loop (see [`self_loop_label_at`]).
pub(super) fn label_spot(path: &[usize]) -> Option<LabelSpot> {
    let segments = path.len().checked_sub(1).filter(|&segments| segments > 0)?;
    let middle = segments / 2;
    Some(if segments % 2 == 1 { LabelSpot::Gap(middle) } else { LabelSpot::Slot(middle) })
}

/// The footprint of a passing slot carrying `label`, as `(main, cross, port_offset)`: the
/// cells it takes along the flow, the cells it takes across the flow, and the offset of
/// its port, where the line runs, from its first cell across the flow. Mermaid makes the
/// label's dummy node a box the size of the label. In a horizontal layout the label runs
/// along the line with a line cell on either side, so it takes its widest row plus 2
/// cells along the flow and its rows across; in a vertical one it takes its rows along
/// the flow and its widest row across. Across the flow the label lies on the port as
/// [`label_cross_reach`] puts it, with a blank cell on either side.
fn labelled_slot_footprint(axis: Axis, label: &Label) -> (usize, usize, usize) {
    let (main, across) = match axis {
        Axis::Horizontal => (label.width() + 2, label.height()),
        Axis::Vertical => (label.height(), label.width()),
    };
    let (before, _) = label_cross_reach(axis, label);
    (main, across + 2, before + 1)
}

/// The reach of the label `edge` carries where `segment` of its laid-out `path` enters
/// the next slot: only a label in the gap that segment crosses ([`LabelSpot::Gap`]) has
/// a reach there, as [`label_cross_reach`] gives it. A one-row label in a horizontal
/// layout runs along the line on that cell's row, so it reaches no further across the
/// flow, but still counts as a label: [`end_gaps`] keeps a blank row between a labelled
/// line and the end next to it, so that two labels never read as one block of text.
pub(super) fn entry_reach(axis: Axis, edge: &Edge, path: &[usize], segment: usize) -> Reach {
    if label_spot(path) != Some(LabelSpot::Gap(segment)) {
        return None;
    }
    edge.label.as_ref().map(|label| label_cross_reach(axis, label))
}

/// The distance between each pair of neighbouring ends, listed in order along the
/// border with the reach of each end's label: [`Axis::entry_spacing`], or, where either
/// end has a label, enough for one blank cell between a label and the label or line
/// next to it.
fn end_gaps(axis: Axis, ordered: &[Reach]) -> Vec<usize> {
    let spacing = axis.entry_spacing();
    ordered
        .windows(2)
        .map(|pair| match pair {
            [None, None] => spacing,
            [before, after] => {
                let after_first = before.map_or(0, |(_, after)| after);
                let before_second = after.map_or(0, |(before, _)| before);
                spacing.max(after_first + before_second + 2)
            }
            _ => spacing,
        })
        .collect()
}

/// The offsets from a border's centre of `count` link ends, in ascending order, where
/// `gaps[i]` is the distance between the ends `i` and `i + 1`: for an odd count the
/// middle end at the centre; for an even count the centre skipped, the inner pair their
/// gap apart, the right one half of it out rounded up and the left one the rest, at
/// least one cell. The other ends follow outwards, each its gap from its neighbour
/// nearer the centre. `None` when `gaps` has fewer than `count - 1` entries.
fn symmetric_offsets(count: usize, gaps: &[usize]) -> Option<Vec<isize>> {
    let gap = |index: usize| gaps.get(index).and_then(|&gap| isize::try_from(gap).ok());
    let mut offsets = vec![0; count];
    let middle = count / 2;
    // The ends right of the centre start at `middle`, those left of it end before
    // `first_left + 1`.
    let first_left = if count % 2 == 1 {
        middle
    } else if let Some(left) = middle.checked_sub(1) {
        let inner_gap = gap(left)?;
        let right = (inner_gap + 1) / 2;
        *offsets.get_mut(left)? = -(inner_gap - right).max(1);
        *offsets.get_mut(middle)? = right;
        left
    } else {
        return Some(offsets);
    };
    for index in middle + 1..count {
        *offsets.get_mut(index)? = offsets.get(index - 1)? + gap(index - 1)?;
    }
    for index in (0..first_left).rev() {
        *offsets.get_mut(index)? = offsets.get(index + 1)? - gap(index)?;
    }
    Some(offsets)
}

/// The cells [`symmetric_offsets`] gives `count` link ends with `gaps` between them
/// about `port`, or `None` when one of them lies outside `cells`.
fn symmetric_ports(
    port: usize,
    cells: &RangeInclusive<usize>,
    count: usize,
    gaps: &[usize],
) -> Option<Vec<usize>> {
    symmetric_offsets(count, gaps)?
        .into_iter()
        .map(|offset| port.checked_add_signed(offset).filter(|cell| cells.contains(cell)))
        .collect()
}

/// Grows each box across the flow by whole cells on both sides of its label until the
/// link ends on each of its two borders along the flow fit its port cells at
/// [`Axis::entry_spacing`], spread as [`symmetric_offsets`] spreads them. A link closing
/// a cycle is counted on the borders it is laid out between, and a self loop takes two
/// cells [`SELF_LOOP_SPAN`] apart on the border its box's other links leave by. `None`
/// when the chart cannot be searched for cycles.
pub(super) fn grow_boxes(chart: &mut Flowchart) -> Option<()> {
    let axis = chart.direction.axis();
    let reversed = cycle_closing_edges(chart)?;
    let mut entering = vec![0; chart.nodes.len()];
    let mut leaving = vec![0; chart.nodes.len()];
    let mut loops: Vec<Vec<Option<&Label>>> = vec![Vec::new(); chart.nodes.len()];
    for (edge, &reversed) in chart.edges.iter().zip(&reversed) {
        // An invisible link meets no border.
        if !edge.stroke.is_visible() {
            continue;
        }
        let (from, to) = if reversed { (edge.to, edge.from) } else { (edge.from, edge.to) };
        let (from, to) = (laid_out_end(chart, from), laid_out_end(chart, to));
        // The end at a frame meets no box.
        match (from, to) {
            (End::Node(from), End::Node(to)) if from == to => {
                loops.get_mut(from)?.push(edge.label.as_ref());
            }
            _ => {
                if let End::Node(from) = from {
                    *leaving.get_mut(from)? += 1;
                }
                if let End::Node(to) = to {
                    *entering.get_mut(to)? += 1;
                }
            }
        }
    }
    let reach = |offsets: Vec<isize>| offsets.iter().map(|offset| offset.unsigned_abs()).max();
    let spreads = entering
        .into_iter()
        .zip(leaving)
        .zip(&loops)
        .map(|((entering, leaving), labels)| {
            let entries = symmetric_offsets(
                entering,
                &vec![axis.entry_spacing(); entering.saturating_sub(1)],
            )?;
            let exits =
                symmetric_offsets(leaving + 2 * labels.len(), &exit_gaps(axis, leaving, labels))?;
            Some(reach(entries).max(reach(exits)).unwrap_or(0))
        })
        .collect::<Option<Vec<_>>>()?;
    for (node, reach) in chart.nodes.iter_mut().zip(spreads) {
        node.spread = spread_for(node, axis, reach);
    }
    Some(())
}

/// The cells `node` grows by on both sides of its label across the flow so that its
/// port cells reach `reach` cells either side of its centre.
fn spread_for(node: &Node, axis: Axis, reach: usize) -> usize {
    match (&node.body, axis) {
        // A drawing keeps the size of its content; its links share cells instead.
        (Body::Drawing(_), _) => 0,
        // The middle label row is a port row, and every row the box grew by around it
        // is one more on either side; a hidden node's every cell is a port.
        (Body::Box { .. }, Axis::Horizontal) | (Body::Hidden, _) => reach,
        // The port cells run from one column inside the left corner to one inside
        // the right corner, around the centre column.
        (Body::Box { .. }, Axis::Vertical) => {
            let width = box_width(node);
            let centre = width / 2;
            (reach + 1).saturating_sub(centre).max((reach + centre + 2).saturating_sub(width))
        }
    }
}

/// Grows each box further where the labels of the links entering it need their entries
/// further apart than [`grow_boxes`] allowed, given the order the links arrive in under
/// `layered`, as [`port_cells`] spreads them. The order of the slots in each layer does
/// not depend on the boxes' sizes, so the chart can be laid out again with the grown
/// boxes. `None` when a link's slot is missing from `layered`.
pub(super) fn grow_for_labels(chart: &mut Flowchart, layered: &Layered) -> Option<()> {
    let axis = chart.direction.axis();
    let slots = &layered.slots;
    // For each box, the links entering it: the port they come from, their edge and the
    // reach of their label.
    let mut arrivals: Vec<Vec<(usize, usize, Reach)>> = vec![Vec::new(); chart.nodes.len()];
    for (index, (edge, path)) in chart.edges.iter().zip(&layered.paths).enumerate() {
        if !edge.stroke.is_visible() {
            continue;
        }
        for (segment, pair) in path.windows(2).enumerate() {
            if let &[from, to] = pair
                && let Some(arriving) = arrivals.get_mut(to)
            {
                let reach = entry_reach(axis, edge, path, segment);
                arriving.push((slots.get(from)?.port, index, reach));
            }
        }
    }
    for (node, mut arriving) in chart.nodes.iter_mut().zip(arrivals) {
        if arriving.iter().all(|&(.., reach)| reach.is_none()) {
            continue;
        }
        arriving.sort_by_key(|&(port, index, _)| (port, index));
        let ordered: Vec<Reach> = arriving.iter().map(|&(.., reach)| reach).collect();
        let offsets = symmetric_offsets(ordered.len(), &end_gaps(axis, &ordered))?;
        let reach = offsets.iter().map(|offset| offset.unsigned_abs()).max().unwrap_or(0);
        node.spread = node.spread.max(spread_for(node, axis, reach));
    }
    Some(())
}

/// Up to `count` cells within `cells`, `spacing` apart, nearest to `port` first, in
/// ascending order.
fn spread_ports(
    port: usize,
    cells: &RangeInclusive<usize>,
    spacing: usize,
    count: usize,
) -> Vec<usize> {
    let mut ports = vec![port];
    let mut distance = spacing;
    while ports.len() < count {
        let before = port.checked_sub(distance).filter(|cell| cells.contains(cell));
        let after = Some(port + distance).filter(|cell| cells.contains(cell));
        if before.is_none() && after.is_none() {
            break;
        }
        ports.extend(before);
        if ports.len() < count {
            ports.extend(after);
        }
        distance += spacing;
    }
    ports.sort_unstable();
    ports
}

/// A place in a layer: a node's box, or the cells a link longer than one layer passes
/// through.
#[derive(Debug)]
pub(super) struct Slot {
    pub layer: usize,
    /// The first cell across the flow.
    pub cross: usize,
    /// The cell across the flow where links leave and enter.
    pub port: usize,
    /// The cells the slot takes along the flow; `route` sizes a layer as the largest `main`
    /// of its slots. A node's box takes its size along the flow. A passing slot carrying a
    /// label takes its widest row plus a line cell on either side in a horizontal layout
    /// (`label.width() + 2`), and its rows (`label.height()`) in a vertical one, so that a
    /// layer with no node (as under `A -----> B`) still has room for the label. Any other
    /// passing slot takes 0.
    pub main: usize,
}

#[derive(Debug)]
pub(super) struct Layered {
    /// The chart's nodes, indexed like [`Flowchart::nodes`], followed by the slots of
    /// links that span several layers.
    pub slots: Vec<Slot>,
    pub layer_count: usize,
    /// The slots each edge passes through, one per layer from the earlier of its ends to
    /// the later, indexed like [`Flowchart::edges`]; a self loop's path is its node
    /// alone.
    pub paths: Vec<Vec<usize>>,
    /// Whether each edge runs against the flow, from its source in a later layer back
    /// to its target, indexed like [`Flowchart::edges`].
    pub reversed: Vec<bool>,
}

/// Bounds the slots added for long links. Their number grows with the product of the
/// number of edges and the number of layers, so a small input could otherwise demand
/// an enormous drawing.
const MAX_PASSING_SLOTS: usize = 10_000;

/// Places every node in the first layer that is, for each parent, at least the link's
/// `length` layers after that parent: the layer after its furthest parent when every
/// link has length one. Edges closing a cycle are laid out reversed. `None` when the
/// links spanning several layers would need more than [`MAX_PASSING_SLOTS`] slots.
pub(super) fn lay_out(chart: &Flowchart, axis: Axis, sibling_gap: usize) -> Option<Layered> {
    let reversed = cycle_closing_edges(chart)?;
    let mut layer_of =
        longest_path_layers(chart.nodes.len(), &layering_constraints(chart, &reversed)?)?;
    let node_count = chart.nodes.len();
    // The first and last layers of each subgraph's members, where links to and from it
    // meet its frame.
    let member_layers = chart
        .subgraphs
        .iter()
        .map(|subgraph| {
            let layers = subgraph.members.iter().map(|&member| layer_of.get(member).copied());
            let layers = layers.collect::<Option<Vec<_>>>()?;
            Some((*layers.iter().min()?, *layers.iter().max()?))
        })
        .collect::<Option<Vec<_>>>()?;
    // For each slot past the nodes that is the end of a link at a frame rather than a
    // passing slot: the subgraph, and whether the link enters it.
    let mut frame_end: HashMap<usize, (usize, bool)> = HashMap::new();
    let mut paths = Vec::with_capacity(chart.edges.len());
    for (edge, &reversed) in chart.edges.iter().zip(&reversed) {
        // The routing ends a link laid out to an empty subgraph's hidden member at the
        // frame's border.
        let mut slot_at = |end: End, entering: bool| -> Option<usize> {
            match laid_out_end(chart, end) {
                End::Node(node) => Some(node),
                End::Subgraph(subgraph) => {
                    let (first, last) = *member_layers.get(subgraph)?;
                    frame_end.insert(layer_of.len(), (subgraph, entering));
                    layer_of.push(if entering { first } else { last });
                    Some(layer_of.len() - 1)
                }
            }
        };
        let (from, to) = if reversed { (edge.to, edge.from) } else { (edge.from, edge.to) };
        let (start, end) = (slot_at(from, false)?, slot_at(to, true)?);
        if start == end {
            paths.push(vec![start]);
            continue;
        }
        let (from, to) = (*layer_of.get(start)?, *layer_of.get(end)?);
        // The layering keeps a frame after the links into it and before the links out of
        // it, so only a link between two nodes can have its ends in one layer: a self
        // loop, handled above.
        if from >= to {
            return None;
        }
        // A link spanning several layers gets a slot in every layer it passes.
        let mut path = vec![start];
        for layer in from + 1..to {
            if layer_of.len() - node_count >= MAX_PASSING_SLOTS {
                return None;
            }
            path.push(layer_of.len());
            layer_of.push(layer);
        }
        path.push(end);
        paths.push(path);
    }
    let layer_count = layer_of.iter().max().map_or(0, |last| last + 1);
    let mut members = vec![Vec::new(); layer_count];
    // The ends of links at frames, by layer: they take no room in their layer's stack
    // and are placed on the frame's border once its members in that layer are.
    let mut frame_ends = vec![Vec::new(); layer_count];
    for (slot, &layer) in layer_of.iter().enumerate() {
        let stack = if frame_end.contains_key(&slot) { &mut frame_ends } else { &mut members };
        stack.get_mut(layer)?.push(slot);
    }
    let mut parents = vec![Vec::new(); layer_of.len()];
    let mut children = vec![Vec::new(); layer_of.len()];
    // For each parent of a slot, the index of the link among the parent's children.
    let mut link_of = vec![Vec::new(); layer_of.len()];
    // Whether the link to each of a slot's children is drawn, indexed like `children`.
    let mut drawn = vec![Vec::new(); layer_of.len()];
    // The reach of the label each link to a slot carries where it enters the slot,
    // indexed like `parents`.
    let mut arrival_reach: Vec<Vec<Reach>> = vec![Vec::new(); layer_of.len()];
    for (path, edge) in paths.iter().zip(&chart.edges) {
        for (segment, pair) in path.windows(2).enumerate() {
            if let &[parent, child] = pair {
                let reach = entry_reach(axis, edge, path, segment);
                let siblings = children.get_mut(parent)?;
                link_of.get_mut(child)?.push(siblings.len());
                siblings.push(child);
                drawn.get_mut(parent)?.push(edge.stroke.is_visible());
                parents.get_mut(child)?.push(parent);
                arrival_reach.get_mut(child)?.push(reach);
            }
        }
    }
    // Each layer keeps declaration order unless reordering it reduces crossings with the
    // layer before.
    reduce_crossings(&mut members, &parents)?;
    // The subgraphs each slot lies in, outermost first: the innermost one holding it
    // (the first declared among equally deep ones) and those it is nested in.
    let mut innermost: Vec<Option<(usize, usize)>> = vec![None; layer_of.len()];
    for (index, subgraph) in chart.subgraphs.iter().enumerate() {
        let depth = chart.enclosing(index).len();
        for &member in &subgraph.members {
            let held = innermost.get_mut(member)?;
            if held.is_none_or(|(deepest, _)| depth > deepest) {
                *held = Some((depth, index));
            }
        }
    }
    let mut chain_of: Vec<Vec<usize>> = innermost
        .iter()
        .map(|held| {
            held.map_or_else(Vec::new, |(_, subgraph)| {
                let mut chain = chart.enclosing(subgraph);
                chain.reverse();
                chain.push(subgraph);
                chain
            })
        })
        .collect();
    // A link's passing slots lie in the subgraphs both its ends lie in, as Mermaid puts
    // the dummy nodes of a link between two members of a cluster in that cluster: the
    // frame runs around them, and the slots of other nodes stay outside it. An end at a
    // frame lies in no subgraph, so neither do the passing slots of its link.
    for path in &paths {
        let (Some(&first), Some(&last)) = (path.first(), path.last()) else { continue };
        let (from, to) = (chain_of.get(first)?, chain_of.get(last)?);
        let common: Vec<usize> =
            from.iter().zip(to).take_while(|(a, b)| a == b).map(|(&a, _)| a).collect();
        for &slot in path.get(1..path.len().saturating_sub(1)).unwrap_or(&[]) {
            *chain_of.get_mut(slot)? = common.clone();
        }
    }
    let chain_of = chain_of;
    let in_subgraph = |slot: usize, subgraph: usize| {
        chain_of.get(slot).is_some_and(|chain| chain.contains(&subgraph))
    };
    for layer in &mut members {
        group_subgraph_members(layer, &chain_of)?;
    }
    // The labels of each node's drawn self loops, in the order of their edges.
    let mut loop_labels: Vec<Vec<Option<&Label>>> = vec![Vec::new(); node_count];
    for edge in chart.edges.iter().filter(|edge| edge.stroke.is_visible()) {
        if let Some((from, to)) = edge.nodes()
            && from == to
        {
            loop_labels.get_mut(from)?.push(edge.label.as_ref());
        }
    }
    // The cells across the flow the labels of each node's self loops reach past its box,
    // before and after it: a label across the flow, in a vertical layout, or one with rows
    // above or below its middle row, in a horizontal one. The loops' legs come after the
    // node's other exits, whatever their order.
    let mut overhang = vec![(0, 0); node_count];
    for (node, labels) in loop_labels.iter().enumerate() {
        let Some(node_data) = chart.nodes.get(node) else { continue };
        if labels.iter().all(Option::is_none) {
            continue;
        }
        let targets = drawn.get(node)?.iter().filter(|&&drawn| drawn).count();
        let Exits { legs, .. } = exit_cells(
            axis,
            targets,
            labels,
            &axis.port_range(node_data),
            axis.port_offset(node_data),
        )?;
        let size = signed(axis.box_cross_size(node_data))?;
        for (&leg, label) in legs.iter().zip(labels) {
            let Some(label) = label else { continue };
            let (_, first) = self_loop_label_at(axis, leg, label)?;
            let end = first + signed(axis.label_size(label).1)?;
            let (before, after) = overhang.get_mut(node)?;
            *before = (*before).max(usize::try_from(-first).unwrap_or(0));
            *after = (*after).max(usize::try_from(end - size).unwrap_or(0));
        }
    }
    let overhang = |slot: usize| overhang.get(slot).copied().unwrap_or((0, 0));
    // Each slot's index in its layer's stack.
    let positions = |members: &[Vec<usize>]| -> Option<Vec<usize>> {
        let mut position = vec![0; layer_of.len()];
        for layer in members {
            for (index, &slot) in layer.iter().enumerate() {
                *position.get_mut(slot)? = index;
            }
        }
        // An end at a frame lies among its subgraph's members in its layer.
        for (&slot, &(subgraph, _)) in &frame_end {
            let layer = members.get(*layer_of.get(slot)?)?;
            let first_member = layer.iter().position(|&member| in_subgraph(member, subgraph));
            *position.get_mut(slot)? = first_member.unwrap_or(0);
        }
        Some(position)
    };
    // The footprint of each passing slot that carries a label, keyed by slot index: only a
    // label on a passing slot (`LabelSpot::Slot`) sizes a slot; a label in a gap gets its
    // room from `route`'s gap sizing.
    let slot_footprints: HashMap<usize, (usize, usize, usize)> = chart
        .edges
        .iter()
        .zip(&paths)
        .filter_map(|(edge, path)| {
            let Some(LabelSpot::Slot(index)) = label_spot(path) else { return None };
            Some((*path.get(index)?, labelled_slot_footprint(axis, edge.label.as_ref()?)))
        })
        .collect();
    // A passing slot is one cell across the flow, entered and left at that cell, or as
    // wide as the label it carries.
    let extent = |slot: usize| match chart.nodes.get(slot) {
        Some(node) => (axis.box_cross_size(node), axis.port_offset(node)),
        None => slot_footprints
            .get(&slot)
            .map_or((1, 0), |&(_, cross, port_offset)| (cross, port_offset)),
    };

    let frame_margin = signed(axis.frame_margins().1)?;
    // The cells across the flow a frame starting at `frame_start` needs to fit its title
    // clear of the links crossing its top border at `crossings`: the title runs across
    // the flow only in a vertical layout.
    let title_cells = |subgraph: usize, crossings: &[isize], frame_start: isize| match axis {
        Axis::Horizontal => Some(0),
        Axis::Vertical => {
            let title_width = chart.subgraphs.get(subgraph)?.title.width();
            let from_corner = crossings
                .iter()
                .filter_map(|&crossing| usize::try_from(crossing - frame_start).ok())
                .collect();
            signed(frame_width(title_width, title_offset(from_corner, title_width)))
        }
    };
    let gap = signed(sibling_gap)?;

    // Each slot goes as close as the stack allows to the mean port of its parents,
    // `sibling_gap` cells or more after the slot before it, with room for a subgraph's
    // frame wherever the stack enters or leaves the subgraph's members; no further than
    // its `ceiling` towards its parents, and no earlier than its `floor`. Returns the
    // first cell and the port of each slot.
    let place = |members: &[Vec<usize>],
                 floor: &HashMap<usize, isize>,
                 ceiling: &HashMap<usize, isize>|
     -> Option<(Vec<isize>, Vec<isize>)> {
        let position = positions(members)?;
        let mut starts = vec![0; layer_of.len()];
        let mut ports = vec![0; layer_of.len()];
        let mut exits: Vec<Vec<isize>> = vec![Vec::new(); layer_of.len()];
        for (layer_index, layer) in members.iter().enumerate() {
            // The first cell after the previous slot, or after the frame closing behind it.
            let mut end = None;
            // The subgraphs of the previous slot, outermost first, each with the first
            // cell of its frame and the cells where links cross the frame's top border,
            // estimated in each layer from its boxes: where links from outside enter them,
            // or, when the top border faces the next layer, where links to outside leave
            // them.
            let mut open: Vec<(usize, isize, Vec<isize>)> = Vec::new();
            // Where each slot of the layer would start. A parent's links leave it from
            // cells of their own, and a child goes opposite the cells its drawn links
            // leave from, so that children fan out the way their links do. A slot no drawn
            // link reaches goes just before the next slot that has such a place, so that
            // an invisible link fixes only the layer and the order of its target.
            let mut wanted_of = Vec::with_capacity(layer.len());
            // The first and last cells, from the slot's first cell, that the links entering
            // each slot take on the label rows of the gap before the layer: their entry
            // cells and their labels. `None` when no drawn link enters the slot.
            let mut row_of = Vec::with_capacity(layer.len());
            // The labels of the links entering each slot: the link's other end and the
            // first cell of its label, from the slot's first cell.
            let mut labels_of = Vec::with_capacity(layer.len());
            for &slot in layer {
                let (_, port_offset) = extent(slot);
                let (arrivals, reach): (Vec<usize>, Vec<Reach>) = parents
                    .get(slot)?
                    .iter()
                    .zip(link_of.get(slot)?)
                    .zip(arrival_reach.get(slot)?)
                    .filter_map(|((&parent, &link), &reach)| {
                        drawn.get(parent)?.get(link)?.then_some((parent, reach))
                    })
                    .unzip();
                let sources = arrivals
                    .iter()
                    .map(|&parent| ports.get(parent).copied())
                    .collect::<Option<Vec<_>>>()?;
                let cells = chart
                    .nodes
                    .get(slot)
                    .map_or(port_offset..=port_offset, |node| axis.port_range(node));
                let entries = port_cells(axis, &sources, &reach, &cells, port_offset)?;
                let mut row: Option<(isize, isize)> = None;
                let mut labels = Vec::new();
                for ((cell, reach), &parent) in entries.into_iter().zip(&reach).zip(&arrivals) {
                    let (before, after) = reach.unwrap_or((0, 0));
                    let (low, high) = (signed(cell)? - signed(before)?, signed(cell + after)?);
                    row = Some(
                        row.map_or((low, high), |(first, last)| (first.min(low), last.max(high))),
                    );
                    if reach.is_some() {
                        labels.push((parent, low));
                    }
                }
                row_of.push(row);
                labels_of.push(labels);
                let offset = signed(port_offset)?;
                let (sum, count) = parents.get(slot)?.iter().zip(link_of.get(slot)?).try_fold(
                    (0_isize, 0_isize),
                    |(sum, count), (&parent, &link)| {
                        if !*drawn.get(parent)?.get(link)? {
                            return Some((sum, count));
                        }
                        Some((sum + exits.get(parent)?.get(link)?, count + 1))
                    },
                )?;
                wanted_of.push((count > 0).then(|| sum.div_euclid(count) - offset));
            }
            // The next slot with a place, and that place.
            let mut next: Option<(usize, isize)> = None;
            for (wanted, &slot) in wanted_of.iter_mut().zip(layer).rev() {
                if wanted.is_none()
                    && let Some((next_slot, next_wanted)) = next
                {
                    // The margins of the frames closing after this slot and opening
                    // before the next lie between them too.
                    let (chain, next_chain) = (chain_of.get(slot)?, chain_of.get(next_slot)?);
                    let common = chain.iter().zip(next_chain).take_while(|(a, b)| a == b).count();
                    let frames = signed(chain.len() - common + next_chain.len() - common)?;
                    let size = signed(extent(slot).0 + overhang(slot).1)?;
                    *wanted = Some(next_wanted - gap - size - frame_margin * frames);
                }
                next = wanted.map(|wanted| (slot, wanted)).or(next);
            }
            // The last cell the previous slots take on the label rows: a line, a label, or
            // the border of a frame closing behind them that spans the layer before.
            let mut row_end: Option<isize> = None;
            for (((&slot, wanted), row), labels) in
                layer.iter().zip(wanted_of).zip(row_of).zip(labels_of)
            {
                let chain = chain_of.get(slot)?;
                // The frames this slot is not in close behind the previous slot, the
                // innermost first, around the labels of the links entering it too.
                let kept = open
                    .iter()
                    .zip(chain)
                    .take_while(|((current, ..), subgraph)| current == *subgraph)
                    .count();
                if open.len() > kept {
                    end = end.max(row_end.map(|row_end| row_end + 1));
                }
                while open.len() > kept {
                    let (current, frame_start, crossings) = open.pop()?;
                    let title_end = frame_start + title_cells(current, &crossings, frame_start)?;
                    end = end.map(|end: isize| (end + frame_margin).max(title_end));
                    // A frame whose members start in this layer has its top border below
                    // the label rows.
                    if member_layers.get(current)?.0 < layer_index {
                        row_end = row_end.max(end.map(|end| end - 1));
                    }
                }
                let lead = frame_margin * signed(chain.len() - kept)?;
                // The box keeps the gap from the box or frame before. The label rows hold
                // no box, so a label reaching past the slot keeps one blank cell from the
                // lines, labels and frame borders before it there. A frame opening here
                // encloses the labels of links from its own members, so such a label keeps
                // the gap from the box before as well, as the frame does. The labels of the
                // slot's self loops keep the gap from it as the box does.
                let (before, after) = overhang(slot);
                let (before, after) = (signed(before)?, signed(after)?);
                let opening = match chain.get(kept) {
                    Some(&outermost) => labels
                        .iter()
                        .filter(|&&(parent, _)| in_subgraph(parent, outermost))
                        .map(|&(_, first)| (-first).max(0))
                        .max()
                        .unwrap_or(0),
                    None => 0,
                };
                let box_free = end.map(|end| end + gap + lead + opening.max(before));
                let row_free =
                    row.zip(row_end).map(|((first, _), row_end)| row_end + 2 + lead - first);
                let free = box_free.max(row_free);
                let (cross_size, port_offset) = extent(slot);
                let offset = signed(port_offset)?;
                let wanted = wanted.map(|wanted| {
                    ceiling.get(&slot).map_or(wanted, |&ceiling| wanted.min(ceiling))
                });
                let start = match (wanted, free) {
                    (Some(wanted), Some(free)) => wanted.max(free),
                    (Some(at), None) | (None, Some(at)) => at,
                    (None, None) => 0,
                };
                let start = floor.get(&slot).map_or(start, |&floor| start.max(floor));
                // Each frame opening here lies one margin outside the next one in.
                for (depth, &subgraph) in chain.iter().enumerate().skip(kept) {
                    let frame_start = start - frame_margin * signed(chain.len() - depth)?;
                    open.push((subgraph, frame_start, Vec::new()));
                }
                // The cells the routing gives the links leaving this slot: a box spreads
                // them over its far border in the order of their targets in the next layer,
                // its self loops last, as `exit_ports` in `route.rs` does; an invisible
                // link takes no cell and counts as leaving from the port; a passing slot
                // uses its port.
                let leaving = match chart.nodes.get(slot) {
                    Some(node) => {
                        let (kids, drawn) = (children.get(slot)?, drawn.get(slot)?);
                        let targets = kids
                            .iter()
                            .zip(drawn)
                            .filter(|&(_, &drawn)| drawn)
                            .map(|(&child, _)| position.get(child).copied())
                            .collect::<Option<Vec<_>>>()?;
                        let exits = exit_cells(
                            axis,
                            targets.len(),
                            loop_labels.get(slot)?,
                            &axis.port_range(node),
                            port_offset,
                        )?;
                        let mut cells = scatter_by_order(&targets, &exits.targets)?.into_iter();
                        drawn
                            .iter()
                            .map(|&drawn| {
                                let cell = if drawn { cells.next()? } else { port_offset };
                                Some(start + signed(cell)?)
                            })
                            .collect::<Option<Vec<_>>>()?
                    }
                    _ => vec![start + offset; children.get(slot)?.len()],
                };
                if axis == Axis::Vertical
                    && let Some(node) = chart.nodes.get(slot)
                {
                    // The cells where links to or from the slot's other ends meet its
                    // border facing the top borders of its frames; an invisible link meets
                    // no border.
                    let ends: Vec<(usize, isize)> = if chart.direction.points_backward() {
                        children
                            .get(slot)?
                            .iter()
                            .zip(drawn.get(slot)?)
                            .zip(&leaving)
                            .filter(|&((_, &drawn), _)| drawn)
                            .map(|((&child, _), &cell)| (child, cell))
                            .collect()
                    } else {
                        // The entry cells the routing gives links arriving at this box.
                        let (arrivals, reach): (Vec<usize>, Vec<Reach>) = parents
                            .get(slot)?
                            .iter()
                            .zip(link_of.get(slot)?)
                            .zip(arrival_reach.get(slot)?)
                            .filter_map(|((&parent, &link), &reach)| {
                                drawn.get(parent)?.get(link)?.then_some((parent, reach))
                            })
                            .unzip();
                        let sources = arrivals
                            .iter()
                            .map(|&parent| ports.get(parent).copied())
                            .collect::<Option<Vec<_>>>()?;
                        let entries = port_cells(
                            axis,
                            &sources,
                            &reach,
                            &axis.port_range(node),
                            port_offset,
                        )?;
                        arrivals
                            .iter()
                            .zip(entries)
                            .map(|(&parent, entry)| Some((parent, start + signed(entry)?)))
                            .collect::<Option<_>>()?
                    };
                    for (subgraph, _, crossings) in &mut open {
                        crossings.extend(
                            ends.iter()
                                .filter(|&&(other, _)| !in_subgraph(other, *subgraph))
                                .map(|&(_, cell)| cell),
                        );
                    }
                }
                end = Some(start + signed(cross_size)? + after);
                if let Some((_, last)) = row {
                    row_end = row_end.max(Some(start + last));
                }
                *starts.get_mut(slot)? = start;
                *ports.get_mut(slot)? = start + offset;
                *exits.get_mut(slot)? = leaving;
            }
            // The links meeting one border of a frame in this layer take cells of their own
            // across the span of its members here, spread about its centre as on a box
            // border and ordered by their other ends.
            let mut borders: Vec<((usize, bool), Vec<usize>)> = Vec::new();
            for &slot in frame_ends.get(layer_index)? {
                let border = *frame_end.get(&slot)?;
                match borders.iter_mut().find(|(other, _)| *other == border) {
                    Some((_, ends)) => ends.push(slot),
                    None => borders.push((border, vec![slot])),
                }
            }
            for ((subgraph, entering), ends) in borders {
                let spans = layer
                    .iter()
                    .filter(|&&member| in_subgraph(member, subgraph))
                    .map(|&member| {
                        let start = *starts.get(member)?;
                        let size = axis.box_cross_size(chart.nodes.get(member)?);
                        Some((start, start + signed(size)? - 1))
                    })
                    .collect::<Option<Vec<_>>>()?;
                let low = spans.iter().map(|&(low, _)| low).min()?;
                let high = spans.iter().map(|&(_, high)| high).max()?;
                let others = ends
                    .iter()
                    .map(|&end| {
                        if entering {
                            let (&parent, &link) =
                                parents.get(end)?.first().zip(link_of.get(end)?.first())?;
                            exits.get(parent)?.get(link).copied()
                        } else {
                            let &child = children.get(end)?.first()?;
                            signed(*position.get(child)?)
                        }
                    })
                    .collect::<Option<Vec<_>>>()?;
                let span = usize::try_from(high - low).ok()?;
                for (&end, cell) in
                    ends.iter().zip(port_cells(axis, &others, &[], &(0..=span), span / 2)?)
                {
                    let at = low + signed(cell)?;
                    *starts.get_mut(end)? = at;
                    *ports.get_mut(end)? = at;
                    *exits.get_mut(end)? = vec![at; children.get(end)?.len()];
                }
            }
        }
        Some((starts, ports))
    };

    // A frame keeps the cells across the flow it covers clear of non-members in every
    // layer it spans: a node placed inside a frame it does not belong to is placed again
    // after it, and the frame's members no further out than the frame reached, until no
    // frame covers a stranger; the routing gives up on a drawing where one still does.
    let is_member: Vec<Vec<bool>> = chart
        .subgraphs
        .iter()
        .map(|subgraph| subgraph.membership(node_count))
        .collect::<Option<_>>()?;
    // Inner frames first, so that each outer one is measured around them.
    let innermost_first = chart.innermost_first();
    let mut floor = HashMap::new();
    let mut ceiling = HashMap::new();
    let mut rounds = 0;
    let (starts, ports) = loop {
        let (starts, ports) = place(&members, &floor, &ceiling)?;
        rounds += 1;
        // A slot's cells across the flow, with the labels of its self loops.
        let span = |slot: usize| {
            let (before, after) = overhang(slot);
            let start = *starts.get(slot)?;
            Some((start - signed(before)?, start + signed(extent(slot).0 + after)? - 1))
        };
        // Each frame's first and last cells across the flow, around its members' boxes,
        // the passing slots of the links between its members and its inner frames.
        let mut bands: Vec<Option<(isize, isize)>> = vec![None; chart.subgraphs.len()];
        for &index in &innermost_first {
            let subgraph = chart.subgraphs.get(index)?;
            let passing = (node_count..layer_of.len()).filter(|&slot| in_subgraph(slot, index));
            let boxes = subgraph.members.iter().copied().chain(passing).map(span);
            let inner = chart
                .subgraphs
                .iter()
                .zip(&bands)
                .filter(|(inner, _)| inner.parent == Some(index))
                .filter_map(|(_, band)| band.map(Some));
            let spans = boxes.chain(inner).collect::<Option<Vec<_>>>()?;
            let low = spans.iter().map(|&(low, _)| low).min()? - frame_margin;
            let high = (spans.iter().map(|&(_, high)| high).max()? + frame_margin)
                .max(low + title_cells(index, &[], low)? - 1);
            *bands.get_mut(index)? = Some((low, high));
        }
        let mut strays = Vec::new();
        for (index, ((subgraph, is_member), band)) in
            chart.subgraphs.iter().zip(&is_member).zip(&bands).enumerate()
        {
            let &Some((low, high)) = band else { continue };
            let layers = subgraph.members.iter().map(|&member| layer_of.get(member).copied());
            let layers = layers.collect::<Option<Vec<_>>>()?;
            let (first, last) = (*layers.iter().min()?, *layers.iter().max()?);
            for slot in 0..node_count {
                let (slot_low, slot_high) = span(slot)?;
                if (first..=last).contains(layer_of.get(slot)?)
                    && is_member.get(slot) == Some(&false)
                    && slot_low <= high
                    && low <= slot_high
                {
                    strays.push((slot, index, high));
                }
            }
        }
        if strays.is_empty() || rounds > node_count {
            break (starts, ports);
        }
        for (stray, subgraph, high) in strays {
            let lead = frame_margin * signed(chain_of.get(stray)?.len())?;
            // The labels of the stray's self loops move out of the frame with it.
            let after = high + 1 + gap + lead + signed(overhang(stray).0)?;
            floor
                .entry(stray)
                .and_modify(|floor: &mut isize| *floor = (*floor).max(after))
                .or_insert(after);
            for &member in &chart.subgraphs.get(subgraph)?.members {
                // A member nested deeper lies a margin further in for each frame between.
                let chain = chain_of.get(member)?;
                let depth = chain.len() - chain.iter().position(|&outer| outer == subgraph)?;
                let size = extent(member).0 + overhang(member).1;
                let inside = high - frame_margin * signed(depth)? - signed(size)? + 1;
                ceiling
                    .entry(member)
                    .and_modify(|ceiling: &mut isize| *ceiling = (*ceiling).min(inside))
                    .or_insert(inside);
            }
            // In a layer holding members of the frame or passing slots inside it, the stray
            // moves after them, so that its place in the stack agrees with the floor that
            // puts it after the frame.
            let layer = members.get_mut(*layer_of.get(stray)?)?;
            let last_member = layer.iter().rposition(|&slot| in_subgraph(slot, subgraph));
            if let (Some(at), Some(last_member)) =
                (layer.iter().position(|&slot| slot == stray), last_member)
                && at < last_member
            {
                let stray = layer.remove(at);
                layer.insert(last_member, stray);
            }
        }
    };

    let origin = starts.iter().copied().min().unwrap_or(0);
    let slots = layer_of
        .iter()
        .zip(starts.iter().zip(&ports))
        .enumerate()
        .map(|(slot, (&layer, (&start, &port)))| {
            let main = match chart.nodes.get(slot) {
                Some(node) => axis.box_main_size(node),
                None => slot_footprints.get(&slot).map_or(0, |&(main, ..)| main),
            };
            Some(Slot {
                layer,
                cross: usize::try_from(start - origin).ok()?,
                port: usize::try_from(port - origin).ok()?,
                main,
            })
        })
        .collect::<Option<_>>()?;
    Some(Layered { slots, layer_count, paths, reversed })
}

/// The hidden node an empty subgraph holds, if `subgraph` is one.
fn hidden_member(chart: &Flowchart, subgraph: usize) -> Option<usize> {
    match chart.subgraphs.get(subgraph)?.members.as_slice() {
        &[member] if matches!(chart.nodes.get(member)?.body, Body::Hidden) => Some(member),
        _ => None,
    }
}

/// The end a link is laid out to: an empty subgraph's hidden member, which is placed
/// like a node, or else `end` itself.
fn laid_out_end(chart: &Flowchart, end: End) -> End {
    match end {
        End::Subgraph(subgraph) if let Some(node) = hidden_member(chart, subgraph) => {
            End::Node(node)
        }
        _ => end,
    }
}

/// Reorders each layer after the first by the mean position of its slots' parents in
/// the layer before, keeping the new order only when fewer links cross between the
/// two layers. A single sweep from the first layer keeps the work bounded; ties keep
/// the earlier order, which starts as declaration order.
fn reduce_crossings(members: &mut [Vec<usize>], parents: &[Vec<usize>]) -> Option<()> {
    let mut position = vec![0; parents.len()];
    for layer in members.iter() {
        for (index, &slot) in layer.iter().enumerate() {
            *position.get_mut(slot)? = index;
        }
    }
    for layer in members.iter_mut().skip(1) {
        let barycentre = |slot: usize| -> Option<(u64, u64)> {
            let parents = parents.get(slot)?;
            if parents.is_empty() {
                return Some((*position.get(slot)? as u64, 1));
            }
            let sum = parents.iter().map(|&parent| position.get(parent)).sum::<Option<usize>>()?;
            Some((sum as u64, parents.len() as u64))
        };
        let mut keyed = layer
            .iter()
            .map(|&slot| Some((barycentre(slot)?, slot)))
            .collect::<Option<Vec<_>>>()?;
        keyed.sort_by(|((sum_a, count_a), _), ((sum_b, count_b), _)| {
            (sum_a * count_b).cmp(&(sum_b * count_a))
        });
        let reordered: Vec<usize> = keyed.into_iter().map(|(_, slot)| slot).collect();
        if crossings(&reordered, parents, &position)? < crossings(layer, parents, &position)? {
            *layer = reordered;
            for (index, &slot) in layer.iter().enumerate() {
                *position.get_mut(slot)? = index;
            }
        }
    }
    Some(())
}

/// Moves the slots of each subgraph in `layer` up to the first of them, so that no
/// other slot lies between them, and those of a nested subgraph likewise within the
/// run of the one enclosing it, keeping the order of the slots otherwise. `chain_of`
/// lists the subgraphs each slot lies in, outermost first.
fn group_subgraph_members(layer: &mut [usize], chain_of: &[Vec<usize>]) -> Option<()> {
    let mut first_at = HashMap::new();
    for (index, &slot) in layer.iter().enumerate() {
        for &subgraph in chain_of.get(slot)? {
            first_at.entry(subgraph).or_insert(index);
        }
    }
    // A slot sorts by where each of its subgraphs first appears, outermost first, then
    // by its own place.
    let mut keyed = layer
        .iter()
        .enumerate()
        .map(|(index, &slot)| {
            let mut key = chain_of
                .get(slot)?
                .iter()
                .map(|subgraph| first_at.get(subgraph).copied())
                .collect::<Option<Vec<_>>>()?;
            key.push(index);
            Some((key, slot))
        })
        .collect::<Option<Vec<_>>>()?;
    keyed.sort_by(|(key_a, _), (key_b, _)| key_a.cmp(key_b));
    for (place, (_, slot)) in layer.iter_mut().zip(keyed) {
        *place = slot;
    }
    Some(())
}

/// The number of pairs of links from the layer before into `layer`, stacked in this
/// order, that cross, given the positions of the parents in their layer.
///
/// Sorting the links by the parent's position leaves one crossing for each pair
/// whose child positions are out of order; a Fenwick tree over the child positions
/// counts those pairs in O(links log slots) rather than comparing every pair (Barth,
/// Jünger and Mutzel, "Simple and Efficient Bilayer Cross Counting", 2004).
fn crossings(layer: &[usize], parents: &[Vec<usize>], position: &[usize]) -> Option<usize> {
    let mut links = Vec::new();
    for (child, &slot) in layer.iter().enumerate() {
        for &parent in parents.get(slot)? {
            links.push((*position.get(parent)?, child));
        }
    }
    links.sort_unstable();
    let mut tree = vec![0_usize; layer.len() + 1];
    let mut crossed = 0;
    for (seen, &(_, child)) in links.iter().enumerate() {
        let mut at_or_before = 0;
        let mut index = child + 1;
        while index > 0 {
            at_or_before += tree.get(index)?;
            index &= index - 1;
        }
        crossed += seen - at_or_before;
        let mut index = child + 1;
        while let Some(count) = tree.get_mut(index) {
            *count += 1;
            index += index.isolate_lowest_one();
        }
    }
    Some(crossed)
}

/// Marks the links that would close a cycle with the links declared before them, each
/// link standing for the order it imposes on the nodes (see [`link_order`]); a marked
/// link is laid out reversed, which leaves no cycle. Earlier links keep their
/// direction, so a cycle is broken at its last declared link. `None` when a link joins
/// a subgraph to itself or to one of its own members.
fn cycle_closing_edges(chart: &Flowchart) -> Option<Vec<bool>> {
    let mut children = vec![Vec::new(); chart.nodes.len()];
    let mut closing = Vec::with_capacity(chart.edges.len());
    for edge in &chart.edges {
        // A self loop orders nothing.
        let pairs: Vec<(usize, usize)> =
            link_order(chart, edge)?.into_iter().filter(|(from, to)| from != to).collect();
        // Every source of the link precedes every target, so the link closes a cycle
        // when any source can already be reached from any target.
        let mut reached = vec![false; chart.nodes.len()];
        let mut stack: Vec<usize> = pairs.iter().map(|&(_, to)| to).collect();
        while let Some(node) = stack.pop() {
            let seen = reached.get_mut(node)?;
            if !*seen {
                *seen = true;
                stack.extend(children.get(node)?);
            }
        }
        let closes = pairs.iter().any(|&(from, _)| reached.get(from) == Some(&true));
        for (from, to) in pairs {
            let (from, to) = if closes { (to, from) } else { (from, to) };
            children.get_mut(from)?.push(to);
        }
        closing.push(closes);
    }
    Some(closing)
}

/// The pairs of nodes `(from, to)` whose order along the flow `edge` imposes, in its
/// written direction: a link between nodes orders its ends; a link into a subgraph
/// puts every member after the source, a link out of one puts the target after every
/// member, and a link between two puts every member of the second after every member
/// of the first. `None` when the link joins a subgraph to itself or to one of its own
/// members, which has no layout here.
fn link_order(chart: &Flowchart, edge: &Edge) -> Option<Vec<(usize, usize)>> {
    if let Some(pair) = edge.nodes() {
        return Some(vec![pair]);
    }
    let nodes_at = |end: End| -> Option<Vec<usize>> {
        match end {
            End::Node(node) => Some(vec![node]),
            End::Subgraph(subgraph) => Some(chart.subgraphs.get(subgraph)?.members.clone()),
        }
    };
    let mut pairs = Vec::new();
    for from in nodes_at(edge.from)? {
        for to in nodes_at(edge.to)? {
            if from == to {
                return None;
            }
            pairs.push((from, to));
        }
    }
    Some(pairs)
}

/// The pairs of nodes `(from, to, length)` whose layers the links impose (see
/// [`link_order`]), each link in layout order, `reversed` closing a cycle. `None` when
/// a link joins a subgraph to itself or to one of its own members.
fn layering_constraints(
    chart: &Flowchart,
    reversed: &[bool],
) -> Option<Vec<(usize, usize, usize)>> {
    let mut constraints = Vec::new();
    for (edge, &reversed) in chart.edges.iter().zip(reversed) {
        for (from, to) in link_order(chart, edge)? {
            let (from, to) = if reversed { (to, from) } else { (from, to) };
            constraints.push((from, to, edge.length));
        }
    }
    Some(constraints)
}

/// The layer of each of `count` nodes, given `constraints` `(from, to, length)`: `to` at
/// least `length` layers after `from`. `None` when the constraints form a cycle.
fn longest_path_layers(count: usize, constraints: &[(usize, usize, usize)]) -> Option<Vec<usize>> {
    let mut unplaced_parents = vec![0_usize; count];
    let mut children = vec![Vec::new(); count];
    for &(from, to, length) in constraints.iter().filter(|(from, to, _)| from != to) {
        children.get_mut(from)?.push((to, length));
        *unplaced_parents.get_mut(to)? += 1;
    }
    let mut layer_of = vec![0_usize; count];
    let mut ready: Vec<usize> =
        (0..count).filter(|&node| unplaced_parents.get(node) == Some(&0)).collect();
    let mut placed = 0;
    while let Some(node) = ready.pop() {
        placed += 1;
        let layer = *layer_of.get(node)?;
        for &(child, length) in children.get(node)? {
            let child_layer = layer_of.get_mut(child)?;
            *child_layer = (*child_layer).max(layer.saturating_add(length));
            let unplaced = unplaced_parents.get_mut(child)?;
            *unplaced -= 1;
            if *unplaced == 0 {
                ready.push(child);
            }
        }
    }
    (placed == count).then_some(layer_of)
}
