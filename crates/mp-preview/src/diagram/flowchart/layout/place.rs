//! The slots of a layout and their place across the flow, dagre's `position`: each slot
//! as near the cells its parents' links leave from as its layer's stack and the frames
//! around it allow.
//!
//! The *label rows* of a layer are the cells along the flow just before it, in the gap
//! from the layer before, where the lines entering the layer turn and carry their labels;
//! no box lies on them, so what a slot's entries take there is measured apart from the
//! box.

use std::ops::RangeInclusive;

use super::growth::Growth;
use super::layers::{cycle_closing_edges, layering_constraints, member_pairs, reversed_links};
use super::order::{FrameEnd, in_subgraph, reduce_crossings, stack_ranks};
use super::ports::{
    Exits, exit_cells, frame_growth, frame_ports, port_cells, reach_in_order, scatter_by_order,
    self_loop_label_at,
};
use super::reach::{
    Crossing, Footprint, LabelSpot, Reach, entry_reach, label_cross_reach, label_spot,
    labelled_slot_footprint,
};
use super::simplex::network_simplex_layers;
use super::size::{frame_width, title_offset};
use super::{Axis, LaidOutEnd, MAX_PASSING_SLOTS, laid_out_end, on_run};
use crate::diagram::flowchart::chart::{End, Flowchart};
use crate::diagram::flowchart::label::Label;
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
    /// The cell across the flow of the first leg of each subgraph's drawn self loop,
    /// indexed like [`Flowchart::edges`]; `None` for any other edge.
    pub(super) loop_legs: Vec<Option<isize>>,
    /// The cells where the drawn links entering each slot meet it.
    entered: Vec<Vec<isize>>,
    /// The cell where each link leaving a slot leaves it, indexed like its children.
    exits: Vec<Vec<isize>>,
}

impl Placement {
    /// The placement with each slot of `cells` starting at its cell, which is also where
    /// links leave and enter it. `None` when a slot is missing.
    pub(super) fn with_cells(mut self, cells: &[(usize, isize)]) -> Option<Self> {
        for &(slot, cell) in cells {
            *self.starts.get_mut(slot)? = cell;
            *self.ports.get_mut(slot)? = cell;
        }
        Some(self)
    }

    /// The placement with the slots of one more layer placed as `layer` places them, each
    /// frame needing as many cells as the more of the two give, and the legs `layer` places.
    /// `None` when a slot, subgraph or edge is missing.
    fn with_layer(mut self, layer: LayerPlacement) -> Option<Self> {
        for PlacedSlot { slot, start, port, exits, entered } in layer.slots {
            *self.starts.get_mut(slot)? = start;
            *self.ports.get_mut(slot)? = port;
            *self.exits.get_mut(slot)? = exits;
            *self.entered.get_mut(slot)? = entered;
        }
        for (subgraph, need) in layer.needs {
            let frame_need = self.needs.get_mut(subgraph)?;
            *frame_need = (*frame_need).max(need);
        }
        for (edge, leg) in layer.loop_legs {
            *self.loop_legs.get_mut(edge)? = Some(leg);
        }
        Some(self)
    }
}

/// The places of one layer's slots, as [`SlotGraph::place_layer`] finds them.
struct LayerPlacement {
    /// Each slot of the layer's stack, and each end of a link at or beside a frame in the
    /// layer.
    slots: Vec<PlacedSlot>,
    /// The fewest cells a subgraph's frame must grow by on either side for the links
    /// meeting one of its borders in this layer, with the subgraph.
    needs: Vec<(usize, usize)>,
    /// The cell across the flow of the first leg of a subgraph's drawn self loop leaving
    /// its frame in this layer, with the loop's edge index.
    loop_legs: Vec<(usize, isize)>,
}

/// The place of one slot across the flow (see the fields of the same names in
/// [`Placement`]).
struct PlacedSlot {
    slot: usize,
    start: isize,
    port: isize,
    exits: Vec<isize>,
    entered: Vec<isize>,
}

/// Bounds on where each slot starts across the flow, indexed by slot; `None` where a slot
/// has no such bound (see [`SlotGraph::place`]).
pub(super) struct Bounds {
    /// The first cell each slot may start at.
    pub(super) floor: Vec<Option<isize>>,
    /// The last cell each slot's links may ask it to start at.
    pub(super) ceiling: Vec<Option<isize>>,
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
    reach: Option<Reach>,
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
    reach: Option<Reach>,
}

/// What a layout derives from a chart's nodes, links and subgraphs alone, before it
/// places anything across the flow: the slots with their layers, the links between them
/// and the order of each layer's stack. None of it depends on how far boxes and frames
/// grow, so a chart whose boxes and frames grow is laid out again from the same skeleton
/// (see [`Ranked`](super::Ranked)).
pub(super) struct Skeleton {
    /// See [`Layered::reversed`](super::Layered::reversed).
    pub(super) reversed: Vec<bool>,
    /// See [`Layered::beside`](super::Layered::beside).
    pub(super) beside: Vec<Option<LaidOutEnd>>,
    /// The layer of each slot: the chart's nodes, indexed like [`Flowchart::nodes`],
    /// followed by the other slots (see [`Layered::slots`](super::Layered::slots)).
    pub(super) layer_of: Vec<usize>,
    pub(super) node_count: usize,
    /// The first and last layers of each subgraph's members, where links to and from it
    /// meet its frame.
    pub(super) member_layers: Vec<RangeInclusive<usize>>,
    /// For each slot that is the end of a link at a frame rather than a node or a passing
    /// slot: the subgraph, and whether the link enters it.
    frame_end: Vec<Option<(usize, bool)>>,
    /// For each slot where a link meets a frame across the flow (see
    /// [`Layered::beside`](super::Layered::beside)): the subgraph. Such an end takes no
    /// room in its layer's stack, and is put just outside the frame by
    /// [`SlotGraph::beside_end_cells`] only once the frames are known.
    pub(super) beside_end: Vec<Option<usize>>,
    /// The ends of links at frames, by layer: they take no room in their layer's stack
    /// and are placed on the frame's border once its members in that layer are.
    frame_ends: Vec<Vec<usize>>,
    /// The ends of links meeting a frame across the flow, by layer.
    beside_ends: Vec<Vec<usize>>,
    /// The ends of links at frames and beside them, by layer, each with its subgraph, as
    /// the order of the stacks ranks them (see [`stack_ranks`]).
    ranked_ends: Vec<Vec<FrameEnd>>,
    /// See [`Layered::paths`](super::Layered::paths).
    pub(super) paths: Vec<Vec<usize>>,
    parents: Vec<Vec<usize>>,
    children: Vec<Vec<usize>>,
    /// For each parent of a slot, the index of the link among the parent's children.
    link_of: Vec<Vec<usize>>,
    /// Whether the link to each of a slot's children is drawn, indexed like `children`.
    drawn: Vec<Vec<bool>>,
    /// The reach of the label each link to a slot carries where it enters the slot,
    /// indexed like `parents`.
    arrival_reach: Vec<Vec<Option<Reach>>>,
    /// The subgraphs each slot lies in, outermost first.
    pub(super) chain_of: Vec<Vec<usize>>,
    /// The edge index of each subgraph's drawn self loops, in the order of their edges,
    /// indexed like [`Flowchart::subgraphs`].
    frame_loops: Vec<Vec<usize>>,
    /// The slots of each layer's stack, ordered to reduce the links crossing between
    /// layers (see [`reduce_crossings`]).
    pub(super) stacks: Vec<Vec<usize>>,
    /// Every slot of each layer, in slot order.
    pub(super) layer_slots: Vec<Vec<usize>>,
    /// The slots lying in each subgraph, directly or in a subgraph nested in it (see
    /// `chain_of`), in slot order, indexed like [`Flowchart::subgraphs`].
    pub(super) subgraph_slots: Vec<Vec<usize>>,
    /// How the chart's subgraphs nest.
    pub(super) tree: SubgraphTree,
}

