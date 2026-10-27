//! Positions along the flow, the paths of links between the placed boxes, and where
//! link labels go. Positions are `main` and `cross`: cells along and across the flow.

mod frames;

use std::ops::RangeInclusive;

use super::chart::{Body, Direction, Edge, End, Flowchart, Node};
use super::label::Label;
use super::layout::{
    Axis, Growth, LabelSpot, LaidOutEnd, Layered, Reach, SELF_LOOP_CELLS, SELF_LOOP_SPAN,
    entry_reach, exit_cells, label_cross_reach, label_spot, on_run, port_cells, scatter_by_order,
    self_loop_cells, self_loop_label_at,
};
use super::outline::outline;
use super::signed;
use super::styling::Styling;

use self::frames::{FrameBounds, Marked, Nesting, TitlePlacement, frame_bounds, frames_overlap};

/// A drawing before it is oriented on screen.
#[derive(Debug)]
pub(super) struct Scene<'a> {
    pub boxes: Vec<PlacedBox<'a>>,
    /// The drawn links, in the order of their edges; an invisible link has none.
    pub links: Vec<Route<'a>>,
    pub labels: Vec<PlacedLabel<'a>>,
    pub frames: Vec<PlacedFrame<'a>>,
}

/// A drawing as [`route`] lays it out, before its labels are placed (see
/// [`place_labels`]).
#[derive(Debug)]
pub(super) struct Routed<'a> {
    pub boxes: Vec<PlacedBox<'a>>,
    /// The drawn links, in the order of their edges; an invisible link has none.
    pub links: Vec<Route<'a>>,
    pub labels: Vec<RoutedLabel<'a>>,
    pub frames: Vec<PlacedFrame<'a>>,
}

/// The boxes, links and labels of a drawing as [`route`] places them before it puts
/// frames around the subgraphs, which take their cells from these: a link to or from a
/// subgraph still runs to a cell among the subgraph's members, or to its track in the gap
/// beside a frame it meets across the flow, until [`frame_ended`] ends it at the frame,
/// and a subgraph's self loop has no cells yet.
#[derive(Debug)]
struct Unframed<'a> {
    boxes: Vec<PlacedBox<'a>>,
    /// The drawn links, in the order of their edges; an invisible link has none.
    links: Vec<Route<'a>>,
    labels: Vec<RoutedLabel<'a>>,
}

/// A label whose place the routing fixed: on its link's line, or outside a self loop.
#[derive(Debug)]
pub(super) struct RoutedLabel<'a> {
    /// The index of the label's link in [`Scene::links`].
    pub link: usize,
    pub label: &'a Label,
    /// Its first cell along and across the flow, which may lie before the first cell
    /// across the flow (see [`place_labels`]).
    pub at: (isize, isize),
}

/// A cell of a scene, along and across the flow.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(super) struct Cell {
    pub main: usize,
    pub cross: usize,
}

/// A box with its first cell along and across the flow.
#[derive(Debug)]
pub(super) struct PlacedBox<'a> {
    pub node: &'a Node,
    /// The cells the box grows by across the flow on each side of its label (see
    /// [`Growth::boxes`]).
    pub spread: usize,
    pub main: usize,
    pub cross: usize,
}

#[derive(Debug)]
pub(super) struct Route<'a> {
    pub edge: &'a Edge,
    /// Corners of the line, from the cell next to the source's border to the cell next
    /// to the target's border.
    pub points: Vec<Cell>,
}

/// A label starting at a cell; its rows extend along the flow in a horizontal layout and
/// across it in a vertical one, i.e. always to the right on screen, and follow each other
/// down the screen.
#[derive(Debug)]
pub(super) struct PlacedLabel<'a> {
    /// The index of the label's link in [`Scene::links`].
    pub link: usize,
    pub label: &'a Label,
    pub main: usize,
    pub cross: usize,
}

/// A subgraph frame, given by the cells of its borders along and across the flow.
#[derive(Debug)]
pub(super) struct PlacedFrame<'a> {
    /// What the subgraph's classes and `style` statements set.
    pub styling: Styling,
    pub title: &'a Label,
    /// Cells on screen from the top-left corner to the blank before the title.
    pub title_offset: usize,
    pub main: RangeInclusive<usize>,
    pub cross: RangeInclusive<usize>,
}

impl Axis {
    /// Cells a label takes along and across the flow: its widest row, and its rows.
    pub(super) fn label_size(self, label: &Label) -> (usize, usize) {
        match self {
            Self::Horizontal => (label.width(), label.height()),
            Self::Vertical => (label.height(), label.width()),
        }
    }
}

impl Scene<'_> {
    /// Cells the scene spans along and across the flow, from its first cells.
    pub(super) fn extent(&self, axis: Axis) -> (usize, usize) {
        let labels = self.labels.iter().map(|label| {
            let (main_size, cross_size) = axis.label_size(label.label);
            (label.main + main_size, label.cross + cross_size)
        });
        extent(axis, &self.boxes, &self.links, &self.frames, labels)
    }
}

impl Routed<'_> {
    /// Cells the drawing spans along and across the flow, from its first cells, without
    /// its labels, which only add cells.
    pub(super) fn extent_without_labels(&self, axis: Axis) -> (usize, usize) {
        extent(axis, &self.boxes, &self.links, &self.frames, std::iter::empty())
    }
}

