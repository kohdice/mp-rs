//! The slots of a layout and their place across the flow, dagre's `position`: each slot
//! as near the cells its parents' links leave from as its layer's stack and the frames
//! around it allow.
//!
//! The *label rows* of a layer are the cells along the flow just before it, in the gap
//! from the layer before, where the lines entering the layer turn and carry their labels;
//! no box lies on them, so what a slot's entries take there is measured apart from the
//! box.

use std::collections::HashMap;

use super::layers::{cycle_closing_edges, layering_constraints, longest_path_layers};
use super::order::{FrameEnd, in_subgraph, stack_ranks};
use super::ports::{
    Exits, exit_cells, frame_growth, frame_ports, port_cells, reach_in_order, scatter_by_order,
    self_loop_label_at,
};
use super::reach::{
    Crossing, Footprint, LabelSpot, Reach, entry_reach, label_spot, labelled_slot_footprint,
};
use super::size::{frame_width, title_offset};
use super::{Axis, MAX_PASSING_SLOTS, laid_out_end};
use crate::diagram::flowchart::label::Label;
use crate::diagram::flowchart::parse::{End, Flowchart};
use crate::diagram::flowchart::signed;

/// One placement of every slot across the flow, as [`SlotGraph::place`] makes it.
pub(super) struct Placement {
    /// The first cell of each slot.
    pub(super) starts: Vec<isize>,
    /// The cell of each slot where links leave and enter.
    pub(super) ports: Vec<isize>,
    /// The fewest cells each subgraph's frame must grow by on either side (see
    /// [`Layered::frame_needs`](super::Layered::frame_needs)).
    pub(super) needs: Vec<usize>,
    /// The cells where the drawn links entering each slot meet it.
    entered: Vec<Vec<isize>>,
    /// The cell where each link leaving a slot leaves it, indexed like its children.
    exits: Vec<Vec<isize>>,
}

/// The drawn links entering one slot of a layer: what they take on the layer's label rows
/// (see the module doc), and where that puts the slot.
struct Inbound {
    /// Where the slot would start, from its drawn links; `None` when none reaches it.
    wanted: Option<isize>,
    /// The first and last cells, from the slot's first cell, the entries and their labels
    /// take.
    row: Option<(isize, isize)>,
    /// Each label's other end and the first cell of the label, from the slot's first cell.
    labels: Vec<(usize, isize)>,
    /// Each drawn link entering the slot.
    arrivals: Vec<Arrival>,
}

/// One drawn link entering a slot.
struct Arrival {
    /// The slot at the link's other end.
    from: usize,
    /// The cell, from the slot's first cell, where the link enters.
    cell: usize,
    /// The reach of the link's label at the entry, if it has one.
    reach: Reach,
}

/// A subgraph frame that the slot last placed in a layer lies in, which the next slot
/// closes unless it lies in the frame too.
struct OpenFrame {
    subgraph: usize,
    /// The first cell of the frame across the flow.
    start: isize,
    /// The cells where links cross the frame's top border, estimated in each layer from
    /// its boxes: where links from outside enter them, or, when the top border faces the
    /// next layer, where links to outside leave them.
    crossings: Vec<isize>,
}

/// The link whose end lies at a frame border, seen from that end.
struct FrameLink {
    /// Where the link's other end lies across the flow: the cell it leaves its source at
    /// for a link entering the frame, or its target's rank in the stack for one leaving.
    other: isize,
    drawn: bool,
    /// The reach of the link's label at the frame; a link leaving the frame has none.
    reach: Reach,
}