/// How a chart's subgraphs nest, worked out once: every round of
/// [`lay_out`](super::lay_out) asks it for each stray and frame, and walking a subgraph's
/// parents each time (see [`Flowchart::enclosing`]) costs as many steps as the nesting is
/// deep.
pub(super) struct SubgraphTree {
    /// How many subgraphs each subgraph is nested in, indexed like
    /// [`Flowchart::subgraphs`].
    pub(super) depth: Vec<usize>,
    /// The subgraphs nested directly in each subgraph, in index order.
    pub(super) children: Vec<Vec<usize>>,
    /// The subgraphs each subgraph is nested in, outermost first, without itself.
    pub(super) enclosing: Vec<Vec<usize>>,
    /// For each subgraph, the number a depth-first walk of the nesting reaches it at, up
    /// to the last number that walk gives inside it: a subgraph encloses exactly those
    /// whose number lies after its own and within its range.
    tour: Vec<RangeInclusive<usize>>,
}

impl SubgraphTree {
    /// The nesting of `chart`'s subgraphs. `None` when a parent is missing or the
    /// parents form a cycle, which leaves a subgraph unreached from the outermost ones.
    pub(super) fn new(chart: &Flowchart) -> Option<Self> {
        let count = chart.subgraphs.len();
        let mut children = vec![Vec::new(); count];
        let mut outermost = Vec::new();
        for (index, subgraph) in chart.subgraphs.iter().enumerate() {
            match subgraph.parent {
                Some(parent) => children.get_mut(parent)?.push(index),
                None => outermost.push(index),
            }
        }
        let mut depth = vec![0; count];
        let mut enclosing = vec![Vec::new(); count];
        let mut tour = vec![0..=0; count];
        let mut next = 0;
        // A stack of its own rather than recursion: subgraphs may nest deeper than the
        // call stack reaches. A subgraph is pushed again, marked left, under its children.
        let mut pending: Vec<(usize, bool)> =
            outermost.iter().rev().map(|&subgraph| (subgraph, false)).collect();
        while let Some((subgraph, left)) = pending.pop() {
            let range = tour.get_mut(subgraph)?;
            if left {
                *range = *range.start()..=next - 1;
                continue;
            }
            *range = next..=next;
            next += 1;
            pending.push((subgraph, true));
            let mut chain = enclosing.get(subgraph)?.clone();
            chain.push(subgraph);
            let child_depth = depth.get(subgraph)? + 1;
            for &child in children.get(subgraph)?.iter().rev() {
                *depth.get_mut(child)? = child_depth;
                *enclosing.get_mut(child)? = chain.clone();
                pending.push((child, false));
            }
        }
        (next == count).then_some(Self { depth, children, enclosing, tour })
    }

    /// Whether subgraph `inner` is nested, at any depth, in subgraph `outer`, as
    /// [`Flowchart::encloses`] tells.
    pub(super) fn encloses(&self, outer: usize, inner: usize) -> bool {
        match (self.tour.get(outer), self.tour.get(inner)) {
            (Some(outer), Some(inner)) => {
                outer.start() < inner.start() && outer.contains(inner.start())
            }
            _ => false,
        }
    }
}

impl Skeleton {
    /// Assigns every slot to a layer, links the slots and orders each layer's stack.
    /// `None` as for [`Ranked::new`](super::Ranked::new).
    pub(super) fn new(chart: &Flowchart) -> Option<Self> {
        let pairs = member_pairs(chart)?;
        let closing = cycle_closing_edges(chart, &pairs)?;
        let array_index: Vec<Option<u32>> =
            chart.nodes.iter().map(|node| node.array_index).collect();
        let node_layers =
            network_simplex_layers(&array_index, &layering_constraints(chart, &pairs, &closing))?;
        let reversed = reversed_links(&closing);
        let member_layers = layer_spans(chart, &node_layers)?;
        let SlotTable { layer_of, frame_end, beside_end, beside, paths, frame_loops } =
            SlotTable::new(chart, &reversed, node_layers, &member_layers)?;
        let Links { parents, children, link_of, drawn, arrival_reach } =
            Links::new(chart, &paths, layer_of.len())?;
        let tree = SubgraphTree::new(chart)?;
        let chain_of = chains(chart, &tree, &frame_end, &beside_end, &paths)?;
        let layer_slots = slots_by_layer(&layer_of)?;
        let at_frame = |slot: usize| matches!(frame_end.get(slot), Some(Some(_)));
        let at_side = |slot: usize| matches!(beside_end.get(slot), Some(Some(_)));
        let members = filtered(&layer_slots, |slot| !at_frame(slot) && !at_side(slot));
        let frame_ends = filtered(&layer_slots, at_frame);
        let beside_ends = filtered(&layer_slots, at_side);
        let ranked_ends = ranked_ends(&layer_slots, &frame_end, &beside_end)?;
        let node_count = chart.nodes.len();
        let stacks =
            reduce_crossings(&members, &parents, &children, node_count, &ranked_ends, &chain_of)?;
        let subgraph_slots = subgraph_slots(&chain_of, chart.subgraphs.len())?;
        Some(Skeleton {
            reversed,
            beside,
            layer_of,
            node_count,
            member_layers,
            frame_end,
            beside_end,
            frame_ends,
            beside_ends,
            ranked_ends,
            paths,
            parents,
            children,
            link_of,
            drawn,
            arrival_reach,
            chain_of,
            frame_loops,
            stacks,
            layer_slots,
            subgraph_slots,
            tree,
        })
    }
}

/// The first and last layers of each of `chart`'s subgraphs' members, given the layer of
/// each node, `node_layers`. `None` when a subgraph has no members or a member is
/// missing.
fn layer_spans(chart: &Flowchart, node_layers: &[usize]) -> Option<Vec<RangeInclusive<usize>>> {
    chart
        .subgraphs
        .iter()
        .map(|subgraph| {
            let layers = subgraph.members.iter().map(|&member| node_layers.get(member).copied());
            let layers = layers.collect::<Option<Vec<_>>>()?;
            Some(*layers.iter().min()?..=*layers.iter().max()?)
        })
        .collect()
}

/// The slots of a layout and the paths of its links through them: the chart's nodes,
/// then, link by link, a slot for each end of the link at a frame and one for each layer
/// the link passes through.
struct SlotTable {
    /// See [`Skeleton::layer_of`].
    layer_of: Vec<usize>,
    /// See [`Skeleton::frame_end`].
    frame_end: Vec<Option<(usize, bool)>>,
    /// See [`Skeleton::beside_end`].
    beside_end: Vec<Option<usize>>,
    /// See [`Layered::beside`](super::Layered::beside).
    beside: Vec<Option<LaidOutEnd>>,
    /// See [`Layered::paths`](super::Layered::paths).
    paths: Vec<Vec<usize>>,
    /// See [`Skeleton::frame_loops`].
    frame_loops: Vec<Vec<usize>>,
}