/// Cells `boxes`, `links`, `frames` and the cells `labels` end before span along and
/// across the flow, from the first cells.
fn extent(
    axis: Axis,
    boxes: &[PlacedBox<'_>],
    links: &[Route<'_>],
    frames: &[PlacedFrame<'_>],
    labels: impl Iterator<Item = (usize, usize)>,
) -> (usize, usize) {
    let boxes = boxes.iter().map(|placed| {
        (
            placed.main + axis.box_main_size(placed.node),
            placed.cross + axis.box_cross_size(placed.node, placed.spread),
        )
    });
    let links = links.iter().flat_map(|link| &link.points);
    let frames = frames.iter().map(|frame| (frame.main.end() + 1, frame.cross.end() + 1));
    boxes
        .chain(links.map(|cell| (cell.main + 1, cell.cross + 1)))
        .chain(labels)
        .chain(frames)
        .fold((0, 0), |(main, cross), (end_main, end_cross)| {
            (main.max(end_main), cross.max(end_cross))
        })
}

/// The drawing of `layered`, its boxes and frames grown as `growth` says, with its layers
/// spaced along the flow, each gap at least
/// `layer_gap` cells: every box, the line of every visible link along rows and columns
/// from its source to its target, a [`RoutedLabel`] for every link label, and a frame
/// around each subgraph's content, given each subgraph's `memberships` (see
/// [`Flowchart::memberships`]). `None` when a subgraph frame would cover a box outside
/// its subgraph or a frame neither nested in it nor enclosing it, or a cell does not fit
/// an `isize`.
pub(super) fn route<'a>(
    chart: &'a Flowchart,
    growth: &'a Growth,
    layered: &'a Layered,
    memberships: &'a [Vec<bool>],
    axis: Axis,
    layer_gap: usize,
) -> Option<Routed<'a>> {
    let router = Router::new(chart, growth, layered, memberships, axis, layer_gap)?;
    let slots = &layered.slots;
    let place = |layer_start: &[usize], exit: &[usize]| -> Option<Unframed<'a>> {
        let gap_start =
            |layer: usize| Some(layer_start.get(layer)? + router.layer_size.get(layer)?);
        let mut routed = Unframed { boxes: Vec::new(), links: Vec::new(), labels: Vec::new() };
        for ((node, &spread), slot) in chart.nodes.iter().zip(&growth.boxes).zip(slots) {
            routed.boxes.push(PlacedBox {
                node,
                spread,
                main: *layer_start.get(slot.layer)?,
                cross: slot.cross,
            });
        }
        for (index, edge_route) in router.edges.iter().enumerate() {
            let &EdgeRoute { edge, path, reversed, exit: exit_cell, ref entries, .. } = edge_route;
            if !edge.stroke.is_visible() {
                continue;
            }
            // A subgraph's self loop gets its cells once its frame is placed.
            if edge.frame_loop().is_some() {
                routed.links.push(Route { edge, points: Vec::new() });
                continue;
            }
            let (&first, &last) = (path.first()?, path.last()?);
            let first_slot = slots.get(first)?;
            if first == last {
                let node = chart.nodes.get(first)?;
                let main = *layer_start.get(first_slot.layer)?;
                let out = main + axis.box_main_size(node);
                if let Some(text) = &edge.label {
                    let at = self_loop_label(axis, out, exit_cell, text)?;
                    routed.labels.push(RoutedLabel { link: routed.links.len(), label: text, at });
                }
                let inset = side_inset(chart.direction, node, true);
                let points = self_loop(out, inset, exit_cell);
                routed.links.push(Route { edge, points });
                continue;
            }
            let out = match chart.nodes.get(first) {
                Some(node) => (layer_start.get(first_slot.layer)? + axis.box_main_size(node))
                    .checked_sub(side_inset(chart.direction, node, true))?,
                None => gap_start(first_slot.layer)?,
            };
            let mut points = vec![Cell { main: out, cross: exit_cell }];
            for (segment, (&from_index, &entry)) in path.iter().zip(entries).enumerate() {
                let from = slots.get(from_index)?;
                let leave = if segment == 0 { exit_cell } else { from.port };
                if leave != entry {
                    let track = gap_start(from.layer)?
                        + exit.get(from.layer)?
                        + 1
                        + router.loop_cells.get(from.layer)?
                        + router.run_label_cells.get(from.layer)?
                        + router.track_of.get(index)?.get(segment)?;
                    points.extend([
                        Cell { main: track, cross: leave },
                        Cell { main: track, cross: entry },
                    ]);
                }
            }
            if let Some(text) = &edge.label
                && let Some(spot) = label_spot(path)
            {
                let width = text.width();
                let before_line = label_cross_reach(axis, text).before;
                let at = match spot {
                    // A link entering a frame across the flow ends on its track (see
                    // `meet_beside`), so its label goes on the run before the track, just
                    // after the cell the line leaves its source through, in the cells
                    // `run_label_cells` keeps there.
                    LabelSpot::Gap(segment) if on_run(&layered.beside, index) => {
                        let layer = slots.get(*path.get(segment)?)?.layer;
                        let past_exit = gap_start(layer)?
                            + exit.get(layer)?
                            + 1
                            + router.loop_cells.get(layer)?;
                        let along = match axis {
                            Axis::Vertical => past_exit,
                            Axis::Horizontal => {
                                past_exit + gap_end_markers(edge, reversed, path, segment).0
                            }
                        };
                        (signed(along)?, signed(exit_cell)? - signed(before_line)?)
                    }
                    // Upstream puts the label at its edge-label dummy node on the middle
                    // rank (`edge.x`/`edge.y`, see `positionEdgeLabel`), placed across the
                    // flow like any node, and the curve bends through it; an orthogonal
                    // line turns on a track, and a label on the track would cover the
                    // turn, so the label goes on the line after the tracks, at the cell
                    // the line enters the next layer by. For a straight link the two
                    // coincide.
                    LabelSpot::Gap(segment) => {
                        let layer = slots.get(*path.get(segment)?)?.layer;
                        let after_tracks =
                            gap_start(layer)? + exit.get(layer)? + router.leads.get(layer)?;
                        let entry = signed(*entries.get(segment)?)? - signed(before_line)?;
                        match axis {
                            Axis::Vertical => (signed(after_tracks)?, entry),
                            Axis::Horizontal => {
                                let gap_end = gap_start(layer)?
                                    + exit.get(layer)?
                                    + router.gaps.get(layer)?;
                                let (before, after) =
                                    gap_end_markers(edge, reversed, path, segment);
                                let low = after_tracks + 1 + before;
                                let high = gap_end.checked_sub(2 + after)?;
                                (signed(centred_start(low, high, width)?)?, entry)
                            }
                        }
                    }
                    // The line runs straight through the passing slot at its port, in a layer
                    // `Slot::main` sized for the label: a vertical label's middle row is on
                    // the layer's, each the upper of two for an even count, so a label as
                    // tall as the layer fills it.
                    LabelSpot::Slot(index) => {
                        let slot = slots.get(*path.get(index)?)?;
                        let start = *layer_start.get(slot.layer)?;
                        let size = *router.layer_size.get(slot.layer)?;
                        let cross = signed(slot.port)? - signed(before_line)?;
                        match axis {
                            Axis::Vertical => {
                                let middle = start + size.checked_sub(1)? / 2;
                                (signed(middle.checked_sub(text.middle_row())?)?, cross)
                            }
                            Axis::Horizontal => {
                                let end = (start + size).checked_sub(1)?;
                                (signed(centred_start(start, end, width)?)?, cross)
                            }
                        }
                    }
                };
                routed.labels.push(RoutedLabel { link: routed.links.len(), label: text, at });
            }
            let entry = *entries.last()?;
            let into = layer_start.get(slots.get(last)?.layer)?.checked_sub(1)?
                + chart.nodes.get(last).map_or(0, |node| side_inset(chart.direction, node, false));
            points.push(Cell { main: into, cross: entry });
            if reversed {
                points.reverse();
            }
            routed.links.push(Route { edge, points });
        }
        Some(routed)
    };

    // The margins a frame needs past its boxes do not depend on where the layers start,
    // and in a vertical layout a title widens its frame across the flow only, so there a
    // first placement without room for frames measures an upper bound on the room to
    // make. A horizontal title runs along the flow, so that layout is measured again
    // below.
    let no_margins = vec![0; layered.layer_count];
    let unframed_start = router.layer_starts(&no_margins, &no_margins)?;
    let (mut enter, mut exit) =
        router.margins(&unframed_start, &place(&unframed_start, &no_margins)?)?;
    let mut layer_start = router.layer_starts(&enter, &exit)?;
    let mut routed = place(&layer_start, &exit)?;
    // In a horizontal layout the title runs along the flow, and the room made moves the
    // tracks its links cross the top border on, so the title may need more room than
    // was made. The layers are spaced out again only once: the overlap check below
    // accepts or rejects the result, so no further round is needed.
    if axis == Axis::Horizontal {
        let (needed_enter, needed_exit) = router.margins(&layer_start, &routed)?;
        let short = |needed: &[usize], made: &[usize]| needed.iter().zip(made).any(|(n, m)| n > m);
        if short(&needed_enter, &enter) || short(&needed_exit, &exit) {
            let wider = |needed: &[usize], made: &[usize]| {
                needed.iter().zip(made).map(|(&n, &m)| n.max(m)).collect::<Vec<_>>()
            };
            (enter, exit) = (wider(&needed_enter, &enter), wider(&needed_exit, &exit));
            layer_start = router.layer_starts(&enter, &exit)?;
            routed = place(&layer_start, &exit)?;
        }
    }
    // The room left for a title clear of the links crossing its frame's top border
    // was worked out before the final placement; when that room falls short and a frame
    // covers another box or frame, every title stays at its corner, over the links,
    // rather than the drawing being given up.
    let mut framed = router.frames(&routed, TitlePlacement::ClearOfLinks);
    if frames_overlap(chart, &framed, &routed.boxes, axis) {
        framed = router.frames(&routed, TitlePlacement::AtCorner);
        if frames_overlap(chart, &framed, &routed.boxes, axis) {
            return None;
        }
    }
    // Frames reaching past the first cells along or across the flow move everything
    // else on by as much.
    let (main_shift, cross_shift) = framed.iter().fold((0, 0), |(main, cross), (.., frame)| {
        (main.max(-frame.main.start()), cross.max(-frame.cross.start()))
    });
    let (main_by, cross_by) = (unsigned(main_shift)?, unsigned(cross_shift)?);
    let Unframed { boxes, links: unframed_links, mut labels } =
        routed.shifted(main_by, cross_by)?;
    let moved = |span: &RangeInclusive<isize>, by: isize| {
        Some(unsigned(span.start() + by)?..=unsigned(span.end() + by)?)
    };
    let mut frame_of = vec![None; chart.subgraphs.len()];
    let mut frames = Vec::new();
    for (index, _, frame) in framed {
        let main = moved(&frame.main, main_shift)?;
        let cross = moved(&frame.cross, cross_shift)?;
        *frame_of.get_mut(index)? = Some((main.clone(), cross.clone()));
        let styling = chart.subgraphs.get(index)?.styling;
        frames.push(PlacedFrame {
            styling,
            title: frame.title,
            title_offset: frame.title_offset,
            main,
            cross,
        });
    }
    let drawn = router.edges.iter().filter(|edge_route| edge_route.edge.stroke.is_visible());
    let mut links = Vec::with_capacity(unframed_links.len());
    let mut loop_labels = Vec::new();
    for (Route { edge, points }, edge_route) in unframed_links.into_iter().zip(drawn) {
        let &EdgeRoute { reversed, beside, frame_loop_leg: loop_leg, .. } = edge_route;
        // A subgraph's self loop bumps out of its frame's far border along the flow as a
        // node's does out of its box, its legs on the cells the layout kept for them on
        // that border.
        if let Some(subgraph) = edge.frame_loop() {
            let (main, _) = frame_of.get(subgraph)?.as_ref()?;
            let out = main.end() + 1;
            let port = loop_leg? + cross_by;
            if let Some(text) = &edge.label {
                let at = self_loop_label(axis, out, port, text)?;
                loop_labels.push(RoutedLabel { link: links.len(), label: text, at });
            }
            links.push(Route { edge, points: self_loop(out, 0, port) });
            continue;
        }
        let points = frame_ended(points, edge, reversed, beside, &frame_of)?;
        links.push(Route { edge, points });
    }
    labels.extend(loop_labels);
    Some(Routed { boxes, links, labels, frames })
}