/// The slots of a layout and the links between them: what [`lay_out`](super::lay_out)
/// derives from the chart before it places the slots across the flow.
pub(super) struct SlotGraph<'a> {
    chart: &'a Flowchart,
    axis: Axis,
    /// See [`Layered::reversed`](super::Layered::reversed).
    pub(super) reversed: Vec<bool>,
    /// The layer of each slot: the chart's nodes, indexed like [`Flowchart::nodes`],
    /// followed by the other slots (see [`Layered::slots`](super::Layered::slots)).
    pub(super) layer_of: Vec<usize>,
    pub(super) node_count: usize,
    /// The first and last layers of each subgraph's members, where links to and from it
    /// meet its frame.
    pub(super) member_layers: Vec<(usize, usize)>,
    /// For each slot past the nodes that is the end of a link at a frame rather than a
    /// passing slot: the subgraph, and whether the link enters it.
    frame_end: HashMap<usize, (usize, bool)>,
    /// The ends of links at frames, each with its layer and its subgraph.
    pub(super) frame_end_slots: Vec<FrameEnd>,
    /// The ends of links at frames, by layer: they take no room in their layer's stack
    /// and are placed on the frame's border once its members in that layer are.
    frame_ends: Vec<Vec<usize>>,
    /// See [`Layered::paths`](super::Layered::paths).
    pub(super) paths: Vec<Vec<usize>>,
    pub(super) parents: Vec<Vec<usize>>,
    children: Vec<Vec<usize>>,
    /// For each parent of a slot, the index of the link among the parent's children.
    link_of: Vec<Vec<usize>>,
    /// Whether the link to each of a slot's children is drawn, indexed like `children`.
    drawn: Vec<Vec<bool>>,
    /// The reach of the label each link to a slot carries where it enters the slot,
    /// indexed like `parents`.
    arrival_reach: Vec<Vec<Reach>>,
    /// The subgraphs each slot lies in, outermost first.
    pub(super) chain_of: Vec<Vec<usize>>,
    /// The labels of each node's drawn self loops, in the order of their edges.
    loop_labels: Vec<Vec<Option<&'a Label>>>,
    /// The cells across the flow the labels of each node's self loops reach past its box,
    /// before and after it.
    overhang: Vec<(usize, usize)>,
    /// The footprint of each passing slot that carries a label, keyed by slot index (see
    /// [`labelled_slot_footprint`]).
    pub(super) slot_footprints: HashMap<usize, Footprint>,
}