impl SlotTable {
    /// The slots of `chart`'s layout, given the layer of each node, `node_layers`, which
    /// links are laid out `reversed`, and the first and last layers of each subgraph's
    /// members, `member_layers`. `None` when a slot is missing, when a link between two
    /// frames cannot meet both, or when the links would pass through more than
    /// [`MAX_PASSING_SLOTS`] slots.
    fn new(
        chart: &Flowchart,
        reversed: &[bool],
        node_layers: Vec<usize>,
        member_layers: &[RangeInclusive<usize>],
    ) -> Option<Self> {
        let node_count = chart.nodes.len();
        let mut table = SlotTable {
            frame_end: vec![None; node_layers.len()],
            beside_end: vec![None; node_layers.len()],
            layer_of: node_layers,
            beside: Vec::with_capacity(chart.edges.len()),
            paths: Vec::with_capacity(chart.edges.len()),
            frame_loops: vec![Vec::new(); chart.subgraphs.len()],
        };
        for (index, (edge, &reversed)) in chart.edges.iter().zip(reversed).enumerate() {
            // A subgraph's self loop takes no slot: its legs get cells on its frame's far
            // border along the flow with the frame's own link ends (see `place`).
            if let Some(subgraph) = edge.frame_loop() {
                if edge.stroke.is_visible() {
                    table.frame_loops.get_mut(subgraph)?.push(index);
                }
                table.beside.push(None);
                table.paths.push(Vec::new());
                continue;
            }
            // The routing ends a link laid out to an empty subgraph's hidden member at the
            // frame's border.
            let (from, to) = if reversed { (edge.to, edge.from) } else { (edge.from, edge.to) };
            let (from, to) = (laid_out_end(chart, from), laid_out_end(chart, to));
            let side = beside_side(from, to, &table.layer_of, member_layers)?;
            table.beside.push(side.map(|(end, _)| end));
            let start = table.end_slot(from, LaidOutEnd::Source, side, member_layers)?;
            let end = table.end_slot(to, LaidOutEnd::Target, side, member_layers)?;
            if start == end {
                table.paths.push(vec![start]);
                continue;
            }
            let (first, last) = (*table.layer_of.get(start)?, *table.layer_of.get(end)?);
            // The layering puts a frame after the links into it and before the links out of
            // it, so the ends of every link other than a self loop (handled above) lie in
            // different layers.
            if first >= last {
                return None;
            }
            let mut path = vec![start];
            for layer in first + 1..last {
                if table.layer_of.len() - node_count >= MAX_PASSING_SLOTS {
                    return None;
                }
                path.push(table.push(layer, None, None));
            }
            path.push(end);
            table.paths.push(path);
        }
        Some(table)
    }

    /// The slot of a link's end `end`, laid out as its end `at`: a node's own slot, or a
    /// new slot at the frame of a subgraph, beside it in the layer `side` gives when
    /// `side` names this end, on its border along the flow otherwise.
    fn end_slot(
        &mut self,
        end: End,
        at: LaidOutEnd,
        side: Option<(LaidOutEnd, usize)>,
        member_layers: &[RangeInclusive<usize>],
    ) -> Option<usize> {
        let End::Subgraph(subgraph) = end else { return end.node() };
        match side {
            Some((side, layer)) if side == at => Some(self.push(layer, None, Some(subgraph))),
            _ => {
                let entering = at == LaidOutEnd::Target;
                let layers = member_layers.get(subgraph)?;
                let layer = if entering { *layers.start() } else { *layers.end() };
                Some(self.push(layer, Some((subgraph, entering)), None))
            }
        }
    }

    /// Adds a slot in `layer`, at a frame's border or beside a frame as given, and
    /// returns its index.
    fn push(
        &mut self,
        layer: usize,
        frame_end: Option<(usize, bool)>,
        beside_end: Option<usize>,
    ) -> usize {
        self.layer_of.push(layer);
        self.frame_end.push(frame_end);
        self.beside_end.push(beside_end);
        self.layer_of.len() - 1
    }
}

/// Which end of a link laid out from `from` to `to` meets a frame across the flow, and
/// the layer of that end, given the layer of each slot so far, `layer_of`, and the first
/// and last layers of each subgraph's members: a link whose pairs disagree has its other
/// end within a frame's layers, and meets that frame in the layer next to the other end.
/// `None` when a slot is missing or when the link joins two frames neither of whose
/// spans holds the other's end, which one line cannot meet both of.
fn beside_side(
    from: End,
    to: End,
    layer_of: &[usize],
    member_layers: &[RangeInclusive<usize>],
) -> Option<Option<(LaidOutEnd, usize)>> {
    Some(match (from, to) {
        (End::Subgraph(source), End::Subgraph(target)) => {
            let (source_first, source_last) = member_layers.get(source)?.clone().into_inner();
            let (target_first, target_last) = member_layers.get(target)?.clone().into_inner();
            if target_first > source_last {
                None
            } else if target_first > source_first {
                // The target's first layer lies within the source's span: the link leaves
                // the source frame across the flow, in the layer before the target.
                Some((LaidOutEnd::Source, target_first - 1))
            } else if target_last > source_last {
                // The source's last layer lies within the target's span: the link enters
                // the target frame across the flow, in the layer after the source.
                Some((LaidOutEnd::Target, source_last + 1))
            } else {
                return None;
            }
        }
        (End::Subgraph(subgraph), End::Node(node)) => {
            let target = *layer_of.get(node)?;
            (target <= *member_layers.get(subgraph)?.end())
                .then(|| Some((LaidOutEnd::Source, target.checked_sub(1)?)))
                .flatten()
        }
        (End::Node(node), End::Subgraph(subgraph)) => {
            let source = *layer_of.get(node)?;
            (source >= *member_layers.get(subgraph)?.start())
                .then_some((LaidOutEnd::Target, source + 1))
        }
        _ => None,
    })
}

/// The links between the slots of a layout, seen from both their ends (see the fields of
/// the same names in [`Skeleton`]).
struct Links {
    parents: Vec<Vec<usize>>,
    children: Vec<Vec<usize>>,
    link_of: Vec<Vec<usize>>,
    drawn: Vec<Vec<bool>>,
    arrival_reach: Vec<Vec<Option<Reach>>>,
}