/// The line `points` of `edge`, laid out from its target when `reversed`, ended at the
/// frames `frame_of` of the subgraphs it links. A link to or from a subgraph was routed
/// to a cell among its members; it ends in the cell just outside the frame's border
/// instead, where its marker goes, as at a box: before the frame along the flow where
/// the link is laid out into it, after it where the link is laid out from it, or beside
/// it across the flow at the end `beside` names (see [`meet_beside`]). `None` when an
/// end the layout puts beside a frame is not a subgraph with a frame, or the line has no
/// turn to meet it on.
fn frame_ended(
    points: Vec<Cell>,
    edge: &Edge,
    reversed: bool,
    beside: Option<LaidOutEnd>,
    frame_of: &[Option<(RangeInclusive<usize>, RangeInclusive<usize>)>],
) -> Option<Vec<Cell>> {
    let (written_from, written_to) = (edge.from, edge.to);
    // Whether the end at a frame's border across the flow is the link's first point.
    let beside_first = beside.map(|end| (end == LaidOutEnd::Source) != reversed);
    let mut points = match beside_first {
        Some(at_first) => {
            let End::Subgraph(subgraph) = (if at_first { written_from } else { written_to }) else {
                return None;
            };
            let (_, cross) = frame_of.get(subgraph)?.as_ref()?;
            meet_beside(points, at_first, cross)?
        }
        None => points,
    };
    let (first, last) = (points.first().copied()?, points.last().copied()?);
    let ends = [
        (written_from, !reversed, first, beside_first == Some(true)),
        (written_to, reversed, last, beside_first == Some(false)),
    ];
    let [from, to] = ends.map(|(end, leaves, cell, beside)| match end {
        End::Subgraph(subgraph) if !beside => {
            let (frame, _) = frame_of.get(subgraph)?.as_ref()?;
            let main = if leaves { frame.end() + 1 } else { frame.start().checked_sub(1)? };
            Some(Cell { main, ..cell })
        }
        _ => Some(cell),
    });
    *points.first_mut()? = from?;
    *points.last_mut()? = to?;
    Some(points)
}

/// The line `points` of a link meeting a frame across the flow (see
/// [`Layered::beside`]), ended at its first point when `at_first` and at its last
/// otherwise in the cell just outside the frame's border that faces the link's other
/// end, given the frame's cells across the flow, `cross`. The link was routed from the
/// frame's end in the gap beside the frame to the track where it turns; it meets the
/// frame on that track instead. `None` when the line has no such turn.
fn meet_beside(
    mut points: Vec<Cell>,
    at_first: bool,
    cross: &RangeInclusive<usize>,
) -> Option<Vec<Cell>> {
    if !at_first {
        points.reverse();
    }
    if points.len() < 3 {
        return None;
    }
    points.remove(0);
    let toward = points.get(1)?.cross;
    let end = points.first_mut()?;
    end.cross =
        if toward > *cross.end() { cross.end() + 1 } else { cross.start().checked_sub(1)? };
    if !at_first {
        points.reverse();
    }
    Some(points)
}

