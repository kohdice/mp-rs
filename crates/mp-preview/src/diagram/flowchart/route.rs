//! Positions along the flow, the paths of links between the placed boxes, and where
//! link labels go. Positions are `(main, cross)`: cells along and across the flow.

mod frames;

use std::ops::RangeInclusive;

use super::label::Label;
use super::layout::{
    Axis, LabelSpot, Layered, Reach, SELF_LOOP_CELLS, SELF_LOOP_SPAN, entry_reach, exit_cells,
    label_cross_reach, label_spot, port_cells, scatter_by_order, self_loop_cells,
    self_loop_label_at,
};
use super::outline::outline;
use super::parse::{Body, Direction, Edge, End, Flowchart, Node};
use super::signed;
use super::styling::Styling;

use self::frames::{FrameBounds, frame_bounds, frames_overlap};

/// A drawing before it is oriented on screen.
#[derive(Debug)]
pub(super) struct Scene<'a> {
    pub boxes: Vec<PlacedBox<'a>>,
    /// The drawn links, in the order of their edges; an invisible link has none.
    pub links: Vec<Route<'a>>,
    /// Empty as [`route`] returns the scene; the caller fills it from the routed labels
    /// (see [`place_labels`]).
    pub labels: Vec<PlacedLabel<'a>>,
    pub frames: Vec<PlacedFrame<'a>>,
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

/// A box with its first cell along and across the flow.
#[derive(Debug)]
pub(super) struct PlacedBox<'a> {
    pub node: &'a Node,
    pub main: usize,
    pub cross: usize,
}

#[derive(Debug)]
pub(super) struct Route<'a> {
    pub edge: &'a Edge,
    /// Corners of the line, from the cell next to the source's border to the cell next
    /// to the target's border.
    pub points: Vec<(usize, usize)>,
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
        let boxes = self.boxes.iter().map(|placed| {
            (
                placed.main + axis.box_main_size(placed.node),
                placed.cross + axis.box_cross_size(placed.node),
            )
        });
        let links = self.links.iter().flat_map(|link| &link.points);
        let labels = self.labels.iter().map(|label| {
            let (main_size, cross_size) = axis.label_size(label.label);
            (label.main + main_size, label.cross + cross_size)
        });
        let frames = self.frames.iter().map(|frame| (frame.main.end() + 1, frame.cross.end() + 1));
        boxes
            .chain(links.map(|&(main, cross)| (main + 1, cross + 1)))
            .chain(labels)
            .chain(frames)
            .fold((0, 0), |(main, cross), (end_main, end_cross)| {
                (main.max(end_main), cross.max(end_cross))
            })
    }
}