impl<'a> SlotGraph<'a> {
    /// Assigns every slot to a layer and links the slots, returning also each layer's
    /// stack of slots in declaration order. `None` as for [`lay_out`](super::lay_out).
    pub(super) fn new(
        chart: &'a Flowchart,
        axis: Axis,
    ) -> Option<(SlotGraph<'a>, Vec<Vec<usize>>)> {
        let reversed = cycle_closing_edges(chart)?;
        let mut layer_of =
            longest_path_layers(chart.nodes.len(), &layering_constraints(chart, &reversed)?)?;
        let node_count = chart.nodes.len();
        let member_layers = chart
            .subgraphs
            .iter()
            .map(|subgraph| {
                let layers = subgraph.members.iter().map(|&member| layer_of.get(member).copied());
                let layers = layers.collect::<Option<Vec<_>>>()?;
                Some((*layers.iter().min()?, *layers.iter().max()?))
            })
            .collect::<Option<Vec<_>>>()?;
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
            // The layering puts a frame after the links into it and before the links out of
            // it, so the ends of every link other than a self loop (handled above) lie in
            // different layers.
            if from >= to {
                return None;
            }
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
        let mut frame_ends = vec![Vec::new(); layer_count];
        for (slot, &layer) in layer_of.iter().enumerate() {
            let stack = if frame_end.contains_key(&slot) { &mut frame_ends } else { &mut members };
            stack.get_mut(layer)?.push(slot);
        }
        let mut parents = vec![Vec::new(); layer_of.len()];
        let mut children = vec![Vec::new(); layer_of.len()];
        let mut link_of = vec![Vec::new(); layer_of.len()];
        let mut drawn = vec![Vec::new(); layer_of.len()];
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
        // The subgraphs each slot lies in, outermost first: the innermost one holding it
        // (the first declared among equally deep ones) and those it is nested in.
        let mut innermost: Vec<Option<(usize, usize)>> = vec![None; layer_of.len()];
        for (index, subgraph) in chart.subgraphs.iter().enumerate() {
            let depth = chart.depth(index);
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
                    let mut chain: Vec<usize> = chart.enclosing(subgraph).collect();
                    chain.reverse();
                    chain.push(subgraph);
                    chain
                })
            })
            .collect();
        // A link's passing slots lie in the subgraphs both its ends lie in, as Mermaid puts
        // the dummy nodes of a link between two members of a cluster in that cluster: the
        // frame runs around them, and the slots of other nodes stay outside it. An end at a
        // frame lies in no subgraph, so neither do the passing slots of its link. Upstream
        // assigns an edge's dummy nodes to the clusters on its path by rank
        // (`parentDummyChains`), so the label of a link entering a cluster may fall inside
        // it; here a link to a frame ends at the frame's border, so its label stays in the
        // gap before the frame, outside it.
        for path in &paths {
            let (Some(&first), Some(&last)) = (path.first(), path.last()) else { continue };
            let (from, to) = (chain_of.get(first)?, chain_of.get(last)?);
            let common: Vec<usize> =
                from.iter().zip(to).take_while(|(a, b)| a == b).map(|(&a, _)| a).collect();
            for &slot in path.get(1..path.len().saturating_sub(1)).unwrap_or(&[]) {
                *chain_of.get_mut(slot)? = common.clone();
            }
        }
        let frame_end_slots = frame_end
            .iter()
            .map(|(&slot, &(subgraph, _))| {
                Some(FrameEnd { slot, layer: *layer_of.get(slot)?, subgraph })
            })
            .collect::<Option<Vec<_>>>()?;
        let mut loop_labels: Vec<Vec<Option<&Label>>> = vec![Vec::new(); node_count];
        for edge in chart.edges.iter().filter(|edge| edge.stroke.is_visible()) {
            if let Some((from, to)) = edge.nodes()
                && from == to
            {
                loop_labels.get_mut(from)?.push(edge.label.as_ref());
            }
        }
        // The loops' legs come after the node's other exits whatever their order, so only
        // how many drawn links leave the node matters here, not where their targets lie.
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
        // Only a label on a passing slot (`LabelSpot::Slot`) sizes a slot; a label in a gap
        // gets its room from `route`'s gap sizing.
        let slot_footprints: HashMap<usize, Footprint> = chart
            .edges
            .iter()
            .zip(&paths)
            .filter_map(|(edge, path)| {
                let Some(LabelSpot::Slot(index)) = label_spot(path) else { return None };
                Some((*path.get(index)?, labelled_slot_footprint(axis, edge.label.as_ref()?)))
            })
            .collect();
        let graph = SlotGraph {
            chart,
            axis,
            reversed,
            layer_of,
            node_count,
            member_layers,
            frame_end,
            frame_end_slots,
            frame_ends,
            paths,
            parents,
            children,
            link_of,
            drawn,
            arrival_reach,
            chain_of,
            loop_labels,
            overhang,
            slot_footprints,
        };
        Some((graph, members))
    }

    /// Whether `slot` lies in `subgraph`, directly or in a subgraph nested in it.
    pub(super) fn in_subgraph(&self, slot: usize, subgraph: usize) -> bool {
        in_subgraph(&self.chain_of, slot, subgraph)
    }

    /// The cells `slot` takes across the flow, and the offset from its first cell of the
    /// cell links leave and enter it at: a passing slot is one cell, entered and left at
    /// that cell, or, when it carries a label, the label's cells across the flow plus a
    /// blank cell on either side (see [`labelled_slot_footprint`]).
    pub(super) fn extent(&self, slot: usize) -> (usize, usize) {
        match self.chart.nodes.get(slot) {
            Some(node) => (self.axis.box_cross_size(node), self.axis.port_offset(node)),
            None => self
                .slot_footprints
                .get(&slot)
                .map_or((1, 0), |footprint| (footprint.cross, footprint.port_offset)),
        }
    }

    /// The cells across the flow the labels of `slot`'s self loops reach past its box,
    /// before and after it.
    pub(super) fn overhang(&self, slot: usize) -> (usize, usize) {
        self.overhang.get(slot).copied().unwrap_or((0, 0))
    }

    /// The rank of each slot in its layer's stack (see [`Slot::order`](super::Slot::order)),
    /// the end of a link at a frame ranking just before the frame's first member there.
    pub(super) fn positions(&self, members: &[Vec<usize>]) -> Option<Vec<usize>> {
        stack_ranks(members, &self.frame_end_slots, &self.chain_of, self.layer_of.len())
    }

    /// The cells `frames` take across the flow on one side of their members: each frame's
    /// margin and the cells it grew by (see `grow_frames`).
    pub(super) fn pads(&self, frames: &[usize]) -> Option<isize> {
        let frame_margin = signed(self.axis.frame_margins().1)?;
        frames.iter().try_fold(0, |sum, &subgraph| {
            Some(sum + frame_margin + signed(self.chart.subgraphs.get(subgraph)?.spread)?)
        })
    }

    /// The cells across the flow a frame starting at `frame_start` needs to fit its title
    /// clear of the links crossing its top border at `crossings`: the title runs across the
    /// flow only in a vertical layout.
    pub(super) fn title_cells(
        &self,
        subgraph: usize,
        crossings: &[isize],
        frame_start: isize,
    ) -> Option<isize> {
        match self.axis {
            Axis::Horizontal => Some(0),
            Axis::Vertical => {
                let title_width = self.chart.subgraphs.get(subgraph)?.title.width();
                let from_corner = crossings
                    .iter()
                    .filter_map(|&crossing| usize::try_from(crossing - frame_start).ok())
                    .collect();
                signed(frame_width(title_width, title_offset(from_corner, title_width)))
            }
        }
    }

    /// Whether the link that ends at the frame-end slot `end` is drawn (an invisible link
    /// takes no cell).
    fn end_drawn(&self, end: usize, entering: bool) -> Option<bool> {
        let Self { parents, link_of, drawn, .. } = self;
        if entering {
            let (&parent, &link) = parents.get(end)?.first().zip(link_of.get(end)?.first())?;
            drawn.get(parent)?.get(link).copied()
        } else {
            drawn.get(end)?.first().copied()
        }
    }

    /// Places every slot across the flow. Each slot goes as close as the stack allows to the
    /// mean of the cells its drawn links leave their parents from (a slot no drawn link
    /// reaches goes just before the next slot that has such a place), `sibling_gap` cells
    /// or more after the slot before it, with room for a subgraph's frame wherever the
    /// stack enters or leaves the subgraph's members. A slot's `ceiling` caps only the
    /// place its links ask for, not the room the stack before it takes; its `floor` is a
    /// hard lower bound. The ends of links at frames take no room in the stack and are
    /// placed on the frame's border after the layer's members. `None` as for
    /// [`lay_out`](super::lay_out).
    pub(super) fn place(
        &self,
        members: &[Vec<usize>],
        floor: &HashMap<usize, isize>,
        ceiling: &HashMap<usize, isize>,
        sibling_gap: usize,
    ) -> Option<Placement> {
        let Self { chart, axis, node_count, .. } = *self;
        let Self { layer_of, member_layers, frame_end, frame_ends, children, drawn, .. } = self;
        let Self { parents, link_of, arrival_reach, chain_of, loop_labels, .. } = self;
        let gap = signed(sibling_gap)?;
        let position = self.positions(members)?;
        let mut starts = vec![0; layer_of.len()];
        let mut ports = vec![0; layer_of.len()];
        let mut exits: Vec<Vec<isize>> = vec![Vec::new(); layer_of.len()];
        let mut needs = vec![0; chart.subgraphs.len()];
        let mut entered: Vec<Vec<isize>> = vec![Vec::new(); layer_of.len()];
        for (layer_index, layer) in members.iter().enumerate() {
            // The first cell after the previous slot, or after the frame closing behind it.
            let mut next_free = None;
            let mut open: Vec<OpenFrame> = Vec::new();
            // Where each slot of the layer would start. A parent's links leave it from
            // cells of their own, and a child goes opposite the cells its drawn links
            // leave from, so that children fan out the way their links do. A slot no drawn
            // link reaches goes just before the next slot that has such a place, so that
            // an invisible link fixes only the layer and the order of its target. Upstream's
            // Brandes-Köpf pass (`balance` in dagre's `bk.js`) aligns a node with the
            // median of its neighbours, the targets of invisible links included; in text
            // a box placed for a link that is not drawn only bends the drawn lines, as
            // cells cannot place a box between two columns.
            let mut inbound = Vec::with_capacity(layer.len());
            for &slot in layer {
                let (_, port_offset) = self.extent(slot);
                let (from, reach): (Vec<usize>, Vec<Reach>) = parents
                    .get(slot)?
                    .iter()
                    .zip(link_of.get(slot)?)
                    .zip(arrival_reach.get(slot)?)
                    .filter_map(|((&parent, &link), &reach)| {
                        drawn.get(parent)?.get(link)?.then_some((parent, reach))
                    })
                    .unzip();
                let sources = from
                    .iter()
                    .map(|&parent| ports.get(parent).copied())
                    .collect::<Option<Vec<_>>>()?;
                let cells = chart
                    .nodes
                    .get(slot)
                    .map_or(port_offset..=port_offset, |node| axis.port_range(node));
                let entries = port_cells(axis, &sources, &reach, &cells, port_offset)?;
                let arrivals: Vec<Arrival> = from
                    .into_iter()
                    .zip(entries)
                    .zip(reach)
                    .map(|((from, cell), reach)| Arrival { from, cell, reach })
                    .collect();
                let mut row: Option<(isize, isize)> = None;
                let mut labels = Vec::new();
                for &Arrival { from, cell, reach } in &arrivals {
                    let (before, after) = reach.unwrap_or((0, 0));
                    let (low, high) = (signed(cell)? - signed(before)?, signed(cell + after)?);
                    row = Some(
                        row.map_or((low, high), |(first, last)| (first.min(low), last.max(high))),
                    );
                    if reach.is_some() {
                        labels.push((from, low));
                    }
                }
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
                let wanted = (count > 0).then(|| sum.div_euclid(count) - offset);
                inbound.push(Inbound { wanted, row, labels, arrivals });
            }
            let mut next_placed: Option<(usize, isize)> = None;
            for (Inbound { wanted, .. }, &slot) in inbound.iter_mut().zip(layer).rev() {
                if wanted.is_none()
                    && let Some((next_slot, next_wanted)) = next_placed
                {
                    // The margins of the frames closing after this slot and opening
                    // before the next lie between them too.
                    let (chain, next_chain) = (chain_of.get(slot)?, chain_of.get(next_slot)?);
                    let common = chain.iter().zip(next_chain).take_while(|(a, b)| a == b).count();
                    let frames =
                        self.pads(chain.get(common..)?)? + self.pads(next_chain.get(common..)?)?;
                    let size = signed(self.extent(slot).0 + self.overhang(slot).1)?;
                    *wanted = Some(next_wanted - gap - size - frames);
                }
                next_placed = wanted.map(|wanted| (slot, wanted)).or(next_placed);
            }
            // The last cell the previous slots take on the label rows: a line, a label, or
            // the border of a frame closing behind them that spans the layer before.
            let mut row_end: Option<isize> = None;
            for (&slot, &Inbound { wanted, row, ref labels, ref arrivals }) in
                layer.iter().zip(&inbound)
            {
                let chain = chain_of.get(slot)?;
                // The frames this slot is not in close behind the previous slot, the
                // innermost first, around the labels of the links entering it too.
                let kept = open
                    .iter()
                    .zip(chain)
                    .take_while(|(frame, subgraph)| frame.subgraph == **subgraph)
                    .count();
                if open.len() > kept {
                    next_free = next_free.max(row_end.map(|row_end| row_end + 1));
                }
                while open.len() > kept {
                    let OpenFrame { subgraph: current, start: frame_start, crossings } =
                        open.pop()?;
                    let title_end =
                        frame_start + self.title_cells(current, &crossings, frame_start)?;
                    let pad = self.pads(&[current])?;
                    next_free = next_free.map(|end: isize| (end + pad).max(title_end));
                    // A frame that spans the layer before has its border on the label rows;
                    // one whose members start in this layer has its top border below them,
                    // so it takes nothing there.
                    if member_layers.get(current)?.0 < layer_index {
                        row_end = row_end.max(next_free.map(|end| end - 1));
                    }
                }
                let lead = self.pads(chain.get(kept..)?)?;
                // The box keeps the gap from the box or frame before. The label rows hold
                // no box, so a label reaching past the slot keeps one blank cell from the
                // lines, labels and frame borders before it there. A frame opening here
                // encloses the labels of links from its own members, so such a label keeps
                // the gap from the box before as well, as the frame does. The labels of the
                // slot's self loops keep the gap from it as the box does.
                let (before, after) = self.overhang(slot);
                let (before, after) = (signed(before)?, signed(after)?);
                let opening = match chain.get(kept) {
                    Some(&outermost) => labels
                        .iter()
                        .filter(|&&(parent, _)| self.in_subgraph(parent, outermost))
                        .map(|&(_, first)| (-first).max(0))
                        .max()
                        .unwrap_or(0),
                    None => 0,
                };
                let box_free = next_free.map(|end| end + gap + lead + opening.max(before));
                let row_free =
                    row.zip(row_end).map(|((first, _), row_end)| row_end + 2 + lead - first);
                let free = box_free.max(row_free);
                let (cross_size, port_offset) = self.extent(slot);
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
                // Each frame opening here lies its margin and growth outside the next one in.
                for (depth, &subgraph) in chain.iter().enumerate().skip(kept) {
                    let frame_start = start - self.pads(chain.get(depth..)?)?;
                    open.push(OpenFrame { subgraph, start: frame_start, crossings: Vec::new() });
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
                if axis == Axis::Vertical && slot < node_count {
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
                        arrivals
                            .iter()
                            .map(|&Arrival { from, cell, .. }| Some((from, start + signed(cell)?)))
                            .collect::<Option<_>>()?
                    };
                    for OpenFrame { subgraph, crossings, .. } in &mut open {
                        crossings.extend(
                            ends.iter()
                                .filter(|&&(other, _)| !self.in_subgraph(other, *subgraph))
                                .map(|&(_, cell)| cell),
                        );
                    }
                }
                next_free = Some(start + signed(cross_size)? + after);
                if let Some((_, last)) = row {
                    row_end = row_end.max(Some(start + last));
                }
                *starts.get_mut(slot)? = start;
                *ports.get_mut(slot)? = start + offset;
                *exits.get_mut(slot)? = leaving;
            }
            for (&slot, Inbound { arrivals, .. }) in layer.iter().zip(&inbound) {
                let start = *starts.get(slot)?;
                *entered.get_mut(slot)? = arrivals
                    .iter()
                    .map(|arrival| Some(start + signed(arrival.cell)?))
                    .collect::<Option<_>>()?;
            }
            // The links meeting one border of a frame in this layer take cells of their own
            // across the span of its members here and the cells the frame grew by around
            // it, ordered by their other ends and clear of the links crossing that border
            // into the frame (see `frame_ports`); an invisible link meets no border and
            // stays at the centre.
            let mut borders: Vec<((usize, bool), Vec<usize>)> = Vec::new();
            for &slot in frame_ends.get(layer_index)? {
                let border = *frame_end.get(&slot)?;
                match borders.iter_mut().find(|(other, _)| *other == border) {
                    Some((_, ends)) => ends.push(slot),
                    None => borders.push((border, vec![slot])),
                }
            }
            // The ends at a nested frame are cells the links to them cross the borders of
            // the frames around it at, so the deepest frames take their cells first.
            borders.sort_by_key(|&((subgraph, _), _)| std::cmp::Reverse(chart.depth(subgraph)));
            for ((subgraph, entering), ends) in borders {
                // The cells the frame's content takes here: each member's box with the
                // frames nested in this one around it, their margins and growth included,
                // as a box's ports are spread about the box itself.
                let spans = layer
                    .iter()
                    .filter(|&&member| self.in_subgraph(member, subgraph))
                    .map(|&member| {
                        let chain = chain_of.get(member)?;
                        let depth = chain.iter().position(|&outer| outer == subgraph)?;
                        let nested = self.pads(chain.get(depth + 1..)?)?;
                        let start = *starts.get(member)?;
                        let size = axis.box_cross_size(chart.nodes.get(member)?);
                        Some((start - nested, start + signed(size)? - 1 + nested))
                    })
                    .collect::<Option<Vec<_>>>()?;
                let first = spans.iter().map(|&(low, _)| low).min()?;
                let last = spans.iter().map(|&(_, high)| high).max()?;
                let middle = first + (last - first) / 2;
                let spread = signed(chart.subgraphs.get(subgraph)?.spread)?;
                let (low, high) = (first - spread, last + spread);
                // Where drawn links from outside cross this border: into the frame's
                // members here at their entry cells, out of them at their exit cells, and
                // to or from the frames nested in it at those frames' ends.
                let mut crossings: Vec<Crossing> = Vec::new();
                for &end in frame_ends.get(layer_index)? {
                    let &(inner, inner_entering) = frame_end.get(&end)?;
                    if inner == subgraph
                        || inner_entering != entering
                        || !chart.encloses(subgraph, inner)
                    {
                        continue;
                    }
                    let reach = if entering { *arrival_reach.get(end)?.first()? } else { None };
                    if self.end_drawn(end, entering)? {
                        crossings.push((*starts.get(end)?, reach));
                    }
                }
                for (&slot, Inbound { arrivals, .. }) in layer.iter().zip(&inbound) {
                    if !self.in_subgraph(slot, subgraph) {
                        continue;
                    }
                    let start = *starts.get(slot)?;
                    if entering {
                        for &Arrival { cell, reach, .. } in arrivals {
                            crossings.push((start + signed(cell)?, reach));
                        }
                    } else {
                        let leaving = children.get(slot)?.iter().zip(drawn.get(slot)?);
                        for ((&child, &is_drawn), &cell) in leaving.zip(exits.get(slot)?) {
                            if is_drawn && !self.in_subgraph(child, subgraph) {
                                crossings.push((cell, None));
                            }
                        }
                    }
                }
                let links = ends
                    .iter()
                    .map(|&end| {
                        if entering {
                            let (&parent, &link) =
                                parents.get(end)?.first().zip(link_of.get(end)?.first())?;
                            Some(FrameLink {
                                other: *exits.get(parent)?.get(link)?,
                                drawn: *drawn.get(parent)?.get(link)?,
                                reach: *arrival_reach.get(end)?.first()?,
                            })
                        } else {
                            let &child = children.get(end)?.first()?;
                            Some(FrameLink {
                                other: signed(*position.get(child)?)?,
                                drawn: *drawn.get(end)?.first()?,
                                reach: None,
                            })
                        }
                    })
                    .collect::<Option<Vec<_>>>()?;
                let (others, reach): (Vec<isize>, Vec<Reach>) = links
                    .iter()
                    .filter(|link| link.drawn)
                    .map(|link| (link.other, link.reach))
                    .unzip();
                let ordered = reach_in_order(&others, &reach);
                let need = frame_growth(axis, &ordered, &crossings, (first, last), middle)?;
                let frame_need = needs.get_mut(subgraph)?;
                *frame_need = (*frame_need).max(need);
                let span = usize::try_from(high - low).ok()?;
                let port = usize::try_from(middle - low).ok()?;
                // Before the frame has grown as far as it needs, as in the rounds that
                // measure it, its ends share cells as on a box border too short for them.
                let placed = frame_ports(axis, &ordered, &crossings, (low, high), middle);
                let cells = match placed {
                    Some(ranked) => {
                        let ranked = ranked
                            .iter()
                            .map(|&cell| usize::try_from(cell - low).ok())
                            .collect::<Option<Vec<_>>>()?;
                        scatter_by_order(&others, &ranked)?
                    }
                    None => port_cells(axis, &others, &reach, &(0..=span), port)?,
                };
                let mut cells = cells.into_iter();
                for (&end, link) in ends.iter().zip(&links) {
                    let cell = if link.drawn { cells.next()? } else { port };
                    let at = low + signed(cell)?;
                    *starts.get_mut(end)? = at;
                    *ports.get_mut(end)? = at;
                    *exits.get_mut(end)? = vec![at; children.get(end)?.len()];
                }
            }
        }
        Some(Placement { starts, ports, needs, entered, exits })
    }

    /// The cells where drawn links meet the top border of subgraph `index`'s frame on
    /// screen in `placement`, which its title keeps clear of: in a vertical layout, the
    /// border facing the layer before its members (`TD`) or after them (`BT`), where links
    /// from outside reach its members, the frames nested in it and the frame itself; a
    /// horizontal title runs along the flow, where `title_cells` reserves nothing.
    pub(super) fn top_crossings(&self, index: usize, placement: &Placement) -> Option<Vec<isize>> {
        let Self { chart, axis, .. } = *self;
        let Self { layer_of, member_layers, frame_end, children, drawn, .. } = self;
        let Placement { starts, entered, exits, .. } = placement;
        if axis == Axis::Horizontal {
            return Some(Vec::new());
        }
        let facing = !chart.direction.points_backward();
        let &(first, last) = member_layers.get(index)?;
        let layer = if facing { first } else { last };
        let mut cells = Vec::new();
        for (slot, &slot_layer) in layer_of.iter().enumerate() {
            if slot_layer != layer {
                continue;
            }
            if let Some(&(frame, entering)) = frame_end.get(&slot) {
                let within = frame == index || chart.encloses(index, frame);
                if entering == facing && within && self.end_drawn(slot, entering)? {
                    cells.push(*starts.get(slot)?);
                }
            } else if self.in_subgraph(slot, index) {
                if facing {
                    cells.extend(entered.get(slot)?);
                } else {
                    let leaving = children.get(slot)?.iter().zip(drawn.get(slot)?);
                    for ((&child, &is_drawn), &cell) in leaving.zip(exits.get(slot)?) {
                        if is_drawn && !self.in_subgraph(child, index) {
                            cells.push(cell);
                        }
                    }
                }
            }
        }
        Some(cells)
    }
}