/// The spacing of the layers and the frames around the subgraphs, shared by the rounds
/// of [`route`] that space the layers out again for the frames.
struct Router<'a> {
    chart: &'a Flowchart,
    growth: &'a Growth,
    layered: &'a Layered,
    /// Every edge, drawn or not, as the routing lays out its line, indexed like
    /// [`Flowchart::edges`].
    edges: Vec<EdgeRoute<'a>>,
    /// By edge and then segment, like [`EdgeRoute::entries`], the track in its gap of
    /// each segment that turns there.
    track_of: Vec<Vec<usize>>,
    layer_size: Vec<usize>,
    loop_cells: Vec<usize>,
    /// The cells each gap gives the labels on runs after its self loops and before its
    /// tracks (see [`on_run`]).
    run_label_cells: Vec<usize>,
    leads: Vec<usize>,
    gaps: Vec<usize>,
    memberships: &'a [Vec<bool>],
    /// Inner frames first, so that each outer one is drawn around them.
    innermost_first: Vec<usize>,
    /// By subgraph and then subgraph, whether the second is nested in the first at any
    /// depth.
    nested: Vec<Vec<bool>>,
    /// By subgraph, the subgraphs nested in it directly.
    children: Vec<Vec<usize>>,
    /// Where a drawn link's marker lies next to each subgraph's frame along the flow:
    /// [`route`] ends a link to or from a subgraph in the cell just outside the frame's
    /// border, where its marker goes.
    marked: Vec<Marked>,
    /// The cells along the flow each subgraph's self loops take after its frame, as
    /// [`self_loop_cells`] counts them; 0 for a frame with none.
    frame_loop_cells: Vec<usize>,
}

impl<'a> Router<'a> {
    /// Assigns the ports and tracks of `layered` and sizes its layers and gaps, each gap
    /// at least `layer_gap` cells.
    fn new(
        chart: &'a Flowchart,
        growth: &'a Growth,
        layered: &'a Layered,
        memberships: &'a [Vec<bool>],
        axis: Axis,
        layer_gap: usize,
    ) -> Option<Self> {
        let mut layer_size = vec![0; layered.layer_count];
        for slot in &layered.slots {
            let size = layer_size.get_mut(slot.layer)?;
            *size = (*size).max(slot.main);
        }
        let entries = entry_ports(chart, growth, layered, axis)?;
        let exits = exit_ports(chart, growth, layered, axis)?;
        let edges = edge_routes(chart, layered, entries, &exits)?;
        let runs = runs(layered, &edges)?;
        let routed_label_cells = routed_label_cells(chart, layered, axis)?;
        let run_label_cells = run_label_cells(chart, layered, axis)?;
        let (track_of, track_counts) = assign_tracks(layered, &edges, &runs)?;
        let loop_cells = loop_cells(chart, layered, axis)?;
        let frame_loop_cells = frame_loop_cells(chart, axis)?;

        // The cells of each gap before the cell after its last track: the cell the line
        // leaves the box through, the cells of self loops, those of the labels on runs and
        // the tracks. A vertical label row comes after those even without tracks, and a
        // horizontal label after the self loops. Otherwise a horizontal gap without tracks
        // has a lead of 0: a straight line's first cell, where the line leaves the box, is
        // the line cell before its label, or holds the link's marker there with that line
        // cell after it (a back edge's head, a `<-->` tail; see `gap_end_markers`).
        let leads: Vec<usize> = track_counts
            .iter()
            .zip(&loop_cells)
            .zip(&run_label_cells)
            .zip(&routed_label_cells)
            .map(|(((&tracks, &loop_cells), &run_cells), routed_cells)| {
                let label_lead =
                    routed_cells.is_some() && (axis == Axis::Vertical || loop_cells > 0);
                if tracks == 0 && run_cells == 0 && !label_lead {
                    0
                } else {
                    1 + loop_cells + run_cells + tracks
                }
            })
            .collect();
        let gaps: Vec<usize> = leads
            .iter()
            .zip(&routed_label_cells)
            .zip(&loop_cells)
            .map(|((&lead, &routed_cells), &loop_cells)| {
                // After the last track, a horizontal gap holds a line cell and the arrowhead,
                // or a label on the line with its markers and a line cell on either side of
                // the text; a vertical one the label rows, if any, and the cell of the
                // arrowhead.
                let needed = match axis {
                    Axis::Horizontal => lead + routed_cells.unwrap_or(0) + 2,
                    Axis::Vertical => lead + routed_cells.unwrap_or(0) + 1,
                };
                // A blank cell separates the next layer from the loops and their labels.
                let loops = if loop_cells > 0 { loop_cells + 1 } else { 0 };
                layer_gap.max(needed).max(loops)
            })
            .collect();
        Some(Self {
            chart,
            growth,
            layered,
            track_of,
            layer_size,
            loop_cells,
            run_label_cells,
            leads,
            gaps,
            memberships,
            innermost_first: chart.innermost_first(),
            nested: nested(chart),
            children: children(chart),
            marked: marked(&edges, chart.subgraphs.len()),
            edges,
            frame_loop_cells,
        })
    }

    /// Where each layer starts along the flow. Each layer is preceded by the `enter`
    /// cells the frames opening at it take before its boxes, and its gap begins with the
    /// `exit` cells the frames closing at it take after them, so that tracks, labels and
    /// the previous layer stay clear of the frames.
    fn layer_starts(&self, enter: &[usize], exit: &[usize]) -> Option<Vec<usize>> {
        let mut layer_start = Vec::with_capacity(self.layered.layer_count);
        let mut next = 0;
        for (layer, (size, gap)) in self.layer_size.iter().zip(&self.gaps).enumerate() {
            next += enter.get(layer)?;
            layer_start.push(next);
            next += size + exit.get(layer)? + gap;
        }
        Some(layer_start)
    }

    /// The frames of the subgraphs that get one around `routed`, by subgraph, with their
    /// membership tables, each title placed as `title` says.
    fn frames(
        &self,
        routed: &Unframed<'_>,
        title: TitlePlacement,
    ) -> Vec<(usize, &[bool], FrameBounds<'a>)> {
        let chart = self.chart;
        let mut bounds: Vec<Option<FrameBounds<'a>>> =
            chart.subgraphs.iter().map(|_| None).collect();
        for &index in &self.innermost_first {
            let (Some(subgraph), Some(&spread), Some(is_member), Some(children)) = (
                chart.subgraphs.get(index),
                self.growth.frames.get(index),
                self.memberships.get(index),
                self.children.get(index),
            ) else {
                continue;
            };
            let inner: Vec<_> = children
                .iter()
                // A nested frame's self loops lie inside this frame, after the nested one.
                .filter_map(|&child| {
                    let inner = bounds.get(child)?.as_ref()?;
                    let loop_cells = signed(*self.frame_loop_cells.get(child)?)?;
                    let main = *inner.main.start()..=inner.main.end() + loop_cells;
                    Some(((main, inner.cross.clone()), *self.marked.get(child)?))
                })
                .collect();
            let nesting = Nesting { subgraph, index, spread, nested: &self.nested };
            if let Some(slot) = bounds.get_mut(index) {
                *slot = frame_bounds(&nesting, is_member, &inner, routed, chart.direction, title);
            }
        }
        bounds
            .into_iter()
            .zip(self.memberships)
            .enumerate()
            .filter_map(|(index, (frame, is_member))| Some((index, is_member.as_slice(), frame?)))
            .collect::<Vec<_>>()
    }