impl Links {
    /// The links between `slot_count` slots along the `paths` of `chart`'s edges. `None`
    /// when a path names a missing slot.
    fn new(chart: &Flowchart, paths: &[Vec<usize>], slot_count: usize) -> Option<Self> {
        let axis = chart.direction.axis();
        let mut links = Links {
            parents: vec![Vec::new(); slot_count],
            children: vec![Vec::new(); slot_count],
            link_of: vec![Vec::new(); slot_count],
            drawn: vec![Vec::new(); slot_count],
            arrival_reach: vec![Vec::new(); slot_count],
        };
        for (path, edge) in paths.iter().zip(&chart.edges) {
            for (segment, pair) in path.windows(2).enumerate() {
                if let &[parent, child] = pair {
                    let reach = entry_reach(axis, edge, path, segment);
                    let siblings = links.children.get_mut(parent)?;
                    links.link_of.get_mut(child)?.push(siblings.len());
                    siblings.push(child);
                    links.drawn.get_mut(parent)?.push(edge.stroke.is_visible());
                    links.parents.get_mut(child)?.push(parent);
                    links.arrival_reach.get_mut(child)?.push(reach);
                }
            }
        }
        Some(links)
    }
}

/// The subgraphs each slot lies in, outermost first, given which slots are ends at frames,
/// `frame_end`, or beside them, `beside_end`, and the `paths` of the links. A node lies in
/// the innermost subgraph holding it (the first declared among equally deep ones) and
/// those it is nested in; an end at or beside a frame lies in the subgraphs enclosing
/// that frame, not in the frame's own subgraph: the frame's border runs through it.
///
/// A link's passing slots lie in the subgraphs both its ends lie in, as Mermaid puts the
/// dummy nodes of a link between two members of a cluster in that cluster: the frame runs
/// around them, and the slots of other nodes stay outside it. A link between a member of
/// `S` and a subgraph nested in `S` thus passes inside `S`, as upstream's
/// `parentDummyChains` puts the dummy nodes of such an edge in the cluster, while a link
/// from outside `S` to that nested subgraph passes outside it. Upstream assigns an edge's
/// dummy nodes to the clusters on its path by rank, so the label of a link entering a
/// cluster may fall inside it; here a link to a frame ends at the frame's border, so its
/// label stays in the gap before the frame, outside it.
fn chains(
    chart: &Flowchart,
    tree: &SubgraphTree,
    frame_end: &[Option<(usize, bool)>],
    beside_end: &[Option<usize>],
    paths: &[Vec<usize>],
) -> Option<Vec<Vec<usize>>> {
    let mut innermost: Vec<Option<(usize, usize)>> = vec![None; frame_end.len()];
    for (index, subgraph) in chart.subgraphs.iter().enumerate() {
        let depth = *tree.depth.get(index)?;
        for &member in &subgraph.members {
            let held = innermost.get_mut(member)?;
            if held.is_none_or(|(deepest, _)| depth > deepest) {
                *held = Some((depth, index));
            }
        }
    }
    let mut chain_of: Vec<Vec<usize>> = innermost
        .iter()
        .zip(frame_end.iter().zip(beside_end))
        .map(|(held, (frame, beside))| match frame.map(|(subgraph, _)| subgraph).or(*beside) {
            Some(subgraph) => tree.enclosing.get(subgraph).cloned(),
            None => held.map_or_else(
                || Some(Vec::new()),
                |(_, subgraph)| {
                    let mut chain = tree.enclosing.get(subgraph)?.clone();
                    chain.push(subgraph);
                    Some(chain)
                },
            ),
        })
        .collect::<Option<_>>()?;
    // The ends of a path are nodes or ends at frames, never passing slots, so their
    // chains are final here.
    for path in paths {
        let (Some(&first), Some(&last)) = (path.first(), path.last()) else { continue };
        let (from, to) = (chain_of.get(first)?, chain_of.get(last)?);
        let common: Vec<usize> =
            from.iter().zip(to).take_while(|(a, b)| a == b).map(|(&a, _)| a).collect();
        for &slot in path.get(1..path.len().saturating_sub(1)).unwrap_or(&[]) {
            *chain_of.get_mut(slot)? = common.clone();
        }
    }
    Some(chain_of)
}

/// Every slot of each layer, in slot order, given the layer of each slot.
fn slots_by_layer(layer_of: &[usize]) -> Option<Vec<Vec<usize>>> {
    let layer_count = layer_of.iter().max().map_or(0, |last| last + 1);
    let mut layers = vec![Vec::new(); layer_count];
    for (slot, &layer) in layer_of.iter().enumerate() {
        layers.get_mut(layer)?.push(slot);
    }
    Some(layers)
}

/// The slots of each of `layers` that `keep` holds for, in their order.
fn filtered(layers: &[Vec<usize>], keep: impl Fn(usize) -> bool) -> Vec<Vec<usize>> {
    layers.iter().map(|layer| layer.iter().copied().filter(|&slot| keep(slot)).collect()).collect()
}

/// The slots of each of `layers` at a frame's border, `frame_end`, or beside a frame,
/// `beside_end`, each with the frame's subgraph. `None` when a slot is missing.
fn ranked_ends(
    layers: &[Vec<usize>],
    frame_end: &[Option<(usize, bool)>],
    beside_end: &[Option<usize>],
) -> Option<Vec<Vec<FrameEnd>>> {
    layers
        .iter()
        .map(|layer| {
            let ends = layer.iter().map(|&slot| {
                let frame = frame_end.get(slot)?.map(|(subgraph, _)| subgraph);
                Some(frame.or(*beside_end.get(slot)?).map(|subgraph| FrameEnd { slot, subgraph }))
            });
            let ends = ends.collect::<Option<Vec<_>>>()?;
            Some(ends.into_iter().flatten().collect())
        })
        .collect()
}

/// The slots lying in each of `subgraph_count` subgraphs, given the subgraphs each slot
/// lies in, `chain_of`, in slot order.
fn subgraph_slots(chain_of: &[Vec<usize>], subgraph_count: usize) -> Option<Vec<Vec<usize>>> {
    let mut slots = vec![Vec::new(); subgraph_count];
    for (slot, chain) in chain_of.iter().enumerate() {
        for &subgraph in chain {
            slots.get_mut(subgraph)?.push(slot);
        }
    }
    Some(slots)
}

/// The labels of each of `chart`'s nodes' drawn self loops, in the order of their edges.
/// `None` when a link names a missing node.
fn loop_labels(chart: &Flowchart) -> Option<Vec<Vec<Option<&Label>>>> {
    let mut labels: Vec<Vec<Option<&Label>>> = vec![Vec::new(); chart.nodes.len()];
    for edge in chart.edges.iter().filter(|edge| edge.stroke.is_visible()) {
        if let Some((from, to)) = edge.nodes()
            && from == to
        {
            labels.get_mut(from)?.push(edge.label.as_ref());
        }
    }
    Some(labels)
}

/// The labels on the runs of the drawn links leaving each slot of `skeleton`, which lie
/// across the flow about the cell the link leaves by (see `label_cross_reach`). `None`
/// when a path is empty or names a missing slot.
fn run_labels<'a>(chart: &'a Flowchart, skeleton: &Skeleton) -> Option<Vec<Vec<&'a Label>>> {
    let Skeleton { layer_of, paths, beside, .. } = skeleton;
    let mut labels: Vec<Vec<&Label>> = vec![Vec::new(); layer_of.len()];
    for (index, (edge, path)) in chart.edges.iter().zip(paths).enumerate() {
        if let Some(label) = &edge.label
            && edge.stroke.is_visible()
            && on_run(beside, index)
        {
            labels.get_mut(*path.first()?)?.push(label);
        }
    }
    Some(labels)
}

