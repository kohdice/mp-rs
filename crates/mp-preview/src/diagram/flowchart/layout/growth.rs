//! The cells boxes and subgraph frames grow by across the flow so that the links meeting
//! their borders take cells of their own.

use super::ports::{end_gaps, exit_gaps, symmetric_offsets};
use super::reach::{Reach, entry_reach};
use super::size::{TITLE_CORNER_OFFSET, box_height, box_width};
use super::{Axis, Layered, Ranked, laid_out_end};
use crate::diagram::flowchart::chart::{Body, End, Flowchart, Node, Shape};
use crate::diagram::flowchart::label::Label;
use crate::diagram::flowchart::outline::outline;

/// Grows each box across the flow by whole cells on both sides of its label until the
/// link ends on each of its two borders along the flow fit its port cells at
/// [`Axis::entry_spacing`], spread as [`symmetric_offsets`] spreads them. A link closing
/// a cycle is counted on the borders it is laid out between, and a self loop takes two
/// cells [`SELF_LOOP_SPAN`](super::SELF_LOOP_SPAN) apart on the border its box's other
/// links leave by. Upstream sizes a node by its label (widened to
/// `flowchart.minNodeWidth`) and padding, never by its edges, and lets edges meet its
/// border at any point (`intersect`); a three-row box has one non-corner cell on each
/// side, and two links sharing a cell would show a single line and marker, so the box
/// grows instead. Returns the growth of `ranked`'s chart, its frames not grown; `None`
/// when a link names a node outside the chart.
pub(in crate::diagram::flowchart) fn grow_boxes(ranked: &Ranked) -> Option<Growth> {
    let Ranked { chart, skeleton, .. } = ranked;
    let axis = chart.direction.axis();
    let reversed = &skeleton.reversed;
    let mut entering = vec![0; chart.nodes.len()];
    let mut leaving = vec![0; chart.nodes.len()];
    let mut loops: Vec<Vec<Option<&Label>>> = vec![Vec::new(); chart.nodes.len()];
    for (edge, &reversed) in chart.edges.iter().zip(reversed) {
        // An invisible link meets no border, and a subgraph's self loop meets its frame's.
        if !edge.stroke.is_visible() || edge.frame_loop().is_some() {
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
    let boxes = chart
        .nodes
        .iter()
        .zip(spreads)
        .map(|(node, farthest_port)| spread_for(node, axis, farthest_port))
        .collect();
    Some(Growth { boxes, frames: vec![0; chart.subgraphs.len()] })
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
/// `layered`, as [`port_cells`](super::port_cells) spreads them. Slots keep their order
/// within each layer when the boxes grow, except where [`lay_out`](super::lay_out) moves
/// a stray out of a frame it does not belong to, so the growth is measured again until it
/// settles. Returns `growth` with each box grown by what it needs, never less than it
/// already grows by; `None` when a link's slot is missing from `layered` or a box from
/// `growth`.
pub(in crate::diagram::flowchart) fn grow_for_labels(
    chart: &Flowchart,
    growth: &Growth,
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
    let mut boxes = Vec::with_capacity(chart.nodes.len());
    for (index, (node, mut arriving)) in chart.nodes.iter().zip(arrivals).enumerate() {
        let spread = *growth.boxes.get(index)?;
        if arriving.iter().all(|entry| entry.reach.is_none()) {
            boxes.push(spread);
            continue;
        }
        arriving.sort_by_key(|entry| (entry.from_port, entry.edge_index));
        let ordered: Vec<Option<Reach>> = arriving.iter().map(|entry| entry.reach).collect();
        let offsets = symmetric_offsets(ordered.len(), &end_gaps(axis, &ordered))?;
        let farthest_port = offsets.iter().map(|offset| offset.unsigned_abs()).max().unwrap_or(0);
        boxes.push(spread.max(spread_for(node, axis, Some(farthest_port))));
    }
    Some(Growth { boxes, frames: growth.frames.clone() })
}

/// A drawn link entering a box, as [`grow_for_labels`] sizes the box for it.
struct LabelledEntry {
    /// The port cell across the flow the link leaves its source from.
    from_port: usize,
    /// The link's index in [`Flowchart::edges`].
    edge_index: usize,
    /// The reach of the link's label at the entry, if it has one.
    reach: Option<Reach>,
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
/// changes. Returns `growth` with each frame grown by what `layered` measured; `None`
/// when every frame already grows by that.
pub(in crate::diagram::flowchart) fn grow_frames(
    growth: &Growth,
    layered: &Layered,
) -> Option<Growth> {
    let changed = growth.frames != layered.frame_needs;
    changed.then(|| Growth { boxes: growth.boxes.clone(), frames: layered.frame_needs.clone() })
}

/// The cells each box and each subgraph frame grows by across the flow on both sides, so
/// that every link end on one of its borders has a cell of its own. The layout measures
/// it ([`grow_boxes`], [`grow_for_labels`], [`grow_frames`]) and every stage after
/// parsing draws the chart grown by it.
#[derive(Debug, PartialEq, Eq)]
pub(in crate::diagram::flowchart) struct Growth {
    /// The cells each box grows by on each side of its label, indexed like
    /// [`Flowchart::nodes`].
    pub boxes: Vec<usize>,
    /// The cells each frame grows by on each side of its members, indexed like
    /// [`Flowchart::subgraphs`].
    pub frames: Vec<usize>,
}