    /// The cells each layer's frames take before and after its boxes in `routed`, laid
    /// out from `layer_start`.
    fn margins(
        &self,
        layer_start: &[usize],
        routed: &Unframed<'_>,
    ) -> Option<(Vec<usize>, Vec<usize>)> {
        let layered = self.layered;
        let mut enter = vec![0; layered.layer_count];
        let mut exit = vec![0; layered.layer_count];
        for (index, _, frame) in self.frames(routed, TitlePlacement::ClearOfLinks) {
            let (first, last) = layered.member_layers.get(index)?.clone().into_inner();
            let before = signed(*layer_start.get(first)?)? - frame.main.start();
            let after =
                frame.main.end() + 1 - signed(layer_start.get(last)? + self.layer_size.get(last)?)?;
            let enter = enter.get_mut(first)?;
            *enter = (*enter).max(unsigned(before.max(0))?);
            let exit = exit.get_mut(last)?;
            *exit = (*exit).max(unsigned(after.max(0))?);
        }
        Some((enter, exit))
    }
}

/// One edge of a chart as the routing lays out its line: what [`Layered`] says of it and
/// the cells across the flow its line leaves and enters the slots of its path by.
struct EdgeRoute<'a> {
    edge: &'a Edge,
    /// See [`Layered::paths`].
    path: &'a [usize],
    /// See [`Layered::reversed`].
    reversed: bool,
    /// See [`Layered::beside`].
    beside: Option<LaidOutEnd>,
    /// See [`Layered::frame_loop_legs`].
    frame_loop_leg: Option<usize>,
    /// The cell across the flow where the line leaves the first slot of its path; for a
    /// self loop, the cell of its first leg (see [`exit_cells`]).
    exit: usize,
    /// The cell across the flow where each segment enters the next slot on the path,
    /// indexed like `path` without its first slot.
    entries: Vec<usize>,
}

/// Every edge of `chart` as the routing lays out its line in `layered`, given each
/// edge's entry cells `entries` (see [`entry_ports`]) and exit cell in `exits` (see
/// [`exit_ports`]). `None` when a table of `layered` misses an edge.
fn edge_routes<'a>(
    chart: &'a Flowchart,
    layered: &'a Layered,
    entries: Vec<Vec<usize>>,
    exits: &[usize],
) -> Option<Vec<EdgeRoute<'a>>> {
    chart
        .edges
        .iter()
        .zip(entries)
        .enumerate()
        .map(|(index, (edge, entries))| {
            Some(EdgeRoute {
                edge,
                path: layered.paths.get(index)?,
                reversed: *layered.reversed.get(index)?,
                beside: *layered.beside.get(index)?,
                frame_loop_leg: *layered.frame_loop_legs.get(index)?,
                exit: *exits.get(index)?,
                entries,
            })
        })
        .collect()
}

/// The runs of the visible links' segments that turn in the gap after their sources.
fn runs(layered: &Layered, edges: &[EdgeRoute<'_>]) -> Option<Vec<Run>> {
    let mut runs = Vec::new();
    for (index, edge_route) in edges.iter().enumerate() {
        let &EdgeRoute { edge, path, exit, ref entries, .. } = edge_route;
        for (segment, (&from, &entry)) in path.iter().zip(entries).enumerate() {
            let leave = if segment == 0 { exit } else { layered.slots.get(from)?.port };
            if leave != entry && edge.stroke.is_visible() {
                runs.push(Run {
                    source: from,
                    edge: index,
                    segment,
                    leave,
                    cells: leave.min(entry)..=leave.max(entry),
                    rightward: entry > leave,
                });
            }
        }
    }
    Some(runs)
}

/// The labels that lie in a gap (see [`LabelSpot::Gap`]), each with its link's index,
/// the layer the gap follows and the markers at the gap's ends (see
/// [`gap_end_markers`]). A label on a passing slot lies in its layer, which `Slot::main`
/// sized for it; a self loop's label is counted in [`loop_cells`].
fn gap_labels<'a>(
    chart: &'a Flowchart,
    layered: &'a Layered,
) -> impl Iterator<Item = Option<(usize, &'a Label, usize, (usize, usize))>> {
    chart.edges.iter().zip(&layered.paths).zip(&layered.reversed).enumerate().filter_map(
        |(index, ((edge, path), &reversed))| {
            let label = edge.label.as_ref()?;
            let Some(LabelSpot::Gap(segment)) = label_spot(path) else {
                return None;
            };
            let markers = gap_end_markers(edge, reversed, path, segment);
            let slot = path.get(segment).and_then(|&slot| layered.slots.get(slot));
            Some(slot.map(|slot| (index, label, slot.layer, markers)))
        },
    )
}

/// For each gap, the cells the routed labels on its lines need after the tracks: in a
/// horizontal layout a label's widest row and the markers at the gap's ends, in a
/// vertical one a label's rows; `None` for a gap without such labels.
fn routed_label_cells(
    chart: &Flowchart,
    layered: &Layered,
    axis: Axis,
) -> Option<Vec<Option<usize>>> {
    let mut routed_label_cells = vec![None; layered.layer_count];
    for gap_label in gap_labels(chart, layered) {
        let (index, label, layer, (before, after)) = gap_label?;
        if on_run(&layered.beside, index) {
            continue;
        }
        let needed = match axis {
            Axis::Horizontal => before + label.width() + after,
            Axis::Vertical => label.height(),
        };
        let cells = routed_label_cells.get_mut(layer)?;
        *cells = Some(cells.unwrap_or(0).max(needed));
    }
    Some(routed_label_cells)
}

/// For each gap, the cells the labels on runs (see [`on_run`]) need before the tracks.
fn run_label_cells(chart: &Flowchart, layered: &Layered, axis: Axis) -> Option<Vec<usize>> {
    let mut run_label_cells = vec![0; layered.layer_count];
    for gap_label in gap_labels(chart, layered) {
        let (index, label, layer, (before, _)) = gap_label?;
        if !on_run(&layered.beside, index) {
            continue;
        }
        let needed = match axis {
            Axis::Horizontal => before + label.width() + 2,
            Axis::Vertical => label.height(),
        };
        let cells = run_label_cells.get_mut(layer)?;
        *cells = (*cells).max(needed);
    }
    Some(run_label_cells)
}