/// The cells across the flow the labels of each of `chart`'s nodes' self loops,
/// `loop_labels`, and those on the runs of the links leaving it, `run_labels`, reach past
/// its box, grown as `growth` says, before and after it. `None` when a slot is missing.
fn overhangs(
    chart: &Flowchart,
    skeleton: &Skeleton,
    growth: &Growth,
    loop_labels: &[Vec<Option<&Label>>],
    run_labels: &[Vec<&Label>],
) -> Option<Vec<Reach>> {
    let axis = chart.direction.axis();
    chart
        .nodes
        .iter()
        .zip(&growth.boxes)
        .zip(loop_labels.iter().zip(run_labels))
        .zip(&skeleton.drawn)
        .map(|(((node, &spread), (labels, runs)), drawn)| {
            if labels.iter().all(Option::is_none) && runs.is_empty() {
                return Some(Reach::default());
            }
            // The loops' legs come after the node's other exits whatever their order, so
            // only how many drawn links leave the node matters here, not where their
            // targets lie.
            let targets = drawn.iter().filter(|&&drawn| drawn).count();
            let (cells, offset) = (axis.port_range(node, spread), axis.port_offset(node, spread));
            let Exits { targets: cells, legs } = exit_cells(axis, targets, labels, &cells, offset)?;
            let size = signed(axis.box_cross_size(node, spread))?;
            let (mut before, mut after) = (0, 0);
            for (&leg, label) in legs.iter().zip(labels) {
                let Some(label) = label else { continue };
                let (_, first) = self_loop_label_at(axis, leg, label)?;
                let end = first + signed(axis.label_size(label).1)?;
                before = before.max(usize::try_from(-first).unwrap_or(0));
                after = after.max(usize::try_from(end - size).unwrap_or(0));
            }
            // Which of the cells a run leaves by depends on where its end beside the frame
            // ranks among the node's targets, which the layers' order settles only later,
            // so the label reaches as far as it would from the outermost cell on either
            // side.
            let (Some(&first), Some(&last)) = (cells.iter().min(), cells.iter().max()) else {
                return Some(Reach { before, after });
            };
            for &label in runs {
                let reach = label_cross_reach(axis, label);
                let end = signed(last + reach.after + 1)?;
                before = before.max(reach.before.saturating_sub(first));
                after = after.max(usize::try_from(end - size).unwrap_or(0));
            }
            Some(Reach { before, after })
        })
        .collect()
}

/// The cells across the flow the labels on the runs of the links leaving each of
/// `chart`'s subgraphs' frames, `run_labels` by slot, reach before and after the cell the
/// link leaves by. `None` when a subgraph is missing.
fn run_reaches(
    chart: &Flowchart,
    skeleton: &Skeleton,
    run_labels: &[Vec<&Label>],
) -> Option<Vec<Reach>> {
    let axis = chart.direction.axis();
    let mut reaches = vec![Reach::default(); chart.subgraphs.len()];
    for (labels, frame_end) in run_labels.iter().zip(&skeleton.frame_end) {
        let &Some((subgraph, _)) = frame_end else { continue };
        let reach = reaches.get_mut(subgraph)?;
        for &label in labels {
            let label_reach = label_cross_reach(axis, label);
            reach.before = reach.before.max(label_reach.before);
            reach.after = reach.after.max(label_reach.after);
        }
    }
    Some(reaches)
}

/// The footprint of each passing slot of `skeleton` that carries a label, indexed by
/// slot. Only a label on a passing slot (`LabelSpot::Slot`) sizes a slot; a label in a
/// gap gets its room from `route`'s gap sizing. `None` when a slot is missing.
fn slot_footprints(chart: &Flowchart, skeleton: &Skeleton) -> Option<Vec<Option<Footprint>>> {
    let axis = chart.direction.axis();
    let mut footprints = vec![None; skeleton.layer_of.len()];
    for (edge, path) in chart.edges.iter().zip(&skeleton.paths) {
        let (Some(LabelSpot::Slot(index)), Some(label)) = (label_spot(path), &edge.label) else {
            continue;
        };
        *footprints.get_mut(*path.get(index)?)? = Some(labelled_slot_footprint(axis, label));
    }
    Some(footprints)
}

/// A chart's [`Skeleton`] with the cells across the flow the labels around its slots
/// reach, which depend on how far the boxes grow: what [`lay_out`](super::lay_out)
/// derives from the chart before it places the slots across the flow.
pub(super) struct SlotGraph<'a> {
    chart: &'a Flowchart,
    axis: Axis,
    skeleton: &'a Skeleton,
    growth: &'a Growth,
    /// The labels of each node's drawn self loops, in the order of their edges.
    loop_labels: Vec<Vec<Option<&'a Label>>>,
    /// The cells across the flow the labels of each node's self loops, and those on the
    /// runs of the links leaving it (see [`on_run`]), reach past its box, before and after
    /// it.
    overhang: Vec<Reach>,
    /// The cells across the flow the labels on the runs of the links leaving each
    /// subgraph's frame (see [`on_run`]) reach before and after the cell the link leaves
    /// by, indexed like [`Flowchart::subgraphs`].
    pub(super) run_reach: Vec<Reach>,
    /// The footprint of each passing slot that carries a label, indexed by slot; `None`
    /// for any other slot (see [`labelled_slot_footprint`]).
    pub(super) slot_footprints: Vec<Option<Footprint>>,
}

impl<'a> SlotGraph<'a> {
    /// Measures the labels around the slots of `skeleton`, which must have been made from
    /// `chart`'s nodes, links and subgraphs, with its boxes and frames grown as `growth`
    /// says. `None` when a slot is missing.
    pub(super) fn new(
        chart: &'a Flowchart,
        skeleton: &'a Skeleton,
        growth: &'a Growth,
    ) -> Option<Self> {
        let axis = chart.direction.axis();
        let loop_labels = loop_labels(chart)?;
        let run_labels = run_labels(chart, skeleton)?;
        let overhang = overhangs(chart, skeleton, growth, &loop_labels, &run_labels)?;
        let run_reach = run_reaches(chart, skeleton, &run_labels)?;
        let slot_footprints = slot_footprints(chart, skeleton)?;
        Some(SlotGraph {
            chart,
            axis,
            skeleton,
            growth,
            loop_labels,
            overhang,
            run_reach,
            slot_footprints,
        })
    }

    /// [`order::in_subgraph`](in_subgraph) over the subgraphs this layout's slots lie in.
    pub(super) fn in_subgraph(&self, slot: usize, subgraph: usize) -> bool {
        in_subgraph(&self.skeleton.chain_of, slot, subgraph)
    }

    /// The cells `slot` takes across the flow, and the offset from its first cell of the
    /// cell links leave and enter it at: a passing slot is one cell, entered and left at
    /// that cell, or, when it carries a label, the label's cells across the flow plus a
    /// blank cell on either side (see [`labelled_slot_footprint`]).
    pub(super) fn extent(&self, slot: usize) -> (usize, usize) {
        match self.chart.nodes.get(slot).zip(self.growth.boxes.get(slot)) {
            Some((node, &spread)) => {
                (self.axis.box_cross_size(node, spread), self.axis.port_offset(node, spread))
            }
            None => self
                .slot_footprints
                .get(slot)
                .copied()
                .flatten()
                .map_or((1, 0), |footprint| (footprint.cross, footprint.port_offset)),
        }
    }