/// Spaces the layers along the flow, `layer_gap` apart unless tracks or labels need
/// more, and routes each edge from its source's port to its target. The label of every
/// link but a self loop gets its place on its line at the middle of its span (see
/// [`LabelSpot`]), returned as a [`RoutedLabel`]: in a gap it has room of its own after
/// the tracks, a row in a vertical layout and cells along the line in a horizontal one;
/// on a passing slot the layout sized the slot both along the flow and across it. A self
/// loop is a bump on its box's far border along the flow, after the box's other exits
/// (see [`exit_cells`]); its label lies outside the loop beyond its run (see
/// [`self_loop_label_at`]), in cells after the box that the tracks and the next layer
/// start beyond, and that the layout keeps clear of the box's siblings across the flow.
/// `None` when a subgraph frame would cover a box outside its subgraph or a frame
/// neither nested in it nor enclosing it.
///
/// Several links entering one box get their own entry cells along its border, in the
/// order of the cells they come from. Lines run along rows and columns only, as
/// box-drawing glyphs have no cells for the curves upstream draws; segments turn on
/// tracks in the gaps between layers (see [`assign_tracks`]), and a link closing a cycle
/// follows its reversed path (see [`lay_out`](super::layout::lay_out)). Frames are placed
/// by [`frame_bounds`].
pub(super) fn route<'a>(
    chart: &'a Flowchart,
    layered: &'a Layered,
    axis: Axis,
    layer_gap: usize,
) -> Option<(Scene<'a>, Vec<RoutedLabel<'a>>)> {
    let router = Router::new(chart, layered, axis, layer_gap)?;
    let slots = &layered.slots;
    let paths = &layered.paths;
    let place = |layer_start: &[usize],
                 exit: &[usize]|
     -> Option<(Scene<'a>, Vec<RoutedLabel<'a>>)> {
        let gap_start =
            |layer: usize| Some(layer_start.get(layer)? + router.layer_size.get(layer)?);
        let mut scene =
            Scene { boxes: Vec::new(), links: Vec::new(), labels: Vec::new(), frames: Vec::new() };
        let mut routed = Vec::new();
        for (node, slot) in chart.nodes.iter().zip(slots) {
            scene.boxes.push(PlacedBox {
                node,
                main: *layer_start.get(slot.layer)?,
                cross: slot.cross,
            });
        }
        for (index, ((((edge, path), entries), &reversed), &exit_cell)) in chart
            .edges
            .iter()
            .zip(paths)
            .zip(&router.entries)
            .zip(&layered.reversed)
            .zip(&router.exits)
            .enumerate()
        {
            if !edge.stroke.is_visible() {
                continue;
            }
            let (&first, &last) = (path.first()?, path.last()?);
            let first_slot = slots.get(first)?;
            if first == last {
                let node = chart.nodes.get(first)?;
                let main = *layer_start.get(first_slot.layer)?;
                if let Some(text) = &edge.label {
                    let (along, cross) = self_loop_label_at(axis, exit_cell, text)?;
                    let out = main + axis.box_main_size(node);
                    let at = (signed(out + along)?, cross);
                    routed.push(RoutedLabel { link: scene.links.len(), label: text, at });
                }
                let inset = side_inset(chart.direction, node, true);
                let points = self_loop(axis, node, main, exit_cell, inset);
                scene.links.push(Route { edge, points });
                continue;
            }
            let out = match chart.nodes.get(first) {
                Some(node) => (layer_start.get(first_slot.layer)? + axis.box_main_size(node))
                    .checked_sub(side_inset(chart.direction, node, true))?,
                None => gap_start(first_slot.layer)?,
            };
            let mut points = vec![(out, exit_cell)];
            for (segment, (&from_index, &entry)) in path.iter().zip(entries).enumerate() {
                let from = slots.get(from_index)?;
                let leave = if segment == 0 { exit_cell } else { from.port };
                if leave != entry {
                    let track = gap_start(from.layer)?
                        + exit.get(from.layer)?
                        + 1
                        + router.loop_cells.get(from.layer)?
                        + router.track_of.get(index)?.get(segment)?;
                    points.extend([(track, leave), (track, entry)]);
                }
            }
            if let Some(text) = &edge.label
                && let Some(spot) = label_spot(path)
            {
                let width = text.width();
                let (before_line, _) = label_cross_reach(axis, text);
                let at = match spot {
                    // After the tracks the line is already at the cell it enters the next
                    // layer by. A vertical label's rows sit on the rows after the tracks,
                    // centred on that cell; a horizontal one runs its middle row along the
                    // line on that cell's row, centred between the markers at the gap's
                    // ends with a line cell on either side of the text, as Mermaid
                    // centres a label between a link's ends. Upstream puts the label at its
                    // edge-label dummy node on the middle rank (`edge.x`/`edge.y`, see
                    // `positionEdgeLabel`), placed across the flow like any node, and the
                    // curve bends through it; an orthogonal line turns on a track, and a
                    // label on the track would cover the turn, so the label goes on the
                    // line after the tracks, at the cell the line enters the next layer by.
                    // For a straight link the two coincide.
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
                routed.push(RoutedLabel { link: scene.links.len(), label: text, at });
            }
            let entry = *entries.last()?;
            let into = layer_start.get(slots.get(last)?.layer)?.checked_sub(1)?
                + chart.nodes.get(last).map_or(0, |node| side_inset(chart.direction, node, false));
            points.push((into, entry));
            if reversed {
                points.reverse();
            }
            scene.links.push(Route { edge, points });
        }
        Some((scene, routed))
    };

    // The margins a frame needs past its boxes do not depend on where the layers start,
    // and a title widens it less once the layers are spaced out, so a first placement
    // without room for frames measures an upper bound on the room to make.
    let unframed = vec![0; layered.layer_count];
    let unframed_start = router.layer_starts(&unframed, &unframed)?;
    let (mut enter, mut exit) =
        router.margins(&unframed_start, &place(&unframed_start, &unframed)?)?;
    let mut layer_start = router.layer_starts(&enter, &exit)?;
    let mut placed = place(&layer_start, &exit)?;
    // In a horizontal layout the title runs along the flow, and the room made moves the
    // tracks its links cross the top border on, so the title may need more room than
    // was made. The layers are spaced out again only once: the overlap check below
    // accepts or rejects the result, so no further round is needed.
    if axis == Axis::Horizontal {
        let (needed_enter, needed_exit) = router.margins(&layer_start, &placed)?;
        let short = |needed: &[usize], made: &[usize]| needed.iter().zip(made).any(|(n, m)| n > m);
        if short(&needed_enter, &enter) || short(&needed_exit, &exit) {
            let wider = |needed: &[usize], made: &[usize]| {
                needed.iter().zip(made).map(|(&n, &m)| n.max(m)).collect::<Vec<_>>()
            };
            (enter, exit) = (wider(&needed_enter, &enter), wider(&needed_exit, &exit));
            layer_start = router.layer_starts(&enter, &exit)?;
            placed = place(&layer_start, &exit)?;
        }
    }
    // The room left for a title clear of the links crossing its frame's top border
    // was worked out before the final placement; when that room falls short and a frame
    // covers another box or frame, every title stays at its corner, over the links,
    // rather than the drawing being given up.
    let (scene, routed) = placed;
    let mut framed = router.frames(&scene, &routed, true);
    if frames_overlap(chart, &framed, &scene, axis) {
        framed = router.frames(&scene, &routed, false);
        if frames_overlap(chart, &framed, &scene, axis) {
            return None;
        }
    }
    // Frames reaching past the first cells along or across the flow move everything
    // else on by as much.
    let (main_shift, cross_shift) = framed.iter().fold((0, 0), |(main, cross), (.., frame)| {
        (main.max(-frame.main.0), cross.max(-frame.cross.0))
    });
    let (main_by, cross_by) = (unsigned(main_shift)?, unsigned(cross_shift)?);
    let routed = shift_labels(routed, main_by, cross_by)?;
    let mut scene = shift_scene(scene, main_by, cross_by);
    let moved =
        |(low, high): (isize, isize), by: isize| Some(unsigned(low + by)?..=unsigned(high + by)?);
    let mut frame_of = vec![None; chart.subgraphs.len()];
    for (index, _, frame) in framed {
        let main = moved(frame.main, main_shift)?;
        let cross = moved(frame.cross, cross_shift)?;
        *frame_of.get_mut(index)? = Some(main.clone());
        let styling = chart.subgraphs.get(index)?.styling;
        scene.frames.push(PlacedFrame {
            styling,
            title: frame.title,
            title_offset: frame.title_offset,
            main,
            cross,
        });
    }
    // A link to or from a subgraph was routed to a cell among its members; it ends in
    // the cell just outside the frame's border instead, where its marker goes, as at a
    // box: before the frame along the flow where the link is laid out into it, after it
    // where the link is laid out from it. A reversed link is laid out from its target.
    let drawn_reversed = chart
        .edges
        .iter()
        .zip(&layered.reversed)
        .filter(|(edge, _)| edge.stroke.is_visible())
        .map(|(_, reversed)| reversed);
    for (link, &reversed) in scene.links.iter_mut().zip(drawn_reversed) {
        let (written_from, written_to) = (link.edge.from, link.edge.to);
        let (first, last) = (link.points.first().copied()?, link.points.last().copied()?);
        let ends = [(written_from, !reversed, first), (written_to, reversed, last)];
        let [from, to] = ends.map(|(end, leaves, (main, cross))| match end {
            End::Subgraph(subgraph) => {
                let frame = frame_of.get(subgraph)?.as_ref()?;
                let main = if leaves { frame.end() + 1 } else { frame.start().checked_sub(1)? };
                Some((main, cross))
            }
            End::Node(_) => Some((main, cross)),
        });
        *link.points.first_mut()? = from?;
        *link.points.last_mut()? = to?;
    }
    Some((scene, routed))
}

/// The spacing of the layers and the frames around the subgraphs, shared by the rounds
/// of [`route`] that space the layers out again for the frames.
struct Router<'a> {
    chart: &'a Flowchart,
    layered: &'a Layered,
    entries: Vec<Vec<usize>>,
    exits: Vec<usize>,
    track_of: Vec<Vec<usize>>,
    layer_size: Vec<usize>,
    loop_cells: Vec<usize>,
    leads: Vec<usize>,
    gaps: Vec<usize>,
    memberships: Vec<Vec<bool>>,
    /// Inner frames first, so that each outer one is drawn around them.
    innermost_first: Vec<usize>,
    /// Whether a drawn link's marker lies just before each subgraph's frame along the
    /// flow, and just after it: [`route`] ends a link to or from a subgraph in the cell
    /// just outside the frame's border, where its marker goes.
    marked: Vec<(bool, bool)>,
}