/// For each layer, the cells along the flow the self loops after its boxes take with
/// their labels, as [`self_loop_cells`] counts them: the tracks after the layer, and the
/// next layer, start beyond them. A subgraph's self loop counts in the layer its frame
/// ends at, as it takes its cells after the frame.
fn loop_cells(chart: &Flowchart, layered: &Layered, axis: Axis) -> Option<Vec<usize>> {
    let mut loop_cells = vec![0; layered.layer_count];
    for edge in &chart.edges {
        if !edge.stroke.is_visible() {
            continue;
        }
        let layer = match (edge.nodes(), edge.frame_loop()) {
            (Some((node, to)), _) if node == to => layered.slots.get(node)?.layer,
            (_, Some(subgraph)) => *layered.member_layers.get(subgraph)?.end(),
            _ => continue,
        };
        let cells = loop_cells.get_mut(layer)?;
        *cells = (*cells).max(self_loop_cells(axis, edge.label.as_ref()));
    }
    Some(loop_cells)
}

/// The cells along the flow each subgraph's self loops take after its frame, as
/// [`self_loop_cells`] counts them; 0 for a frame with none.
fn frame_loop_cells(chart: &Flowchart, axis: Axis) -> Option<Vec<usize>> {
    let mut frame_loop_cells = vec![0; chart.subgraphs.len()];
    for edge in &chart.edges {
        let Some(subgraph) = edge.frame_loop() else {
            continue;
        };
        if !edge.stroke.is_visible() {
            continue;
        }
        let cells = frame_loop_cells.get_mut(subgraph)?;
        *cells = (*cells).max(self_loop_cells(axis, edge.label.as_ref()));
    }
    Some(frame_loop_cells)
}

/// By subgraph and then subgraph, whether the second is nested in the first at any
/// depth.
fn nested(chart: &Flowchart) -> Vec<Vec<bool>> {
    let count = chart.subgraphs.len();
    let mut nested = vec![vec![false; count]; count];
    for inner in 0..count {
        for outer in chart.enclosing(inner) {
            if let Some(cell) = nested.get_mut(outer).and_then(|row| row.get_mut(inner)) {
                *cell = true;
            }
        }
    }
    nested
}

/// By subgraph, the subgraphs nested in it directly, in the order of their indices.
fn children(chart: &Flowchart) -> Vec<Vec<usize>> {
    let mut children = vec![Vec::new(); chart.subgraphs.len()];
    for (index, subgraph) in chart.subgraphs.iter().enumerate() {
        if let Some(siblings) = subgraph.parent.and_then(|parent| children.get_mut(parent)) {
            siblings.push(index);
        }
    }
    children
}

/// Where a drawn link's marker lies next to each subgraph's frame along the flow.
fn marked(edges: &[EdgeRoute<'_>], subgraph_count: usize) -> Vec<Marked> {
    let mut marked = vec![Marked::default(); subgraph_count];
    for &EdgeRoute { edge, reversed, beside, .. } in edges {
        // A subgraph's self loop and its marker count among the frame's loop cells.
        if !edge.stroke.is_visible() || edge.frame_loop().is_some() {
            continue;
        }
        // A link meeting a frame across the flow has its marker there, not along it.
        let along = |leaves: bool| {
            beside != Some(if leaves { LaidOutEnd::Source } else { LaidOutEnd::Target })
        };
        let ends = [
            (edge.from, !reversed, edge.tail.is_some() && along(!reversed)),
            (edge.to, reversed, edge.head.is_some() && along(reversed)),
        ];
        for (end, leaves, marker) in ends {
            if let End::Subgraph(subgraph) = end
                && marker
                && let Some(mark) = marked.get_mut(subgraph)
            {
                if leaves { mark.after = true } else { mark.before = true }
            }
        }
    }
    marked
}

impl Unframed<'_> {
    /// The drawing with every box, link and label `main` cells further on along the flow
    /// and `cross` cells further across it. `None` when a distance does not fit an
    /// `isize`.
    fn shifted(mut self, main: usize, cross: usize) -> Option<Self> {
        let (main_by, cross_by) = (signed(main)?, signed(cross)?);
        for placed in &mut self.boxes {
            placed.main += main;
            placed.cross += cross;
        }
        for point in self.links.iter_mut().flat_map(|link| &mut link.points) {
            point.main += main;
            point.cross += cross;
        }
        for label in &mut self.labels {
            label.at.0 += main_by;
            label.at.1 += cross_by;
        }
        Some(self)
    }
}