    /// The cells across the flow the labels of `slot`'s self loops reach past its box,
    /// before and after it.
    pub(super) fn overhang(&self, slot: usize) -> Reach {
        self.overhang.get(slot).copied().unwrap_or_default()
    }

    /// The rank of each slot in its layer's stack (see [`Slot::order`](super::Slot::order)),
    /// the end of a link at a frame ranking just before the frame's first member there.
    pub(super) fn positions(&self, members: &[Vec<usize>]) -> Option<Vec<usize>> {
        let Skeleton { ranked_ends, chain_of, layer_of, .. } = self.skeleton;
        stack_ranks(members, ranked_ends, chain_of, layer_of.len())
    }

    /// The cells `frames` take across the flow on one side of their members: each frame's
    /// margin and the cells it grew by (see `grow_frames`).
    pub(super) fn pads(&self, frames: &[usize]) -> Option<isize> {
        let frame_margin = signed(self.axis.frame_margins().1)?;
        frames.iter().try_fold(0, |sum, &subgraph| {
            Some(sum + frame_margin + signed(*self.growth.frames.get(subgraph)?)?)
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
        let Skeleton { parents, link_of, drawn, .. } = self.skeleton;
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
    /// stack enters or leaves the subgraph's members. A slot's ceiling in `bounds` caps
    /// only the place its links ask for, not the room the stack before it takes; its floor
    /// is a hard lower bound. The ends of links at frames take no room in the stack and are
    /// placed on the frame's border after the layer's members.
    pub(super) fn place(
        &self,
        members: &[Vec<usize>],
        bounds: &Bounds,
        sibling_gap: usize,
    ) -> Option<Placement> {
        let slot_count = self.skeleton.layer_of.len();
        let gap = signed(sibling_gap)?;
        let position = self.positions(members)?;
        let unplaced = Placement {
            starts: vec![0; slot_count],
            ports: vec![0; slot_count],
            needs: vec![0; self.chart.subgraphs.len()],
            loop_legs: vec![None; self.chart.edges.len()],
            entered: vec![Vec::new(); slot_count],
            exits: vec![Vec::new(); slot_count],
        };
        members.iter().enumerate().try_fold(unplaced, |placement, (index, layer)| {
            let placed = self.place_layer(index, layer, &placement, bounds, gap, &position)?;
            placement.with_layer(placed)
        })
    }

    /// Places the slots of layer `index`, whose stack is `layer`, given the layers before
    /// it in `placement`, as [`place`](Self::place) does, keeping `gap` cells between
    /// neighbouring boxes; `position` holds each slot's rank in its layer's stack.
    fn place_layer(
        &self,
        index: usize,
        layer: &[usize],
        placement: &Placement,
        bounds: &Bounds,
        gap: isize,
        position: &[usize],
    ) -> Option<LayerPlacement> {
        let inbound =
            layer.iter().map(|&slot| self.inbound(slot, placement)).collect::<Option<Vec<_>>>()?;
        let (members, next_free) =
            self.place_members(index, layer, &inbound, bounds, gap, position)?;
        // The ends of links meeting a frame across the flow lie after the layer's slots
        // until `beside_end_cells` puts them by their frames, which needs the frames'
        // bands; the next layer places the targets of their links from these cells.
        let at = next_free.unwrap_or(0);
        let beside = self
            .skeleton
            .beside_ends
            .get(index)?
            .iter()
            .map(|&end| {
                let exits = vec![at; self.skeleton.children.get(end)?.len()];
                Some(PlacedSlot { slot: end, start: at, port: at, exits, entered: Vec::new() })
            })
            .collect::<Option<Vec<_>>>()?;
        let borders = self.place_borders(index, &inbound, &members, placement, position)?;
        let slots = members.into_iter().chain(beside).chain(borders.slots).collect();
        Some(LayerPlacement { slots, ..borders })
    }

    /// The drawn links entering `slot`, whose parents lie in the layers `placement` has
    /// placed, and where they would put the slot. A parent's links leave it from cells of
    /// their own, and a child goes opposite the cells its drawn links leave from, so that
    /// children fan out the way their links do.
    fn inbound(&self, slot: usize, placement: &Placement) -> Option<Inbound> {
        let Self { chart, axis, skeleton, growth, .. } = *self;
        let Skeleton { parents, link_of, arrival_reach, drawn, .. } = skeleton;
        let (_, port_offset) = self.extent(slot);
        let (from, reach): (Vec<usize>, Vec<Option<Reach>>) = parents
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
            .map(|&parent| placement.ports.get(parent).copied())
            .collect::<Option<Vec<_>>>()?;
        let cells = chart
            .nodes
            .get(slot)
            .zip(growth.boxes.get(slot))
            .map_or(port_offset..=port_offset, |(node, &spread)| axis.port_range(node, spread));
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
            let Reach { before, after } = reach.unwrap_or_default();
            let (low, high) = (signed(cell)? - signed(before)?, signed(cell + after)?);
            row = Some(row.map_or((low, high), |(first, last)| (first.min(low), last.max(high))));
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
                Some((sum + placement.exits.get(parent)?.get(link)?, count + 1))
            },
        )?;
        let wanted = (count > 0).then(|| sum.div_euclid(count) - offset);
        Some(Inbound { wanted, row, labels, arrivals })
    }

    /// Where each slot of `layer` would start, given the drawn links entering each,
    /// `inbound`, and `gap` cells between neighbouring boxes. A slot no drawn link reaches
    /// goes just before the next slot that has such a place, so that an invisible link
    /// fixes only the layer and the order of its target. Upstream's Brandes-Köpf pass
    /// (`verticalAlignment` in dagre's `bk.js`) aligns a node with the median of its
    /// neighbours, the targets of invisible links included; in text a box placed for a
    /// link that is not drawn only bends the drawn lines, as cells cannot place a box
    /// between two columns.
    fn wanted_starts(
        &self,
        layer: &[usize],
        inbound: &[Inbound],
        gap: isize,
    ) -> Option<Vec<Option<isize>>> {
        let chain_of = &self.skeleton.chain_of;
        let mut wanted = vec![None; layer.len()];
        let mut next_placed: Option<(usize, isize)> = None;
        for ((place, inbound), &slot) in wanted.iter_mut().zip(inbound).zip(layer).rev() {
            *place = match (inbound.wanted, next_placed) {
                (None, Some((next_slot, next_wanted))) => {
                    // The margins of the frames closing after this slot and opening
                    // before the next lie between them too.
                    let (chain, next_chain) = (chain_of.get(slot)?, chain_of.get(next_slot)?);
                    let common = chain.iter().zip(next_chain).take_while(|(a, b)| a == b).count();
                    let frames =
                        self.pads(chain.get(common..)?)? + self.pads(next_chain.get(common..)?)?;
                    let size = signed(self.extent(slot).0 + self.overhang(slot).after)?;
                    Some(next_wanted - gap - size - frames)
                }
                (own, _) => own,
            };
            next_placed = place.map(|wanted| (slot, wanted)).or(next_placed);
        }
        Some(wanted)
    }

    /// Places the slots of the stack `layer` of layer `index`, given the drawn links
    /// entering each, `inbound`, as [`place`](Self::place) does, and returns them with the
    /// first cell after the last of them.
    fn place_members(
        &self,
        index: usize,
        layer: &[usize],
        inbound: &[Inbound],
        bounds: &Bounds,
        gap: isize,
        position: &[usize],
    ) -> Option<(Vec<PlacedSlot>, Option<isize>)> {
        let Self { chart, axis, skeleton, .. } = *self;
        let Skeleton { member_layers, children, drawn, chain_of, node_count, .. } = skeleton;
        let wanted = self.wanted_starts(layer, inbound, gap)?;
        let mut placed = Vec::with_capacity(layer.len());
        // The first cell after the previous slot, or after the frame closing behind it.
        let mut next_free = None;
        let mut open: Vec<OpenFrame> = Vec::new();
        // The last cell the previous slots take on the label rows: a line, a label, or
        // the border of a frame closing behind them that spans the layer before.
        let mut row_end: Option<isize> = None;
        for ((&slot, &wanted), Inbound { row, labels, arrivals, .. }) in
            layer.iter().zip(&wanted).zip(inbound)
        {
            let row = *row;
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
                let OpenFrame { subgraph: current, start: frame_start, crossings } = open.pop()?;
                let title_end = frame_start + self.title_cells(current, &crossings, frame_start)?;
                let pad = self.pads(&[current])?;
                // The labels on the runs of links leaving the frame lie in the gap after
                // its last layer, reaching past the cell they leave by, which lies no
                // further out than the cells the frame grew by; counting their reach from
                // the border instead leaves the frame's margin as slack.
                let run_after = if *member_layers.get(current)?.end() == index {
                    signed(self.run_reach.get(current).map_or(0, |reach| reach.after))?
                } else {
                    0
                };
                next_free = next_free.map(|end: isize| (end + pad + run_after).max(title_end));
                // A frame that spans the layer before has its border on the label rows;
                // one whose members start in this layer has its top border below them, so
                // it takes nothing there.
                if *member_layers.get(current)?.start() < index {
                    row_end = row_end.max(next_free.map(|end| end - 1));
                }
            }
            let lead = self.pads(chain.get(kept..)?)?;
            // The box keeps the gap from the box or frame before. The label rows hold no
            // box, so a label reaching past the slot keeps one blank cell from the lines,
            // labels and frame borders before it there. A frame opening here encloses the
            // labels of links from its own members, so such a label keeps the gap from the
            // box before as well, as the frame does. The labels of the slot's self loops
            // keep the gap from it as the box does.
            let Reach { before, after } = self.overhang(slot);
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
            let row_free = row.zip(row_end).map(|((first, _), row_end)| row_end + 2 + lead - first);
            let free = box_free.max(row_free);
            let (cross_size, port_offset) = self.extent(slot);
            let offset = signed(port_offset)?;
            let ceiling = *bounds.ceiling.get(slot)?;
            let wanted = wanted.map(|wanted| ceiling.map_or(wanted, |ceiling| wanted.min(ceiling)));
            let start = match (wanted, free) {
                (Some(wanted), Some(free)) => wanted.max(free),
                (Some(at), None) | (None, Some(at)) => at,
                (None, None) => 0,
            };
            let start = bounds.floor.get(slot)?.map_or(start, |floor| start.max(floor));
            // Each frame opening here lies its margin and growth outside the next one in.
            for (depth, &subgraph) in chain.iter().enumerate().skip(kept) {
                let frame_start = start - self.pads(chain.get(depth..)?)?;
                open.push(OpenFrame { subgraph, start: frame_start, crossings: Vec::new() });
            }
            // The cells the routing gives the links leaving this slot: a box spreads them
            // over its far border in the order of their targets in the next layer, its self
            // loops last, as `exit_ports` in `route.rs` does; an invisible link takes no
            // cell and counts as leaving from the port; a passing slot uses its port.
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
                        self.loop_labels.get(slot)?,
                        &axis.port_range(node, *self.growth.boxes.get(slot)?),
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
            // The entry cells the routing gives links arriving at this slot.
            let entered = arrivals
                .iter()
                .map(|arrival| Some(start + signed(arrival.cell)?))
                .collect::<Option<Vec<_>>>()?;
            if axis == Axis::Vertical && slot < *node_count {
                // The cells where links to or from the slot's other ends meet its border
                // facing the top borders of its frames; an invisible link meets no border.
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
                    arrivals
                        .iter()
                        .zip(&entered)
                        .map(|(arrival, &cell)| (arrival.from, cell))
                        .collect()
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
            placed.push(PlacedSlot { slot, start, port: start + offset, exits: leaving, entered });
        }
        Some((placed, next_free))
    }

    /// Places the ends of links at frames in layer `index` on the frames' borders, and the
    /// legs of the frames' self loops leaving there, given the drawn links entering each
    /// slot of the layer's stack, `inbound`, the places of those slots, `members`, and the
    /// layers before in `placement`; `position` holds each slot's rank in its layer's
    /// stack. The links meeting one border of a frame take cells of their own across the
    /// span of its members here and the cells the frame grew by around it, ordered by their
    /// other ends and clear of the links crossing that border into the frame (see
    /// `frame_ports`), followed on the border leaving a frame by the legs of its self
    /// loops, as on a box; an invisible link meets no border and stays at the centre.
    fn place_borders(
        &self,
        index: usize,
        inbound: &[Inbound],
        members: &[PlacedSlot],
        placement: &Placement,
        position: &[usize],
    ) -> Option<LayerPlacement> {
        let Self { chart, axis, skeleton, .. } = *self;
        let Skeleton { member_layers, frame_end, frame_ends, frame_loops, .. } = skeleton;
        let Skeleton { children, drawn, parents, link_of, arrival_reach, chain_of, .. } = skeleton;
        let layer_ends = frame_ends.get(index)?;
        // Each border with the ends at it, as indices into `layer_ends`.
        let mut borders: Vec<((usize, bool), Vec<usize>)> = Vec::new();
        for (at, &slot) in layer_ends.iter().enumerate() {
            let border = (*frame_end.get(slot)?)?;
            match borders.iter_mut().find(|(other, _)| *other == border) {
                Some((_, ends)) => ends.push(at),
                None => borders.push((border, vec![at])),
            }
        }
        for (subgraph, loops) in frame_loops.iter().enumerate() {
            let border = (subgraph, false);
            if !loops.is_empty()
                && *member_layers.get(subgraph)?.end() == index
                && !borders.iter().any(|(other, _)| *other == border)
            {
                borders.push((border, Vec::new()));
            }
        }
        // The ends at a nested frame are cells the links to them cross the borders of the
        // frames around it at, so the deepest frames take their cells first.
        borders.sort_by_key(|&((subgraph, _), _)| {
            std::cmp::Reverse(skeleton.tree.depth.get(subgraph).copied())
        });
        let mut end_cells: Vec<Option<isize>> = vec![None; layer_ends.len()];
        let mut needs = Vec::with_capacity(borders.len());
        let mut loop_legs = Vec::new();
        for ((subgraph, entering), ends) in borders {
            // The cells the frame's content takes here: each member's box with the frames
            // nested in this one around it, their margins and growth included, as a box's
            // ports are spread about the box itself.
            let spans = members
                .iter()
                .filter(|member| self.in_subgraph(member.slot, subgraph))
                .map(|&PlacedSlot { slot: member, start, .. }| {
                    let chain = chain_of.get(member)?;
                    let depth = chain.iter().position(|&outer| outer == subgraph)?;
                    let nested = self.pads(chain.get(depth + 1..)?)?;
                    let size = axis
                        .box_cross_size(chart.nodes.get(member)?, *self.growth.boxes.get(member)?);
                    Some((start - nested, start + signed(size)? - 1 + nested))
                })
                .collect::<Option<Vec<_>>>()?;
            let first = spans.iter().map(|&(low, _)| low).min()?;
            let last = spans.iter().map(|&(_, high)| high).max()?;
            let middle = first + (last - first) / 2;
            let spread = signed(*self.growth.frames.get(subgraph)?)?;
            let (low, high) = (first - spread, last + spread);
            // Where drawn links from outside cross this border: into the frame's members
            // here at their entry cells, out of them at their exit cells, and to or from
            // the frames nested in it at those frames' ends, which the deeper frames
            // placed first.
            let mut crossings: Vec<Crossing> = Vec::new();
            for (&end, &cell) in layer_ends.iter().zip(&end_cells) {
                let (inner, inner_entering) = (*frame_end.get(end)?)?;
                if inner == subgraph
                    || inner_entering != entering
                    || !skeleton.tree.encloses(subgraph, inner)
                {
                    continue;
                }
                let reach = if entering { *arrival_reach.get(end)?.first()? } else { None };
                if self.end_drawn(end, entering)? {
                    crossings.push((cell?, reach));
                }
            }
            for (member, Inbound { arrivals, .. }) in members.iter().zip(inbound) {
                if !self.in_subgraph(member.slot, subgraph) {
                    continue;
                }
                if entering {
                    for (&Arrival { reach, .. }, &cell) in arrivals.iter().zip(&member.entered) {
                        crossings.push((cell, reach));
                    }
                } else {
                    let leaving = children.get(member.slot)?.iter().zip(drawn.get(member.slot)?);
                    for ((&child, &is_drawn), &cell) in leaving.zip(&member.exits) {
                        if is_drawn && !self.in_subgraph(child, subgraph) {
                            crossings.push((cell, None));
                        }
                    }
                }
            }
            let links = ends
                .iter()
                .map(|&at| {
                    let &end = layer_ends.get(at)?;
                    if entering {
                        let (&parent, &link) =
                            parents.get(end)?.first().zip(link_of.get(end)?.first())?;
                        Some(FrameLink {
                            other: *placement.exits.get(parent)?.get(link)?,
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
            let (others, reach): (Vec<isize>, Vec<Option<Reach>>) =
                links.iter().filter(|link| link.drawn).map(|link| (link.other, link.reach)).unzip();
            let ordered = reach_in_order(&others, &reach);
            let loops = if entering { &[][..] } else { frame_loops.get(subgraph)?.as_slice() };
            let loop_labels = loops
                .iter()
                .map(|&edge| Some(chart.edges.get(edge)?.label.as_ref()))
                .collect::<Option<Vec<_>>>()?;
            let need =
                frame_growth(axis, &ordered, &loop_labels, &crossings, (first, last), middle)?;
            needs.push((subgraph, need));
            let span = usize::try_from(high - low).ok()?;
            let port = usize::try_from(middle - low).ok()?;
            // Before the frame has grown as far as it needs, as in the rounds that measure
            // it, its ends share cells as on a box border too short for them and its loops'
            // legs take the port; only a frame grown that far is routed.
            let placed = frame_ports(axis, &ordered, &loop_labels, &crossings, (low, high), middle);
            let (cells, legs) = match placed {
                Some(ranked) => {
                    let ranked = ranked
                        .iter()
                        .map(|&cell| usize::try_from(cell - low).ok())
                        .collect::<Option<Vec<_>>>()?;
                    let (links, legs) = ranked.split_at_checked(others.len())?;
                    (scatter_by_order(&others, links)?, legs.iter().step_by(2).copied().collect())
                }
                None => {
                    (port_cells(axis, &others, &reach, &(0..=span), port)?, vec![port; loops.len()])
                }
            };
            for (&edge, &leg) in loops.iter().zip(&legs) {
                loop_legs.push((edge, low + signed(leg)?));
            }
            let mut cells = cells.into_iter();
            for (&at, link) in ends.iter().zip(&links) {
                let cell = if link.drawn { cells.next()? } else { port };
                *end_cells.get_mut(at)? = Some(low + signed(cell)?);
            }
        }
        let slots = layer_ends
            .iter()
            .zip(end_cells)
            .filter_map(|(&end, cell)| cell.map(|cell| (end, cell)))
            .map(|(end, at)| {
                let exits = vec![at; children.get(end)?.len()];
                Some(PlacedSlot { slot: end, start: at, port: at, exits, entered: Vec::new() })
            })
            .collect::<Option<Vec<_>>>()?;
        Some(LayerPlacement { slots, needs, loop_legs })
    }

    /// The cell of the end of each link meeting a frame across the flow, with that end's
    /// slot: just outside the frame on the side of the link's other end in `placement`,
    /// given each frame's first and last cells across the flow, its borders included, in
    /// `bands`.
    pub(super) fn beside_end_cells(
        &self,
        placement: &Placement,
        bands: &[Option<RangeInclusive<isize>>],
    ) -> Option<Vec<(usize, isize)>> {
        self.skeleton
            .beside_end
            .iter()
            .enumerate()
            .filter_map(|(end, subgraph)| subgraph.map(|subgraph| (end, subgraph)))
            .map(|(end, subgraph)| {
                let band = bands.get(subgraph)?.as_ref()?;
                let (children, parents) =
                    (self.skeleton.children.get(end)?, self.skeleton.parents.get(end)?);
                let &other = children.first().or(parents.first())?;
                let at = if placement.starts.get(other)? > band.end() {
                    band.end() + 1
                } else {
                    band.start() - 1
                };
                Some((end, at))
            })
            .collect()
    }

    /// The cells where drawn links meet the top border of subgraph `index`'s frame on
    /// screen in `placement`, which its title keeps clear of: in a vertical layout, the
    /// border facing the layer before its members (`TD`) or after them (`BT`), where links
    /// from outside reach its members, the frames nested in it and the frame itself; a
    /// horizontal title runs along the flow, where `title_cells` reserves nothing.
    pub(super) fn top_crossings(&self, index: usize, placement: &Placement) -> Option<Vec<isize>> {
        let Self { chart, axis, skeleton, .. } = *self;
        let Skeleton { layer_slots, member_layers, frame_end, children, drawn, .. } = skeleton;
        let Placement { starts, entered, exits, .. } = placement;
        if axis == Axis::Horizontal {
            return Some(Vec::new());
        }
        let facing = !chart.direction.points_backward();
        let layers = member_layers.get(index)?;
        let layer = if facing { *layers.start() } else { *layers.end() };
        let mut cells = Vec::new();
        for &slot in layer_slots.get(layer)? {
            if let Some(&Some((frame, entering))) = frame_end.get(slot) {
                let within = frame == index || skeleton.tree.encloses(index, frame);
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
