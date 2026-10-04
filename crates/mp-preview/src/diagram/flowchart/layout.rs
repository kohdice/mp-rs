//! Assigning flowchart nodes to layers along the flow and to positions across it.

use std::collections::HashMap;
use std::ops::RangeInclusive;

use unicode_width::UnicodeWidthStr;

use super::parse::{Direction, Flowchart, Node};
use super::signed;

/// Rows of every box: a border, the label, a border.
pub(super) const BOX_HEIGHT: usize = 3;

/// Cells a self loop runs through beyond its box, both along the flow and across it;
/// the layout keeps the ones across the flow free.
pub(super) const SELF_LOOP_CELLS: usize = 2;

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
            Self::Vertical => BOX_HEIGHT,
        }
    }

    /// The cells a box spans across the flow.
    pub(super) fn box_cross_size(self, node: &Node) -> usize {
        match self {
            Self::Horizontal => BOX_HEIGHT,
            Self::Vertical => box_width(node),
        }
    }

    /// The offset across the flow from a box's first cell to the middle of the borders
    /// that links leave and enter: the label row, or the centre column.
    fn port_offset(self, node: &Node) -> usize {
        match self {
            Self::Horizontal => BOX_HEIGHT / 2,
            Self::Vertical => box_width(node) / 2,
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
                // `────►`, with room above for a three-cell label such as `yes` and a
                // blank on either side before the gap has to widen.
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

/// The label plus one space of padding and a border on each side.
pub(super) fn box_width(node: &Node) -> usize {
    node.label.width() + 4
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

/// The cell where each link arriving at a slot `width` cells wide across the flow
/// enters it, counted from the slot's first cell and listed in the order of `sources`,
/// the cells across the flow the links come from: the links share out cells
/// [`Axis::entry_spacing`] apart around the slot's `port`, in the order of their
/// sources, ties keeping the order of `sources`.
pub(super) fn entry_cells<P: Ord>(
    axis: Axis,
    sources: &[P],
    width: usize,
    port: usize,
) -> Option<Vec<usize>> {
    let mut order: Vec<usize> = (0..sources.len()).collect();
    order.sort_by_key(|&arrival| sources.get(arrival));
    let cells = 0..=width.checked_sub(1)?;
    let ports = spread_ports(port, &cells, axis.entry_spacing(), sources.len());
    let mut entries = vec![0; sources.len()];
    for (rank, &arrival) in order.iter().enumerate() {
        *entries.get_mut(arrival)? = *ports.get(rank * ports.len() / sources.len())?;
    }
    Some(entries)
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

/// A place in a layer: a node's box, or a cell a link longer than one layer passes
/// through.
#[derive(Debug)]
pub(super) struct Slot {
    pub layer: usize,
    /// The first cell across the flow.
    pub cross: usize,
    /// The cell across the flow where links leave and enter.
    pub port: usize,
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
    let ends = chart
        .edges
        .iter()
        .zip(&reversed)
        .map(|(edge, &reversed)| if reversed { (edge.to, edge.from) } else { (edge.from, edge.to) })
        .collect::<Vec<_>>();
    let mut layer_of = longest_path_layers(chart, &ends)?;
    let node_count = chart.nodes.len();
    let mut paths = Vec::with_capacity(chart.edges.len());
    for &(start, end) in &ends {
        if start == end {
            paths.push(vec![start]);
            continue;
        }
        let (from, to) = (*layer_of.get(start)?, *layer_of.get(end)?);
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
    for (slot, &layer) in layer_of.iter().enumerate() {
        members.get_mut(layer)?.push(slot);
    }
    let mut parents = vec![Vec::new(); layer_of.len()];
    let mut children = vec![Vec::new(); layer_of.len()];
    for path in &paths {
        for pair in path.windows(2) {
            if let &[parent, child] = pair {
                parents.get_mut(child)?.push(parent);
                children.get_mut(parent)?.push(child);
            }
        }
    }
    // Each layer keeps declaration order unless reordering it reduces crossings with the
    // layer before.
    reduce_crossings(&mut members, &parents)?;
    let mut subgraph_of = vec![None; layer_of.len()];
    for (index, subgraph) in chart.subgraphs.iter().enumerate() {
        for &member in &subgraph.members {
            subgraph_of.get_mut(member)?.get_or_insert(index);
        }
    }
    for layer in &mut members {
        group_subgraph_members(layer, &subgraph_of)?;
    }
    let mut looped = vec![false; node_count];
    for edge in chart.edges.iter().filter(|edge| edge.from == edge.to) {
        *looped.get_mut(edge.from)? = true;
    }
    // A passing slot is one cell across the flow, entered and left at that cell. A self
    // loop runs through the cells after its box.
    let extent = |slot: usize| match chart.nodes.get(slot) {
        Some(node) => {
            let loop_cells = if looped.get(slot) == Some(&true) { SELF_LOOP_CELLS } else { 0 };
            (axis.box_cross_size(node) + loop_cells, axis.port_offset(node))
        }
        None => (1, 0),
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
    // frame wherever the stack enters or leaves the subgraph's members.
    let mut starts = vec![0; layer_of.len()];
    let mut ports = vec![0; layer_of.len()];
    for layer in &members {
        // The first cell after the previous slot, or after the frame closing behind it.
        let mut end = None;
        // The subgraph of the previous slot, and the first cell of its frame.
        let mut open: Option<(usize, isize)> = None;
        // The cells where links cross the open frame's top border, estimated in each
        // layer from its boxes: where links from outside enter them, or, when the top
        // border faces the next layer, where links to outside leave them.
        let mut crossings = Vec::new();
        for &slot in layer {
            let subgraph = *subgraph_of.get(slot)?;
            if let Some((current, frame_start)) = open
                && subgraph != Some(current)
            {
                let title_end = frame_start + title_cells(current, &crossings, frame_start)?;
                end = end.map(|end: isize| (end + frame_margin).max(title_end));
                open = None;
            }
            let lead = if open.is_none() && subgraph.is_some() { frame_margin } else { 0 };
            let free = end.map(|end| end + gap + lead);
            let (cross_size, port_offset) = extent(slot);
            let offset = signed(port_offset)?;
            let (sum, count) = parents
                .get(slot)?
                .iter()
                .try_fold((0_isize, 0_isize), |(sum, count), &parent| {
                    Some((sum + ports.get(parent)?, count + 1))
                })?;
            let wanted = (count > 0).then(|| sum.div_euclid(count) - offset);
            let start = match (wanted, free) {
                (Some(wanted), Some(free)) => wanted.max(free),
                (Some(at), None) | (None, Some(at)) => at,
                (None, None) => 0,
            };
            if open.is_none()
                && let Some(subgraph) = subgraph
            {
                open = Some((subgraph, start - frame_margin));
                crossings.clear();
            }
            if axis == Axis::Vertical
                && let Some(subgraph) = subgraph
                && let Some(node) = chart.nodes.get(slot)
            {
                if chart.direction.points_backward() {
                    // Links leave a box at its port.
                    let leaves_the_frame = children
                        .get(slot)?
                        .iter()
                        .any(|&child| subgraph_of.get(child).copied().flatten() != Some(subgraph));
                    if leaves_the_frame {
                        crossings.push(start + offset);
                    }
                } else {
                    // The entry cells the routing gives links arriving at this box.
                    let arrivals = parents
                        .get(slot)?
                        .iter()
                        .map(|&parent| {
                            Some((*ports.get(parent)?, *subgraph_of.get(parent)? != Some(subgraph)))
                        })
                        .collect::<Option<Vec<_>>>()?;
                    let sources: Vec<isize> = arrivals.iter().map(|&(port, _)| port).collect();
                    let entries =
                        entry_cells(axis, &sources, axis.box_cross_size(node), port_offset)?;
                    for (&(_, from_outside), entry) in arrivals.iter().zip(entries) {
                        if from_outside {
                            crossings.push(start + signed(entry)?);
                        }
                    }
                }
            }
            end = Some(start + signed(cross_size)?);
            *starts.get_mut(slot)? = start;
            *ports.get_mut(slot)? = start + offset;
        }
    }

    let origin = starts.iter().copied().min().unwrap_or(0);
    let slots = layer_of
        .iter()
        .zip(starts.iter().zip(&ports))
        .map(|(&layer, (&start, &port))| {
            Some(Slot {
                layer,
                cross: usize::try_from(start - origin).ok()?,
                port: usize::try_from(port - origin).ok()?,
            })
        })
        .collect::<Option<_>>()?;
    Some(Layered { slots, layer_count, paths, reversed })
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
/// other slot lies between them, keeping the order of the slots otherwise.
fn group_subgraph_members(layer: &mut [usize], subgraph_of: &[Option<usize>]) -> Option<()> {
    let mut first_at = HashMap::new();
    let mut keyed = layer
        .iter()
        .enumerate()
        .map(|(index, &slot)| {
            let key = match *subgraph_of.get(slot)? {
                Some(subgraph) => *first_at.entry(subgraph).or_insert(index),
                None => index,
            };
            Some((key, slot))
        })
        .collect::<Option<Vec<_>>>()?;
    keyed.sort_by_key(|&(key, _)| key);
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

/// Marks the edges that lead back to a node still being explored in a depth-first
/// search from each node in declaration order; reversing them leaves no cycle.
fn cycle_closing_edges(chart: &Flowchart) -> Option<Vec<bool>> {
    #[derive(Clone, Copy, PartialEq, Eq)]
    enum Visit {
        New,
        Open,
        Done,
    }
    let mut outgoing = vec![Vec::new(); chart.nodes.len()];
    for (index, edge) in chart.edges.iter().enumerate() {
        outgoing.get_mut(edge.from)?.push(index);
    }
    let mut visits = vec![Visit::New; chart.nodes.len()];
    let mut closing = vec![false; chart.edges.len()];
    // An explicit stack of (node, next outgoing edge to follow): recursion could
    // overflow the call stack on a long chain.
    let mut stack = Vec::new();
    for root in 0..chart.nodes.len() {
        if visits.get(root) != Some(&Visit::New) {
            continue;
        }
        *visits.get_mut(root)? = Visit::Open;
        stack.push((root, 0));
        while let Some(&(node, next)) = stack.last() {
            let Some(&index) = outgoing.get(node)?.get(next) else {
                *visits.get_mut(node)? = Visit::Done;
                stack.pop();
                continue;
            };
            if let Some(top) = stack.last_mut() {
                top.1 += 1;
            }
            let to = chart.edges.get(index)?.to;
            match *visits.get(to)? {
                Visit::New => {
                    *visits.get_mut(to)? = Visit::Open;
                    stack.push((to, 0));
                }
                Visit::Open => *closing.get_mut(index)? = true,
                Visit::Done => {}
            }
        }
    }
    Some(closing)
}

/// The layer of each node, given each edge's `ends` in layout order: at least the
/// edge's length after the layer of its first end. `None` when the edges form a cycle.
fn longest_path_layers(chart: &Flowchart, ends: &[(usize, usize)]) -> Option<Vec<usize>> {
    let count = chart.nodes.len();
    let mut unplaced_parents = vec![0_usize; count];
    let mut children = vec![Vec::new(); count];
    for (edge, &(from, to)) in chart.edges.iter().zip(ends).filter(|(_, (from, to))| from != to) {
        children.get_mut(from)?.push((to, edge.length));
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