/// `routed` with every box, link, label and frame `main` cells further on along the flow
/// and `cross` cells further across it. `None` when a distance does not fit an `isize`.
fn shift(routed: Routed<'_>, main: usize, cross: usize) -> Option<Routed<'_>> {
    let Routed { boxes, links, labels, mut frames } = routed;
    let Unframed { boxes, links, labels } =
        Unframed { boxes, links, labels }.shifted(main, cross)?;
    for frame in &mut frames {
        frame.main = frame.main.start() + main..=frame.main.end() + main;
        frame.cross = frame.cross.start() + cross..=frame.cross.end() + cross;
    }
    Some(Routed { boxes, links, labels, frames })
}

fn unsigned(value: isize) -> Option<usize> {
    usize::try_from(value).ok()
}

/// The scene of `routed`, each label at the place the routing fixed for it, and
/// everything moved across the flow (right in a vertical layout, down in a horizontal
/// one) as far as needed for no label to start before the first cell across the flow.
pub(super) fn place_labels(routed: Routed<'_>) -> Option<Scene<'_>> {
    let cross_shift = routed.labels.iter().map(|label| -label.at.1).max().unwrap_or(0).max(0);
    let routed = shift(routed, 0, unsigned(cross_shift)?)?;
    let labels = routed
        .labels
        .iter()
        .map(|&RoutedLabel { link, label, at: (main, cross) }| {
            Some(PlacedLabel { link, label, main: unsigned(main)?, cross: unsigned(cross)? })
        })
        .collect::<Option<_>>()?;
    let Routed { boxes, links, frames, .. } = routed;
    Some(Scene { boxes, links, labels, frames })
}

/// The markers `edge` has at the first and last cells of the gap that `segment` of its
/// laid-out `path` crosses, each 0 or 1: only the first segment starts at a box and only
/// the last ends at one. A reversed link's points run the other way, so its head is
/// drawn at the first gap's first cell and its tail at the last gap's last.
fn gap_end_markers(edge: &Edge, reversed: bool, path: &[usize], segment: usize) -> (usize, usize) {
    let (at_start, at_end) = if reversed { (edge.head, edge.tail) } else { (edge.tail, edge.head) };
    // A path holds one slot more than it has segments.
    let last = segment + 2 == path.len();
    (usize::from(segment == 0 && at_start.is_some()), usize::from(last && at_end.is_some()))
}

/// The first cell of `width` cells centred on the cells `low..=high`, with an odd cell
/// left over on the right. `None` when `width` exceeds `low + high + 1`, where the start
/// would lie before cell 0.
fn centred_start(low: usize, high: usize, width: usize) -> Option<usize> {
    Some((low + high + 1).checked_sub(width)? / 2)
}

/// The run across the flow of a link segment that turns in the gap after its source.
struct Run {
    /// The slot the segment leaves.
    source: usize,
    edge: usize,
    segment: usize,
    /// The cell across the flow the segment leaves.
    leave: usize,
    /// The cells across the flow from the one the segment leaves to the one it enters.
    cells: RangeInclusive<usize>,
    /// Whether the segment enters a later cell across the flow than it leaves.
    rightward: bool,
}

impl Run {
    /// The cell across the flow the segment enters.
    fn entry(&self) -> usize {
        if self.rightward { *self.cells.end() } else { *self.cells.start() }
    }
}

/// Gives each of `runs` a track in the gap after its source's layer. Each source has
/// tracks of its own, two runs share a track only when they have no cell in common, and
/// a run of a source on a track nearer the layer never crosses the line of one of the
/// same source on a further track. The sources take their tracks in [`leg_order`], so
/// the legs of two sources' links meet only where they cross, as upstream's curves
/// cross but never merge, except where two links swap cells, each entering its target
/// by the cell the other leaves its source by: whichever takes the nearer track, its
/// leaving leg runs through the other's entering leg. Returns each segment's track,
/// indexed like the [`EdgeRoute::entries`] of `edges`, and the number of tracks in each gap.
fn assign_tracks(
    layered: &Layered,
    edges: &[EdgeRoute<'_>],
    runs: &[Run],
) -> Option<(Vec<Vec<usize>>, Vec<usize>)> {
    let slots = &layered.slots;
    // Each gap's sources in the order of their first runs, and each gap's and each
    // source's runs in their order in `runs`.
    let mut sources: Vec<Vec<usize>> = vec![Vec::new(); layered.layer_count];
    let mut gap_runs: Vec<Vec<&Run>> = vec![Vec::new(); layered.layer_count];
    let mut by_source: Vec<Vec<&Run>> = vec![Vec::new(); slots.len()];
    for run in runs {
        let layer = slots.get(run.source)?.layer;
        let of_source = by_source.get_mut(run.source)?;
        if of_source.is_empty() {
            sources.get_mut(layer)?.push(run.source);
        }
        of_source.push(run);
        gap_runs.get_mut(layer)?.push(run);
    }
    let mut track_of: Vec<Vec<usize>> =
        edges.iter().map(|edge_route| vec![0; edge_route.entries.len()]).collect();
    let mut track_counts = Vec::with_capacity(layered.layer_count);
    for (sources, gap_runs) in sources.iter_mut().zip(&gap_runs) {
        sources.sort_by_key(|&index| slots.get(index).map(|slot| slot.port));
        let mut first_track = 0;
        for source in leg_order(sources, gap_runs, &by_source, &layered.beside)? {
            // The cells taken on each of this source's tracks.
            let mut tracks: Vec<Vec<&RangeInclusive<usize>>> = Vec::new();
            let mut ordered = by_source.get(source)?.clone();
            ordered.sort_by(|a, b| {
                b.rightward.cmp(&a.rightward).then_with(|| {
                    if a.rightward { b.leave.cmp(&a.leave) } else { a.leave.cmp(&b.leave) }
                })
            });
            for run in ordered {
                let disjoint = |taken: &&RangeInclusive<usize>| {
                    taken.end() < run.cells.start() || run.cells.end() < taken.start()
                };
                let track = match tracks.iter().position(|taken| taken.iter().all(disjoint)) {
                    Some(track) => track,
                    None => {
                        tracks.push(Vec::new());
                        tracks.len() - 1
                    }
                };
                tracks.get_mut(track)?.push(&run.cells);
                *track_of.get_mut(run.edge)?.get_mut(run.segment)? = first_track + track;
            }
            first_track += tracks.len();
        }
        track_counts.push(first_track);
    }
    Some((track_of, track_counts))
}

/// `sources`, in port order, reordered so that each takes its tracks before every other
/// source with a run entering the cell one of its runs leaves by. Later sources get
/// tracks further from the layer, so the entering leg, from its track on to the next
/// layer, then starts past the end of the leaving leg, which runs from the layer to its
/// track; the other way round the two would share the cells between the tracks. The run
/// of a link laid out into a frame it meets across the flow (see [`on_run`]) makes its
/// source wait on none: its line ends on its track, so it has no entering leg. When
/// every source left waits on another, the first in port order of a ring they wait on
/// goes first (see [`first_in_ring`]): a source outside the ring would only add a shared
/// leg to the one the ring forces. `gap_runs` are the runs of the gap, and `by_source`
/// gives each slot's runs, both in the order of the runs.
fn leg_order(
    sources: &[usize],
    gap_runs: &[&Run],
    by_source: &[Vec<&Run>],
    beside: &[Option<LaidOutEnd>],
) -> Option<Vec<usize>> {
    let mut waiting: Vec<Option<(usize, Vec<usize>)>> = sources
        .iter()
        .map(|&source| {
            let entered: Vec<usize> = by_source
                .get(source)?
                .iter()
                .filter(|run| !on_run(beside, run.edge))
                .map(|run| run.entry())
                .collect();
            let leaving = gap_runs
                .iter()
                .filter(|run| run.source != source && entered.contains(&run.leave))
                .map(|run| run.source)
                .collect();
            Some(Some((source, leaving)))
        })
        .collect::<Option<_>>()?;
    // Indexed by slot, like `by_source`.
    let mut is_ordered = vec![false; by_source.len()];
    let mut ordered = Vec::with_capacity(sources.len());
    while ordered.len() < sources.len() {
        let ready = waiting.iter().position(|entry| {
            entry.as_ref().is_some_and(|(_, after)| {
                after.iter().all(|&source| is_ordered.get(source) == Some(&true))
            })
        });
        let next = match ready {
            Some(next) => next,
            None => first_in_ring(&waiting, &is_ordered)?,
        };
        let (source, _) = waiting.get_mut(next)?.take()?;
        *is_ordered.get_mut(source)? = true;
        ordered.push(source);
    }
    Some(ordered)
}

/// The index in `waiting`, which lists each source in port order with the sources it
/// waits on, `None` once it is ordered, of the first source of a ring among those left:
/// following from the first source left the first source each waits on that is not yet
/// ordered, until one comes round again. `is_ordered` tells, by slot, the sources
/// ordered. `None` when a source on the way waits on none left; [`leg_order`] calls it
/// only once every source left waits on another.
fn first_in_ring(waiting: &[Option<(usize, Vec<usize>)>], is_ordered: &[bool]) -> Option<usize> {
    let mut path = vec![waiting.iter().position(Option::is_some)?];
    for _ in 0..waiting.len() {
        let (_, after) = waiting.get(*path.last()?)?.as_ref()?;
        let next = after.iter().find(|&&source| is_ordered.get(source) != Some(&true))?;
        let next = waiting
            .iter()
            .position(|entry| entry.as_ref().is_some_and(|(source, _)| source == next))?;
        if let Some(start) = path.iter().position(|&seen| seen == next) {
            return path.get(start..)?.iter().min().copied();
        }
        path.push(next);
    }
    None
}

/// The index and path of each edge that is drawn: an invisible link takes no cell at
/// its ends.
fn visible_paths<'a>(
    chart: &'a Flowchart,
    layered: &'a Layered,
) -> impl Iterator<Item = (usize, &'a [usize])> {
    chart
        .edges
        .iter()
        .zip(layered.paths.iter().enumerate())
        .filter(|(edge, _)| edge.stroke.is_visible())
        .map(|(_, (index, path))| (index, path.as_slice()))
}

/// For each edge, the cell across the flow where each of its segments enters the next
/// slot on its path, indexed like [`Layered::paths`] without their first slot.
fn entry_ports(
    chart: &Flowchart,
    growth: &Growth,
    layered: &Layered,
    axis: Axis,
) -> Option<Vec<Vec<usize>>> {
    let slots = &layered.slots;
    let mut arrivals = vec![Vec::new(); slots.len()];
    for (edge, path) in visible_paths(chart, layered) {
        let edge_data = chart.edges.get(edge)?;
        for (segment, pair) in path.windows(2).enumerate() {
            if let &[from, to] = pair {
                let reach = entry_reach(axis, edge_data, path, segment);
                arrivals.get_mut(to)?.push((slots.get(from)?.port, edge, segment, reach));
            }
        }
    }
    let mut entries: Vec<Vec<usize>> =
        layered.paths.iter().map(|path| vec![0; path.len().saturating_sub(1)]).collect();
    for (slot_index, arrivals) in arrivals.iter().enumerate() {
        let slot = slots.get(slot_index)?;
        let port = slot.port.checked_sub(slot.cross)?;
        // A passing slot is entered at its port alone, even when a label makes it wider.
        let cells = chart
            .nodes
            .get(slot_index)
            .zip(growth.boxes.get(slot_index))
            .map_or(port..=port, |(node, &spread)| axis.port_range(node, spread));
        // Arrivals are listed by edge, so equal sources keep the order of their edges.
        let sources: Vec<usize> = arrivals.iter().map(|&(port, ..)| port).collect();
        let reach: Vec<Option<Reach>> = arrivals.iter().map(|&(.., reach)| reach).collect();
        let cells = port_cells(axis, &sources, &reach, &cells, port)?;
        for (&(_, edge, segment, _), cell) in arrivals.iter().zip(cells) {
            *entries.get_mut(edge)?.get_mut(segment)? = slot.cross + cell;
        }
    }
    Some(entries)
}

/// For each edge, the cell across the flow where it leaves the first slot of its path;
/// for a self loop, the cell of its first leg (see [`exit_cells`]).
fn exit_ports(
    chart: &Flowchart,
    growth: &Growth,
    layered: &Layered,
    axis: Axis,
) -> Option<Vec<usize>> {
    let slots = &layered.slots;
    // For each node, the links to other slots, with the rank of the slot each goes to
    // (see `Slot::order`), and the self loops.
    let mut leaving = vec![Vec::new(); chart.nodes.len()];
    let mut loops = vec![Vec::new(); chart.nodes.len()];
    let mut exits = vec![0; layered.paths.len()];
    for (edge, path) in visible_paths(chart, layered) {
        // A subgraph's self loop leaves no slot.
        let Some(&first) = path.first() else { continue };
        let listed = match path.get(1) {
            Some(&next) => {
                let order = slots.get(next)?.order;
                leaving.get_mut(first).map(|leaving| leaving.push((order, edge)))
            }
            None => loops.get_mut(first).map(|loops| loops.push(edge)),
        };
        // A link leaving a frame starts at its single cell on the frame's border.
        if listed.is_none() {
            *exits.get_mut(edge)? = slots.get(first)?.port;
        }
    }
    let nodes = chart.nodes.iter().zip(&growth.boxes);
    for ((((node, &box_spread), slot), leaving), loops) in
        nodes.zip(slots).zip(&leaving).zip(&loops)
    {
        let targets: Vec<usize> = leaving.iter().map(|&(order, _)| order).collect();
        let labels = loops
            .iter()
            .map(|&edge| Some(chart.edges.get(edge)?.label.as_ref()))
            .collect::<Option<Vec<_>>>()?;
        let port = slot.port.checked_sub(slot.cross)?;
        let spread =
            exit_cells(axis, targets.len(), &labels, &axis.port_range(node, box_spread), port)?;
        let target_cells = scatter_by_order(&targets, &spread.targets)?;
        let edges = leaving.iter().map(|&(_, edge)| edge).chain(loops.iter().copied());
        for (edge, cell) in edges.zip(target_cells.into_iter().chain(spread.legs)) {
            *exits.get_mut(edge)? = slot.cross + cell;
        }
    }
    Some(exits)
}

/// A loop that leaves the box's far border along the flow at `port`, runs on through
/// the [`SELF_LOOP_CELLS`] cells beyond that border, and re-enters the border
/// [`SELF_LOOP_SPAN`] cells further across the flow, as Mermaid draws a self loop as a
/// rectangle on the side its node's links leave by. `out` is the first cell beyond that
/// border, and the legs start `inset` cells before it (see [`side_inset`]).
fn self_loop(out: usize, inset: usize, port: usize) -> Vec<Cell> {
    let far = out + SELF_LOOP_CELLS - 1;
    let back = port + SELF_LOOP_SPAN;
    let side = out.saturating_sub(inset);
    vec![
        Cell { main: side, cross: port },
        Cell { main: far, cross: port },
        Cell { main: far, cross: back },
        Cell { main: side, cross: back },
    ]
}

/// Where `label`, on a self loop whose first leg is at `port` across the flow, starts
/// along and across the flow, given `out`, the first cell beyond the border the loop
/// leaves (see [`self_loop_label_at`]).
fn self_loop_label(axis: Axis, out: usize, port: usize, label: &Label) -> Option<(isize, isize)> {
    let (along, cross) = self_loop_label_at(axis, port, label)?;
    Some((signed(out + along)?, cross))
}

/// Cells from `node`'s bounding edge to its side glyph on the label rows, on the side
/// links leave it by when `leaving` and on the one they enter it by otherwise: a link
/// along a horizontal flow runs on through them, so that it meets the side as upstream's
/// meets the shape's outline. 0 along a vertical flow, whose ports lie between the
/// border's ends (see [`Axis::port_range`]), and for a body that is not a box.
fn side_inset(direction: Direction, node: &Node, leaving: bool) -> usize {
    match (&node.body, direction.axis()) {
        (Body::Box { shape, .. }, Axis::Horizontal) => {
            let right_side = leaving != direction.points_backward();
            outline(*shape).side_inset(right_side)
        }
        _ => 0,
    }
}
