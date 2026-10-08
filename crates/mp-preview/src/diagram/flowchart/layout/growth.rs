//! The cells boxes and subgraph frames grow by across the flow so that the links meeting
//! their borders take cells of their own.

use super::layers::cycle_closing_edges;
use super::ports::{end_gaps, exit_gaps, symmetric_offsets};
use super::reach::{Reach, entry_reach};
use super::size::{TITLE_CORNER_OFFSET, box_height, box_width};
use super::{Axis, Layered, laid_out_end};
use crate::diagram::flowchart::label::Label;
use crate::diagram::flowchart::outline::outline;
use crate::diagram::flowchart::parse::{Body, End, Flowchart, Node, Shape};

/// Grows each box across the flow by whole cells on both sides of its label until the
/// link ends on each of its two borders along the flow fit its port cells at
/// [`Axis::entry_spacing`], spread as [`symmetric_offsets`] spreads them. A link closing
/// a cycle is counted on the borders it is laid out between, and a self loop takes two
/// cells [`SELF_LOOP_SPAN`](super::SELF_LOOP_SPAN) apart on the border its box's other
/// links leave by. Upstream sizes a node by its label (widened to
/// `flowchart.minNodeWidth`) and padding, never by its edges, and lets edges meet its
/// border at any point (`intersect`); a three-row box has one non-corner cell on each
/// side, and two links sharing a cell would show a single line and marker, so the box
/// grows instead. Returns each box's growth; `None` when a link joins a subgraph to
/// itself or to one of its own members (see [`cycle_closing_edges`]).
pub(in crate::diagram::flowchart) fn grow_boxes(chart: &Flowchart) -> Option<Growth> {
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
    let farthest = |offsets: Vec<isize>| offsets.iter().map(|offset| offset.unsigned_abs()).max();
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
            Some(farthest(entries).max(farthest(exits)))
        })
        .collect::<Option<Vec<_>>>()?;
    let spreads = chart
        .nodes
        .iter()
        .zip(spreads)
        .map(|(node, farthest_port)| spread_for(node, axis, farthest_port));
    Some(Growth::Boxes(spreads.collect()))
}

/// The cells `node` grows by on both sides of its label across the flow so that its
/// port cells reach `farthest_port` cells either side of its centre. `None` means no link
/// ends at the box: it needs no port cells, so it does not grow.
fn spread_for(node: &Node, axis: Axis, farthest_port: Option<usize>) -> usize {
    let Some(farthest_port) = farthest_port else { return 0 };
    // The port cells run from the first cell of `port_range` to its last, `inset` cells
    // in from either side, around the middle of the box's `natural` cells across the flow.
    let around_middle = |natural: usize, inset: usize| {
        let middle = natural / 2;
        (farthest_port + inset)
            .saturating_sub(middle)
            .max((farthest_port + middle + 1 + inset).saturating_sub(natural))
    };
    match (&node.body, axis) {
        // The middle label row is a port row, and every row the box grew by around it
        // is one more on either side; a hidden node's every cell is a port.
        (Body::Box { .. }, Axis::Horizontal) | (Body::Hidden, _) => farthest_port,
        (Body::Box { shape, .. }, Axis::Vertical) => {
            around_middle(box_width(node), vertical_port_inset(*shape))
        }
        // A drawing's frame takes the corner cells, and its top border the title too.
        (Body::Drawing { .. }, Axis::Horizontal) => around_middle(box_height(node), 1),
        (Body::Drawing { title_width, .. }, Axis::Vertical) => {
            around_middle(box_width(node), drawing_port_inset(*title_width))
        }
    }
}

/// Columns from either side of a drawing's box to its first port cell in a vertical
/// layout, given the display width of its frame's title. The title starts
/// [`TITLE_CORNER_OFFSET`] columns right of the corner and takes `title_width + 2`
/// columns, a blank on either side of it; a link ending on the column right after that
/// would read as `title ┼`, so one `─` is left between them. This is the room
/// [`title_offset`](super::title_offset) keeps between a frame's title and the links
/// crossing its top border.
pub(super) fn drawing_port_inset(title_width: usize) -> usize {
    TITLE_CORNER_OFFSET + title_width + 3
}

/// Columns from either side of a box of `shape` to its first port cell in a vertical
/// layout: one past the corner, or past the blanks before a border inset further.
pub(super) fn vertical_port_inset(shape: Shape) -> usize {
    outline(shape).border_inset().max(1)
}