impl<'a> Router<'a> {
    /// Assigns the ports and tracks of `layered` and sizes its layers and gaps, each gap
    /// at least `layer_gap` cells.
    fn new(
        chart: &'a Flowchart,
        layered: &'a Layered,
        axis: Axis,
        layer_gap: usize,
    ) -> Option<Self> {
        let slots = &layered.slots;
        let paths = &layered.paths;
        let mut layer_size = vec![0; layered.layer_count];
        for slot in slots {
            let size = layer_size.get_mut(slot.layer)?;
            *size = (*size).max(slot.main);
        }
        let entries = entry_ports(chart, layered, axis)?;
        let exits = exit_ports(chart, layered, axis)?;

        let mut runs = Vec::new();
        // For each gap, the cells the routed labels on its lines need after the tracks: in a
        // horizontal layout a label's widest row and the markers at the gap's ends, in a
        // vertical one a label's rows.
        let mut routed_label_cells: Vec<Option<usize>> = vec![None; layered.layer_count];
        for (index, ((((edge, path), entries), &reversed), &exit)) in chart
            .edges
            .iter()
            .zip(paths)
            .zip(&entries)
            .zip(&layered.reversed)
            .zip(&exits)
            .enumerate()
        {
            for (segment, (&from, &entry)) in path.iter().zip(entries).enumerate() {
                let leave = if segment == 0 { exit } else { slots.get(from)?.port };
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
            // A label on a passing slot lies in its layer, which `Slot::main` sized for it; a
            // self loop's label is counted in `loop_cells` below, not here.
            if let Some(label) = &edge.label
                && let Some(LabelSpot::Gap(segment)) = label_spot(path)
            {
                let cells = routed_label_cells.get_mut(slots.get(*path.get(segment)?)?.layer)?;
                let needed = match axis {
                    Axis::Horizontal => {
                        let (before, after) = gap_end_markers(edge, reversed, path, segment);
                        before + label.width() + after
                    }
                    Axis::Vertical => label.height(),
                };
                *cells = Some(cells.unwrap_or(0).max(needed));
            }
        }
        let (track_of, track_counts) = assign_tracks(layered, &entries, &runs)?;
        // A self loop and its label take the cells after its box along the flow, so the
        // tracks after its layer, and the next layer, start beyond them.
        let mut loop_cells = vec![0; layered.layer_count];
        for edge in chart.edges.iter().filter(|edge| edge.stroke.is_visible()) {
            if let Some((node, to)) = edge.nodes()
                && node == to
            {
                let cells = loop_cells.get_mut(slots.get(node)?.layer)?;
                *cells = (*cells).max(self_loop_cells(axis, edge.label.as_ref()));
            }
        }

        // The cells of each gap before the cell after its last track: the cell the line
        // leaves the box through, the cells of self loops and the tracks. A vertical label
        // row comes after those even without tracks. A horizontal gap without tracks has a
        // lead of 0: a straight line's first cell, where the line leaves the box, is the
        // line cell before its label, or holds the link's marker there with that line cell
        // after it (a back edge's head, a `<-->` tail; see `gap_end_markers`).
        let leads: Vec<usize> = track_counts
            .iter()
            .zip(&loop_cells)
            .zip(&routed_label_cells)
            .map(|((&tracks, &loop_cells), routed_cells)| {
                let row_lead = axis == Axis::Vertical && routed_cells.is_some();
                if tracks == 0 && !row_lead { 0 } else { 1 + loop_cells + tracks }
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
        let memberships = chart.memberships()?;
        let innermost_first = chart.innermost_first();
        let mut marked = vec![(false, false); chart.subgraphs.len()];
        for (edge, &reversed) in chart.edges.iter().zip(&layered.reversed) {
            if !edge.stroke.is_visible() {
                continue;
            }
            let ends = [
                (edge.from, !reversed, edge.tail.is_some()),
                (edge.to, reversed, edge.head.is_some()),
            ];
            for (end, leaves, marker) in ends {
                if let End::Subgraph(subgraph) = end
                    && marker
                    && let Some((before, after)) = marked.get_mut(subgraph)
                {
                    if leaves { *after = true } else { *before = true }
                }
            }
        }
        Some(Self {
            chart,
            layered,
            entries,
            exits,
            track_of,
            layer_size,
            loop_cells,
            leads,
            gaps,
            memberships,
            innermost_first,
            marked,
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

    /// The frames of the subgraphs that get one in `scene`, by subgraph, with their
    /// membership tables; see [`frame_bounds`] for `clear_links`.
    fn frames(
        &self,
        scene: &Scene<'_>,
        routed: &[RoutedLabel<'_>],
        clear_links: bool,
    ) -> Vec<(usize, &[bool], FrameBounds<'a>)> {
        let chart = self.chart;
        let mut bounds: Vec<Option<FrameBounds<'a>>> =
            chart.subgraphs.iter().map(|_| None).collect();
        for &index in &self.innermost_first {
            let (Some(subgraph), Some(is_member)) =
                (chart.subgraphs.get(index), self.memberships.get(index))
            else {
                continue;
            };
            let inner: Vec<_> = chart
                .subgraphs
                .iter()
                .zip(&bounds)
                .zip(&self.marked)
                .filter(|((inner, _), _)| inner.parent == Some(index))
                .filter_map(|((_, inner), &marked)| {
                    inner.as_ref().map(|inner| ((inner.main, inner.cross), marked))
                })
                .collect();
            if let Some(slot) = bounds.get_mut(index) {
                *slot = frame_bounds(
                    (subgraph, &chart.enclosing(index).collect::<Vec<_>>()),
                    is_member,
                    &inner,
                    scene,
                    routed,
                    chart.direction,
                    clear_links,
                );
            }
        }
        bounds
            .into_iter()
            .zip(&self.memberships)
            .enumerate()
            .filter_map(|(index, (frame, is_member))| Some((index, is_member.as_slice(), frame?)))
            .collect::<Vec<_>>()
    }

    /// The cells each layer's frames take before and after its boxes in `scene`, laid
    /// out from `layer_start`.
    fn margins(
        &self,
        layer_start: &[usize],
        (scene, routed): &(Scene<'_>, Vec<RoutedLabel<'_>>),
    ) -> Option<(Vec<usize>, Vec<usize>)> {
        let layered = self.layered;
        let mut enter = vec![0; layered.layer_count];
        let mut exit = vec![0; layered.layer_count];
        for (index, _, frame) in self.frames(scene, routed, true) {
            let &(first, last) = layered.member_layers.get(index)?;
            let before = signed(*layer_start.get(first)?)? - frame.main.0;
            let after =
                frame.main.1 + 1 - signed(layer_start.get(last)? + self.layer_size.get(last)?)?;
            let enter = enter.get_mut(first)?;
            *enter = (*enter).max(unsigned(before.max(0))?);
            let exit = exit.get_mut(last)?;
            *exit = (*exit).max(unsigned(after.max(0))?);
        }
        Some((enter, exit))
    }
}

/// The `routed` labels, each `main` cells further on along the flow and `cross` cells
/// further across it. `None` when a distance does not fit an `isize`.
fn shift_labels<'a>(
    mut routed: Vec<RoutedLabel<'a>>,
    main: usize,
    cross: usize,
) -> Option<Vec<RoutedLabel<'a>>> {
    let (main_by, cross_by) = (signed(main)?, signed(cross)?);
    for label in &mut routed {
        label.at.0 += main_by;
        label.at.1 += cross_by;
    }
    Some(routed)
}

/// `scene` with every box, link and frame `main` cells further on along the flow and
/// `cross` cells further across it.
pub(super) fn shift_scene(mut scene: Scene<'_>, main: usize, cross: usize) -> Scene<'_> {
    for placed in &mut scene.boxes {
        placed.main += main;
        placed.cross += cross;
    }
    for point in scene.links.iter_mut().flat_map(|link| &mut link.points) {
        point.0 += main;
        point.1 += cross;
    }
    for frame in &mut scene.frames {
        frame.main = frame.main.start() + main..=frame.main.end() + main;
        frame.cross = frame.cross.start() + cross..=frame.cross.end() + cross;
    }
    scene
}

fn unsigned(value: isize) -> Option<usize> {
    usize::try_from(value).ok()
}

/// The `routed` labels, each at the place the routing fixed for it, and the cells
/// everything else must move across the flow by (right in a vertical layout, down in a
/// horizontal one) so that no label starts before the first cell across the flow.
pub(super) fn place_labels<'a>(routed: &[RoutedLabel<'a>]) -> (Vec<PlacedLabel<'a>>, usize) {
    let cross_shift = routed.iter().map(|label| -label.at.1).max().unwrap_or(0).max(0);
    let labels = routed
        .iter()
        .filter_map(|&RoutedLabel { link, label, at: (main, cross) }| {
            Some(PlacedLabel {
                link,
                label,
                main: unsigned(main)?,
                cross: unsigned(cross + cross_shift)?,
            })
        })
        .collect();
    (labels, unsigned(cross_shift).unwrap_or(0))
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

/// Gives each of `runs` a track in the gap after its source's layer. Each source has
/// tracks of its own, two runs share a track only when they have no cell in common, and
/// a run of a source on a track nearer the layer never crosses the line of one of the
/// same source on a further track. Returns each segment's track, indexed like
/// `entries`, and the number of tracks in each gap.
fn assign_tracks(
    layered: &Layered,
    entries: &[Vec<usize>],
    runs: &[Run],
) -> Option<(Vec<Vec<usize>>, Vec<usize>)> {
    let slots = &layered.slots;
    let mut sources: Vec<Vec<usize>> = vec![Vec::new(); layered.layer_count];
    for run in runs {
        let layer = sources.get_mut(slots.get(run.source)?.layer)?;
        if !layer.contains(&run.source) {
            layer.push(run.source);
        }
    }
    let mut track_of: Vec<Vec<usize>> = entries.iter().map(|cells| vec![0; cells.len()]).collect();
    let mut track_counts = Vec::with_capacity(layered.layer_count);
    for sources in &mut sources {
        sources.sort_by_key(|&index| slots.get(index).map(|slot| slot.port));
        let mut first_track = 0;
        for &source in sources.iter() {
            // The cells taken on each of this source's tracks.
            let mut tracks: Vec<Vec<&RangeInclusive<usize>>> = Vec::new();
            let mut ordered: Vec<&Run> = runs.iter().filter(|run| run.source == source).collect();
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
fn entry_ports(chart: &Flowchart, layered: &Layered, axis: Axis) -> Option<Vec<Vec<usize>>> {
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
        let cells = chart.nodes.get(slot_index).map_or(port..=port, |node| axis.port_range(node));
        // Arrivals are listed by edge, so equal sources keep the order of their edges.
        let sources: Vec<usize> = arrivals.iter().map(|&(port, ..)| port).collect();
        let reach: Vec<Reach> = arrivals.iter().map(|&(.., reach)| reach).collect();
        let cells = port_cells(axis, &sources, &reach, &cells, port)?;
        for (&(_, edge, segment, _), cell) in arrivals.iter().zip(cells) {
            *entries.get_mut(edge)?.get_mut(segment)? = slot.cross + cell;
        }
    }
    Some(entries)
}

/// For each edge, the cell across the flow where it leaves the first slot of its path;
/// for a self loop, the cell of its first leg (see [`exit_cells`]).
fn exit_ports(chart: &Flowchart, layered: &Layered, axis: Axis) -> Option<Vec<usize>> {
    let slots = &layered.slots;
    // For each node, the links to other slots, with the rank of the slot each goes to
    // (see `Slot::order`), and the self loops.
    let mut leaving = vec![Vec::new(); chart.nodes.len()];
    let mut loops = vec![Vec::new(); chart.nodes.len()];
    let mut exits = vec![0; layered.paths.len()];
    for (edge, path) in visible_paths(chart, layered) {
        let first = *path.first()?;
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
    for (((node, slot), leaving), loops) in chart.nodes.iter().zip(slots).zip(&leaving).zip(&loops)
    {
        let targets: Vec<usize> = leaving.iter().map(|&(order, _)| order).collect();
        let labels = loops
            .iter()
            .map(|&edge| Some(chart.edges.get(edge)?.label.as_ref()))
            .collect::<Option<Vec<_>>>()?;
        let port = slot.port.checked_sub(slot.cross)?;
        let spread = exit_cells(axis, targets.len(), &labels, &axis.port_range(node), port)?;
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
/// rectangle on the side its node's links leave by. Its legs start `inset` cells inside
/// that border (see [`side_inset`]).
fn self_loop(
    axis: Axis,
    node: &Node,
    main: usize,
    port: usize,
    inset: usize,
) -> Vec<(usize, usize)> {
    let out = main + axis.box_main_size(node);
    let far = out + SELF_LOOP_CELLS - 1;
    let back = port + SELF_LOOP_SPAN;
    let side = out.saturating_sub(inset);
    vec![(side, port), (far, port), (far, back), (side, back)]
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
