//! Assigning flowchart nodes to layers along the flow and to positions across it.

use std::collections::HashMap;
use std::ops::RangeInclusive;

use unicode_width::UnicodeWidthStr;

use super::outline::outline;
use super::parse::{Direction, Edge, End, Flowchart, Node};
use super::signed;

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
    /// that links leave and enter: the label row, or the centre column.
    pub(super) fn port_offset(self, node: &Node) -> usize {
        match self {
            Self::Horizontal => outline(node.shape).label_row() + node.spread,
            Self::Vertical => self.box_cross_size(node) / 2,
        }
    }

    /// The cells across the flow, counted from a box's first cell, where links may meet
    /// its borders along the flow: in a horizontal layout the label row and the rows
    /// the box grew by around it, which leaves out a cylinder's arc row; in a vertical
    /// one every column between the corners.
    pub(super) fn port_range(self, node: &Node) -> RangeInclusive<usize> {
        match self {
            Self::Horizontal => {
                let first = outline(node.shape).label_row();
                first..=first + 2 * node.spread
            }
            Self::Vertical => 1..=self.box_cross_size(node).saturating_sub(2),
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

/// The label plus the cells its shape takes around it.
pub(super) fn box_width(node: &Node) -> usize {
    node.label.width() + outline(node.shape).padding()
}

/// The rows of a node's box.
fn box_height(node: &Node) -> usize {
    outline(node.shape).height()
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
/// keeping the order of `others`. [`grow_boxes`] makes every box border wide enough;
/// should the cells still not hold them, the links share them out from `port`
/// outwards, several to a cell.
pub(super) fn port_cells<P: Ord>(
    axis: Axis,
    others: &[P],
    cells: &RangeInclusive<usize>,
    port: usize,
) -> Option<Vec<usize>> {
    let mut order: Vec<usize> = (0..others.len()).collect();
    order.sort_by_key(|&end| others.get(end));
    let spacing = axis.entry_spacing();
    let ports = symmetric_ports(port, cells, spacing, others.len())
        .unwrap_or_else(|| spread_ports(port, cells, spacing, others.len()));
    let mut cells_of = vec![0; others.len()];
    for (rank, &end) in order.iter().enumerate() {
        *cells_of.get_mut(end)? = *ports.get(rank * ports.len() / others.len())?;
    }
    Some(cells_of)
}

/// The offsets from a border's centre of `count` link ends `spacing` apart, in
/// ascending order: the centre and pairs either side of it for an odd count; for an
/// even count pairs either side that skip the centre, the inner pair half a spacing
/// out, rounded up to a whole cell.
fn symmetric_offsets(count: usize, spacing: usize) -> Vec<isize> {
    let spacing = spacing as isize;
    let count = count as isize;
    if count % 2 == 1 {
        return (0..count).map(|index| (index - (count - 1) / 2) * spacing).collect();
    }
    let inner = (spacing + 1) / 2;
    let mut offsets: Vec<isize> = (0..count / 2)
        .flat_map(|pair| [-(inner + pair * spacing), inner + pair * spacing])
        .collect();
    offsets.sort_unstable();
    offsets
}

/// The cells [`symmetric_offsets`] gives `count` link ends about `port`, or `None`
/// when one of them lies outside `cells`.
fn symmetric_ports(
    port: usize,
    cells: &RangeInclusive<usize>,
    spacing: usize,
    count: usize,
) -> Option<Vec<usize>> {
    symmetric_offsets(count, spacing)
        .into_iter()
        .map(|offset| port.checked_add_signed(offset).filter(|cell| cells.contains(cell)))
        .collect()
}

/// Grows each box across the flow by whole cells on both sides of its label until the
/// link ends on each of its two borders along the flow fit its port cells at
/// [`Axis::entry_spacing`], spread as [`symmetric_offsets`] spreads them. A link closing
/// a cycle is counted on the borders it is laid out between, and a self loop leaves
/// through the border its box's other links leave by. `None` when the chart cannot be
/// searched for cycles.
pub(super) fn grow_boxes(chart: &mut Flowchart) -> Option<()> {
    let axis = chart.direction.axis();
    let reversed = cycle_closing_edges(chart)?;
    let mut entering = vec![0; chart.nodes.len()];
    let mut leaving = vec![0; chart.nodes.len()];
    for (edge, &reversed) in chart.edges.iter().zip(&reversed) {
        let (from, to) = if reversed { (edge.to, edge.from) } else { (edge.from, edge.to) };
        // The end at a frame meets no box.
        if let End::Node(from) = from {
            *leaving.get_mut(from)? += 1;
        }
        if let End::Node(to) = to
            && from != End::Node(to)
        {
            *entering.get_mut(to)? += 1;
        }
    }
    for ((node, entering), leaving) in chart.nodes.iter_mut().zip(entering).zip(leaving) {
        let reach = symmetric_offsets(entering.max(leaving), axis.entry_spacing())
            .last()
            .map_or(0, |&offset| offset.unsigned_abs());
        node.spread = match axis {
            // The label row is a port row, and every row the box grows by on either
            // side of it is one more.
            Axis::Horizontal => reach,
            // The port cells run from one column inside the left corner to one inside
            // the right corner, around the centre column.
            Axis::Vertical => {
                let width = box_width(node);
                let centre = width / 2;
                (reach + 1).saturating_sub(centre).max((reach + centre + 2).saturating_sub(width))
            }
        };
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
        let mut slot_at = |end: End, entering: bool| -> Option<usize> {
            match end {
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
    for path in &paths {
        for pair in path.windows(2) {
            if let &[parent, child] = pair {
                let siblings = children.get_mut(parent)?;
                link_of.get_mut(child)?.push(siblings.len());
                siblings.push(child);
                parents.get_mut(child)?.push(parent);
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
    let mut loops = vec![0_usize; node_count];
    for (from, _) in chart.edges.iter().filter_map(Edge::nodes).filter(|(from, to)| from == to) {
        *loops.get_mut(from)? += 1;
    }
    let mut position = vec![0; layer_of.len()];
    for layer in &members {
        for (index, &slot) in layer.iter().enumerate() {
            *position.get_mut(slot)? = index;
        }
    }
    // An end at a frame lies among its subgraph's members in its layer.
    for (&slot, &(subgraph, _)) in &frame_end {
        let layer = members.get(*layer_of.get(slot)?)?;
        let first_member =
            layer.iter().position(|&member| subgraph_of.get(member) == Some(&Some(subgraph)));
        *position.get_mut(slot)? = first_member.unwrap_or(0);
    }
    // A passing slot is one cell across the flow, entered and left at that cell. A self
    // loop runs through the cells after its box.
    let extent = |slot: usize| match chart.nodes.get(slot) {
        Some(node) => {
            let loop_cells = if loops.get(slot) > Some(&0) { SELF_LOOP_CELLS } else { 0 };
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
    let mut exits: Vec<Vec<isize>> = vec![Vec::new(); layer_of.len()];
    for (layer_index, layer) in members.iter().enumerate() {
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
            // A parent's links leave it from cells of their own, and a child goes
            // opposite the cells its links leave from, so that children fan out the way
            // their links do.
            let (sum, count) = parents.get(slot)?.iter().zip(link_of.get(slot)?).try_fold(
                (0_isize, 0_isize),
                |(sum, count), (&parent, &link)| {
                    Some((sum + exits.get(parent)?.get(link)?, count + 1))
                },
            )?;
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
            // The cells the routing gives the links leaving this slot: a box spreads
            // them over its far border in the order of their targets in the next layer,
            // its self loops last, as `exit_ports` in `route.rs` does; a passing slot uses
            // its port.
            let leaving = match chart.nodes.get(slot) {
                Some(node) => {
                    let targets = children
                        .get(slot)?
                        .iter()
                        .map(|&child| Some((false, *position.get(child)?)))
                        .collect::<Option<Vec<_>>>()?;
                    let others: Vec<(bool, usize)> = targets
                        .into_iter()
                        .chain(std::iter::repeat_n((true, 0), *loops.get(slot)?))
                        .collect();
                    port_cells(axis, &others, &axis.port_range(node), port_offset)?
                        .into_iter()
                        .map(|cell| Some(start + signed(cell)?))
                        .collect::<Option<Vec<_>>>()?
                }
                _ => vec![start + offset; children.get(slot)?.len()],
            };
            if axis == Axis::Vertical
                && let Some(subgraph) = subgraph
                && let Some(node) = chart.nodes.get(slot)
            {
                if chart.direction.points_backward() {
                    for (&child, &cell) in children.get(slot)?.iter().zip(&leaving) {
                        if subgraph_of.get(child).copied().flatten() != Some(subgraph) {
                            crossings.push(cell);
                        }
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
                    let entries = port_cells(axis, &sources, &axis.port_range(node), port_offset)?;
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
                .filter(|&&member| subgraph_of.get(member) == Some(&Some(subgraph)))
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
            for (&end, cell) in ends.iter().zip(port_cells(axis, &others, &(0..=span), span / 2)?) {
                let at = low + signed(cell)?;
                *starts.get_mut(end)? = at;
                *ports.get_mut(end)? = at;
                *exits.get_mut(end)? = vec![at; children.get(end)?.len()];
            }
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
    // Only links between nodes can close a cycle that reversing one of them breaks;
    // a link to or from a frame keeps its direction.
    for (index, edge) in chart.edges.iter().enumerate() {
        if let Some((from, _)) = edge.nodes() {
            outgoing.get_mut(from)?.push(index);
        }
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
            let (_, to) = chart.edges.get(index)?.nodes()?;
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

/// The pairs of nodes `(from, to, length)` whose layers the links impose: each link
/// between nodes in layout order, `reversed` closing a cycle; a link into a subgraph
/// puts every member after the source, a link out of one puts the target after every
/// member, and a link between two puts every member of the second after every member
/// of the first. `None` when a link joins a subgraph to itself or to one of its own
/// members, which has no layout here.
fn layering_constraints(
    chart: &Flowchart,
    reversed: &[bool],
) -> Option<Vec<(usize, usize, usize)>> {
    let nodes_at = |end: End| -> Option<Vec<usize>> {
        match end {
            End::Node(node) => Some(vec![node]),
            End::Subgraph(subgraph) => Some(chart.subgraphs.get(subgraph)?.members.clone()),
        }
    };
    let mut constraints = Vec::new();
    for (edge, &reversed) in chart.edges.iter().zip(reversed) {
        if let Some((from, to)) = edge.nodes() {
            let (from, to) = if reversed { (to, from) } else { (from, to) };
            constraints.push((from, to, edge.length));
            continue;
        }
        for from in nodes_at(edge.from)? {
            for to in nodes_at(edge.to)? {
                if from == to {
                    return None;
                }
                constraints.push((from, to, edge.length));
            }
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