/// Grows each box further where the labels of the links entering it need their entries
/// further apart than [`grow_boxes`] allowed, given the order the links arrive in under
/// `layered`, as [`port_cells`](super::port_cells) spreads them. The order of the slots
/// in each layer does not depend on the boxes' sizes, so the chart can be laid out again
/// with the grown boxes. Returns each box's growth, never less than it already grows by;
/// `None` when a link's slot is missing from `layered`.
pub(in crate::diagram::flowchart) fn grow_for_labels(
    chart: &Flowchart,
    layered: &Layered,
) -> Option<Growth> {
    let axis = chart.direction.axis();
    let slots = &layered.slots;
    let mut arrivals: Vec<Vec<LabelledEntry>> = chart.nodes.iter().map(|_| Vec::new()).collect();
    for (edge_index, (edge, path)) in chart.edges.iter().zip(&layered.paths).enumerate() {
        if !edge.stroke.is_visible() {
            continue;
        }
        for (segment, pair) in path.windows(2).enumerate() {
            if let &[from, to] = pair
                && let Some(arriving) = arrivals.get_mut(to)
            {
                let reach = entry_reach(axis, edge, path, segment);
                arriving.push(LabelledEntry {
                    from_port: slots.get(from)?.port,
                    edge_index,
                    reach,
                });
            }
        }
    }
    let mut spreads = Vec::with_capacity(chart.nodes.len());
    for (node, mut arriving) in chart.nodes.iter().zip(arrivals) {
        if arriving.iter().all(|entry| entry.reach.is_none()) {
            spreads.push(node.spread);
            continue;
        }
        arriving.sort_by_key(|entry| (entry.from_port, entry.edge_index));
        let ordered: Vec<Reach> = arriving.iter().map(|entry| entry.reach).collect();
        let offsets = symmetric_offsets(ordered.len(), &end_gaps(axis, &ordered))?;
        let farthest_port = offsets.iter().map(|offset| offset.unsigned_abs()).max().unwrap_or(0);
        spreads.push(node.spread.max(spread_for(node, axis, Some(farthest_port))));
    }
    Some(Growth::Boxes(spreads))
}

/// A drawn link entering a box, as [`grow_for_labels`] sizes the box for it.
struct LabelledEntry {
    /// The port cell across the flow the link leaves its source from.
    from_port: usize,
    /// The link's index in [`Flowchart::edges`].
    edge_index: usize,
    /// The reach of the link's label at the entry, if it has one.
    reach: Reach,
}

/// The cells each subgraph frame with members grows by across the flow on both sides
/// of its members, as [`grow_boxes`] and [`grow_for_labels`] grow a box: just
/// enough, as `layered` measured it ([`Layered::frame_needs`]), for the drawn links
/// meeting each of its two borders along the flow to take cells of their own there,
/// clear of the links crossing that border into the frame (see `frame_ports`). A frame no
/// drawn link meets grows by nothing. The frame grows on both sides so that its members
/// stay centred in it. Upstream sizes a cluster by its content and its title (`rect` in
/// `clusters.js`), never by its edges, and cuts each edge where it meets the border, at
/// any point (`cutPathAtIntersect`), while an edge to a node inside runs on through the
/// border; a text border has only whole cells, and two links sharing one would read as
/// one line with a single marker, so the frame grows instead.
///
/// The cells an outer frame's members take, and the cells where links cross its borders
/// to the frames nested in it, depend on how far those frames grew, which `layered` was
/// laid out with, so the caller lays the chart out again and calls this until no frame
/// changes. `None` when every frame already grows by what `layered` measured.
pub(in crate::diagram::flowchart) fn grow_frames(
    chart: &Flowchart,
    layered: &Layered,
) -> Option<Growth> {
    let spreads = chart.subgraphs.iter().map(|frame| frame.spread);
    let changed = !spreads.eq(layered.frame_needs.iter().copied());
    changed.then(|| Growth::Frames(layered.frame_needs.clone()))
}

/// The cells boxes or frames grow by across the flow, which
/// [`Flowchart::with_growth`] applies.
#[derive(Debug)]
pub(in crate::diagram::flowchart) enum Growth {
    /// [`Node::spread`] of each node, indexed like [`Flowchart::nodes`].
    Boxes(Vec<usize>),
    /// [`Subgraph::spread`](crate::diagram::flowchart::parse::Subgraph::spread) of each
    /// subgraph, indexed like [`Flowchart::subgraphs`].
    Frames(Vec<usize>),
}

impl Flowchart {
    /// The chart with its boxes or frames grown as `growth` says; one missing from
    /// `growth` keeps its growth.
    pub(in crate::diagram::flowchart) fn with_growth(mut self, growth: Growth) -> Self {
        match growth {
            Growth::Boxes(spreads) => {
                for (node, spread) in self.nodes.iter_mut().zip(spreads) {
                    node.spread = spread;
                }
            }
            Growth::Frames(spreads) => {
                for (frame, spread) in self.subgraphs.iter_mut().zip(spreads) {
                    frame.spread = spread;
                }
            }
        }
        self
    }
}
