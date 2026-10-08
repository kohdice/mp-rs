//! Assigning flowchart nodes to layers along the flow and to positions across it.

mod growth;
mod layers;
mod order;
mod place;
mod ports;
mod reach;
mod size;

use std::collections::HashMap;
use std::ops::RangeInclusive;

use super::parse::{Body, Direction, End, Flowchart, Node};
use super::signed;

use self::growth::{drawing_port_inset, vertical_port_inset};
use self::order::{group_subgraph_members, reduce_crossings};
use self::place::{Placement, SlotGraph};
use self::size::{FRAME_COLS, FRAME_ROWS, Spacing, box_height, label_row};

pub(super) use self::growth::{grow_boxes, grow_for_labels, grow_frames};
pub(super) use self::ports::{
    SELF_LOOP_CELLS, SELF_LOOP_SPAN, exit_cells, port_cells, scatter_by_order, self_loop_cells,
    self_loop_label_at,
};
pub(super) use self::reach::{
    LabelSpot, Reach, entry_reach, label_cross_reach, label_reach, label_spot,
};
pub(super) use self::size::{TITLE_CORNER_OFFSET, box_width, frame_width, title_offset};

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
    /// that links leave and enter: the middle label row (see
    /// [`Label::middle_row`](super::label::Label::middle_row)), or the centre column.
    pub(super) fn port_offset(self, node: &Node) -> usize {
        match self {
            Self::Horizontal => label_row(node) + node.spread,
            Self::Vertical => self.box_cross_size(node) / 2,
        }
    }

    /// The cells across the flow, counted from a box's first cell, where links may meet
    /// its borders along the flow: in a horizontal layout the middle label row and the
    /// rows the box grew by around it, as many on either side as its spread, which leaves
    /// out a cylinder's arc row; in a vertical one every column between the corners of
    /// both the top and the bottom border, so that a border inset further than one
    /// column, as an ellipse's or a triangle's is, keeps links off the blanks beside it.
    pub(super) fn port_range(self, node: &Node) -> RangeInclusive<usize> {
        match (self, &node.body) {
            // A drawing's links meet its frame's side anywhere between the corners.
            (Self::Horizontal, Body::Drawing { .. }) => {
                1..=self.box_cross_size(node).saturating_sub(2)
            }
            // The title sits by the top-left corner only, but the right end is inset as
            // far so that the ports stay symmetric about the box's centre, as every
            // box's are.
            (Self::Vertical, Body::Drawing { title_width, .. }) => {
                let inset = drawing_port_inset(*title_width);
                inset..=self.box_cross_size(node).saturating_sub(1 + inset)
            }
            (Self::Horizontal, Body::Box { .. } | Body::Hidden) => {
                let first = label_row(node);
                first..=first + 2 * node.spread
            }
            (Self::Vertical, Body::Hidden) => 0..=2 * node.spread,
            (Self::Vertical, Body::Box { shape, .. }) => {
                let inset = vertical_port_inset(*shape);
                inset..=self.box_cross_size(node).saturating_sub(1 + inset)
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

    /// The spacing at tightening step `level`: step 0 is the default and each later step
    /// is tighter, tried in turn when the drawing is too wide or spans more than
    /// [`MAX_CELLS`](super::MAX_CELLS). Past the axis's last step the spacing stays at
    /// that step.
    pub(super) fn spacing(self, level: usize) -> Spacing {
        // A sibling gap of one is the least that keeps neighbouring boxes from touching.
        // Upstream separates nodes and edges by `nodesep` and `edgesep` pixels; a cell is
        // the smallest distance text has, so gaps are counted in whole cells. A terminal
        // cell is about twice as tall as it is wide, so the default gap between siblings
        // is one blank row in a horizontal layout and two blank columns in a vertical one,
        // which look about the same distance apart.
        match (self, level) {
            // `────►`, with room on the line for a two-cell label such as `no` between a
            // line cell on either side and the arrowhead before the gap has to widen.
            (Self::Horizontal, 0) => Spacing { layer_gap: 5, sibling_gap: 1 },
            (Self::Horizontal, 1) => Spacing { layer_gap: 3, sibling_gap: 1 },
            // `─►`, the narrowest gap that still shows a line before the arrowhead.
            (Self::Horizontal, _) => Spacing { layer_gap: 2, sibling_gap: 1 },
            (Self::Vertical, 0) => Spacing { layer_gap: 3, sibling_gap: 2 },
            (Self::Vertical, _) => Spacing { layer_gap: 3, sibling_gap: 1 },
        }
    }
}

/// The number of tightening steps [`Axis::spacing`] tells apart, counted on the axis
/// with the most. The axis with fewer stays at its last step past them, so the steps of
/// both axes advance together.
pub(super) const LEVELS: usize = 3;

/// A place in a layer: a node's box, the cells a link longer than one layer passes
/// through, or the end of a link at a subgraph frame. A frame end is made for every link
/// ending at a frame, not only for links spanning several layers, and takes no room in
/// its layer's stack.
#[derive(Debug)]
pub(super) struct Slot {
    pub layer: usize,
    /// The first cell across the flow.
    pub cross: usize,
    /// The cell across the flow where links leave and enter.
    pub port: usize,
    /// The slot's rank in its layer's stack, as `rank_layer` in the `order` module sets
    /// it: twice its index plus one, and for the end of a link at a frame twice the index
    /// of the frame's first member there, so that the end ranks just before that member.
    /// Only the order of the ranks is meaningful. The links leaving a box are spread over
    /// its border in this order of their targets, ties keeping the order of the links,
    /// both where the layout places the targets and where the routing draws the links.
    pub order: usize,
    /// The cells the slot takes along the flow; `route` sizes a layer as the largest `main`
    /// of its slots. A node's box takes its size along the flow. A passing slot carrying a
    /// label takes its widest row plus a line cell on either side in a horizontal layout
    /// (`label.width() + 2`), and its rows (`label.height()`) in a vertical one, so that a
    /// layer with no node (as under `A -----> B`) still has room for the label. Any other
    /// passing slot takes 0.
    pub main: usize,
}

/// A laid-out flowchart: its slots with their layers and places across the flow, the
/// path of each link through them, and what each subgraph's frame needs.
#[derive(Debug)]
pub(super) struct Layered {
    /// The chart's nodes, indexed like [`Flowchart::nodes`], followed by the slots of
    /// links that span several layers and the ends of links at subgraph frames (see
    /// [`Slot`]).
    pub slots: Vec<Slot>,
    pub layer_count: usize,
    /// The slots each edge passes through, one per layer from the earlier of its ends to
    /// the later, indexed like [`Flowchart::edges`]; a self loop's path is its node
    /// alone.
    pub paths: Vec<Vec<usize>>,
    /// Whether each edge runs against the flow, from its source in a later layer back
    /// to its target, indexed like [`Flowchart::edges`].
    pub reversed: Vec<bool>,
    /// The fewest cells each subgraph's frame must grow by on either side of its members
    /// for the links meeting it in this layout to fit its borders (see `frame_ports`),
    /// indexed like [`Flowchart::subgraphs`]; 0 for a frame no drawn link meets.
    pub frame_needs: Vec<usize>,
    /// The first and last layers holding members of each subgraph, indexed like
    /// [`Flowchart::subgraphs`].
    pub member_layers: Vec<(usize, usize)>,
}

/// Bounds the slots added beyond the nodes: the cells long links pass through and the
/// ends of links at frames. The passing slots grow with the product of the number of
/// edges and the number of layers, so a small input could otherwise demand an enormous
/// drawing. The bound is this crate's own, not upstream's: a terminal drawing must bound
/// its memory and time even when no width limits it.
const MAX_PASSING_SLOTS: usize = 10_000;

/// Places every node in the first layer that is, for each parent, at least the link's
/// `length` layers after that parent: the layer after its furthest parent when every
/// link has length one. Edges closing a cycle are laid out reversed. Then orders each
/// layer to reduce crossings with the layer before, keeps each subgraph's slots
/// together, and places every slot across the flow, `sibling_gap` cells or more apart,
/// moving a node out of a frame it does not belong to until no frame covers such a stray
/// or more rounds than there are nodes have run. `None` when a link
/// joins a subgraph to itself or to one of its own members (see
/// [`cycle_closing_edges`](layers::cycle_closing_edges)), or when the slots added beyond
/// the nodes would number more than [`MAX_PASSING_SLOTS`].
pub(super) fn lay_out(chart: &Flowchart, axis: Axis, sibling_gap: usize) -> Option<Layered> {
    let (graph, members) = SlotGraph::new(chart, axis)?;
    let layer_count = members.len();
    let members =
        reduce_crossings(members, &graph.parents, &graph.frame_end_slots, &graph.chain_of)?;
    let mut members = members
        .iter()
        .map(|layer| group_subgraph_members(layer, &graph.chain_of))
        .collect::<Option<Vec<_>>>()?;
    let gap = signed(sibling_gap)?;
    let is_member = chart.memberships()?;
    // Inner frames first, so that each outer one is measured around them.
    let innermost_first = chart.innermost_first();
    let mut floor = HashMap::new();
    let mut ceiling = HashMap::new();
    let mut rounds = 0;
    // A frame keeps the cells across the flow it covers clear of non-members in every
    // layer it spans. A stray is a node placed inside a frame it does not belong to, or
    // whose own frames would meet that frame: each round places the strays again after
    // the frame, and the frame's members no further out than the frame reached, until no
    // frame covers a stray. Every round moves at least one stray outwards, so more rounds
    // than nodes means the places are not settling; the routing gives up on a drawing
    // where a frame still covers a stray.
    let Placement { starts, ports, needs: frame_needs, .. } = loop {
        let placement = graph.place(&members, &floor, &ceiling, sibling_gap)?;
        let starts = &placement.starts;
        rounds += 1;
        // A slot's cells across the flow, with the labels of its self loops.
        let span = |slot: usize| {
            let (before, after) = graph.overhang(slot);
            let start = *starts.get(slot)?;
            Some((start - signed(before)?, start + signed(graph.extent(slot).0 + after)? - 1))
        };
        // Each frame's first and last cells across the flow, around its members' boxes,
        // the passing slots of the links between its members and its inner frames.
        let mut bands: Vec<Option<(isize, isize)>> = vec![None; chart.subgraphs.len()];
        for &index in &innermost_first {
            let subgraph = chart.subgraphs.get(index)?;
            let passing = (graph.node_count..graph.layer_of.len())
                .filter(|&slot| graph.in_subgraph(slot, index));
            let boxes = subgraph.members.iter().copied().chain(passing).map(span);
            let inner = chart
                .subgraphs
                .iter()
                .zip(&bands)
                .filter(|(inner, _)| inner.parent == Some(index))
                .filter_map(|(_, band)| band.map(Some));
            let spans = boxes.chain(inner).collect::<Option<Vec<_>>>()?;
            let pad = graph.pads(&[index])?;
            let low = spans.iter().map(|&(low, _)| low).min()? - pad;
            let high = (spans.iter().map(|&(_, high)| high).max()? + pad).max(
                low + graph.title_cells(index, &graph.top_crossings(index, &placement)?, low)? - 1,
            );
            *bands.get_mut(index)? = Some((low, high));
        }
        let mut strays = Vec::new();
        for (index, (is_member, band)) in is_member.iter().zip(&bands).enumerate() {
            let &Some((low, high)) = band else { continue };
            let &(first, last) = graph.member_layers.get(index)?;
            for slot in 0..graph.node_count {
                // A stray's own frames, those not enclosing this one, take their margins
                // and growth around its box, and may not meet this frame either.
                let own_frames: Vec<usize> = graph
                    .chain_of
                    .get(slot)?
                    .iter()
                    .copied()
                    .filter(|&outer| !chart.encloses(outer, index))
                    .collect();
                let lead = graph.pads(&own_frames)?;
                let (slot_low, slot_high) = span(slot)?;
                let (slot_low, slot_high) = (slot_low - lead, slot_high + lead);
                if (first..=last).contains(graph.layer_of.get(slot)?)
                    && is_member.get(slot) == Some(&false)
                    && slot_low <= high
                    && low <= slot_high
                {
                    strays.push((slot, index, high, lead));
                }
            }
        }
        if strays.is_empty() || rounds > graph.node_count {
            break placement;
        }
        for (stray, subgraph, high, lead) in strays {
            // The labels of the stray's self loops move out of the frame with it.
            let after = high + 1 + gap + lead + signed(graph.overhang(stray).0)?;
            floor
                .entry(stray)
                .and_modify(|floor: &mut isize| *floor = (*floor).max(after))
                .or_insert(after);
            for &member in &chart.subgraphs.get(subgraph)?.members {
                // A member lies the margin and growth of the frame it sits in, and of each
                // frame nested between, further in.
                let chain = graph.chain_of.get(member)?;
                let depth = chain.iter().position(|&outer| outer == subgraph)?;
                let size = graph.extent(member).0 + graph.overhang(member).1;
                let inside = high - graph.pads(chain.get(depth..)?)? - signed(size)? + 1;
                ceiling
                    .entry(member)
                    .and_modify(|ceiling: &mut isize| *ceiling = (*ceiling).min(inside))
                    .or_insert(inside);
            }
            // In a layer holding members of the frame or passing slots inside it, the stray
            // moves after them, so that its place in the stack agrees with the floor that
            // puts it after the frame.
            let layer = members.get_mut(*graph.layer_of.get(stray)?)?;
            let last_member = layer.iter().rposition(|&slot| graph.in_subgraph(slot, subgraph));
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
    let orders = graph.positions(&members)?;
    let slots = graph
        .layer_of
        .iter()
        .zip(starts.iter().zip(&ports).zip(orders))
        .enumerate()
        .map(|(slot, (&layer, ((&start, &port), order)))| {
            let main = match chart.nodes.get(slot) {
                Some(node) => axis.box_main_size(node),
                None => graph.slot_footprints.get(&slot).map_or(0, |footprint| footprint.main),
            };
            Some(Slot {
                layer,
                cross: usize::try_from(start - origin).ok()?,
                port: usize::try_from(port - origin).ok()?,
                order,
                main,
            })
        })
        .collect::<Option<_>>()?;
    Some(Layered {
        slots,
        layer_count,
        paths: graph.paths,
        reversed: graph.reversed,
        frame_needs,
        member_layers: graph.member_layers,
    })
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
