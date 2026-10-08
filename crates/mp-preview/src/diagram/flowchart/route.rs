//! Positions along the flow, the paths of links between the placed boxes, and where
//! link labels go. Positions are `(main, cross)`: cells along and across the flow.

use std::ops::RangeInclusive;

use super::label::Label;
use super::layout::{
    Axis, LabelSpot, Layered, Reach, SELF_LOOP_CELLS, SELF_LOOP_SPAN, TITLE_CORNER_OFFSET,
    entry_reach, exit_cells, frame_width, label_cross_reach, label_spot, port_cells,
    scatter_by_order, self_loop_cells, self_loop_label_at, title_offset,
};
use super::outline::outline;
use super::parse::{Body, Direction, Edge, End, Flowchart, Node, Subgraph};
use super::signed;
use super::styling::Styling;

/// A drawing before it is oriented on screen.
#[derive(Debug)]
pub(super) struct Scene<'a> {
    pub boxes: Vec<PlacedBox<'a>>,
    /// The drawn links, in the order of their edges; an invisible link has none.
    pub links: Vec<Route<'a>>,
    pub labels: Vec<PlacedLabel<'a>>,
    pub frames: Vec<PlacedFrame<'a>>,
}

/// A label whose place the routing fixed: on its link's line, or outside a self loop.
#[derive(Debug)]
pub(super) struct RoutedLabel<'a> {
    /// The index of the label's link in [`Scene::links`].
    pub link: usize,
    pub label: &'a Label,
    /// Its first cell along and across the flow, which may lie before the first column.
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
    layered: &Layered,
    axis: Axis,
    layer_gap: usize,
) -> Option<(Scene<'a>, Vec<RoutedLabel<'a>>)> {
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
    for (index, ((((edge, path), entries), &reversed), &exit)) in
        chart.edges.iter().zip(paths).zip(&entries).zip(&layered.reversed).zip(&exits).enumerate()
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
    // Each layer is preceded by the cells the frames opening at it take before its
    // boxes, and its gap begins with the cells the frames closing at it take after
    // them, so that tracks, labels and the previous layer stay clear of the frames.
    let layer_starts = |enter: &[usize], exit: &[usize]| -> Option<Vec<usize>> {
        let mut layer_start = Vec::with_capacity(layered.layer_count);
        let mut next = 0;
        for (layer, (size, gap)) in layer_size.iter().zip(&gaps).enumerate() {
            next += enter.get(layer)?;
            layer_start.push(next);
            next += size + exit.get(layer)? + gap;
        }
        Some(layer_start)
    };
    let place = |layer_start: &[usize],
                 exit: &[usize]|
     -> Option<(Scene<'a>, Vec<RoutedLabel<'a>>)> {
        let gap_start = |layer: usize| Some(layer_start.get(layer)? + layer_size.get(layer)?);
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
            .zip(&entries)
            .zip(&layered.reversed)
            .zip(&exits)
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
                        + loop_cells.get(from.layer)?
                        + track_of.get(index)?.get(segment)?;
                    points.extend([(track, leave), (track, entry)]);
                }
            }
            if let Some(text) = &edge.label
                && let Some(spot) = label_spot(path)
            {
                let width = text.width();
                // The cells across the flow from the label's first cell to the line.
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
                            gap_start(layer)? + exit.get(layer)? + leads.get(layer)?;
                        let entry = signed(*entries.get(segment)?)? - signed(before_line)?;
                        match axis {
                            Axis::Vertical => (signed(after_tracks)?, entry),
                            Axis::Horizontal => {
                                let gap_end =
                                    gap_start(layer)? + exit.get(layer)? + gaps.get(layer)?;
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
                        let size = *layer_size.get(slot.layer)?;
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

    let memberships = chart.memberships()?;
    // Inner frames first, so that each outer one is drawn around them.
    let innermost_first = chart.innermost_first();
    // Whether a drawn link's marker lies just before each subgraph's frame along the flow,
    // and just after it, where the links ending at the frame put them (see below).
    let mut marked = vec![(false, false); chart.subgraphs.len()];
    for (edge, &reversed) in chart.edges.iter().zip(&layered.reversed) {
        if !edge.stroke.is_visible() {
            continue;
        }
        let ends =
            [(edge.from, !reversed, edge.tail.is_some()), (edge.to, reversed, edge.head.is_some())];
        for (end, leaves, marker) in ends {
            if let End::Subgraph(subgraph) = end
                && marker
                && let Some((before, after)) = marked.get_mut(subgraph)
            {
                if leaves { *after = true } else { *before = true }
            }
        }
    }
    let frames = |scene: &Scene<'_>, routed: &[RoutedLabel<'_>], clear_links: bool| {
        let mut bounds: Vec<Option<FrameBounds<'a>>> =
            chart.subgraphs.iter().map(|_| None).collect();
        for &index in &innermost_first {
            let (Some(subgraph), Some(is_member)) =
                (chart.subgraphs.get(index), memberships.get(index))
            else {
                continue;
            };
            let inner: Vec<_> = chart
                .subgraphs
                .iter()
                .zip(&bounds)
                .zip(&marked)
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
            .zip(&memberships)
            .enumerate()
            .filter_map(|(index, (frame, is_member))| Some((index, is_member.as_slice(), frame?)))
            .collect::<Vec<_>>()
    };
    // The cells each layer's frames take before and after its boxes in `scene`, laid
    // out from `layer_start`.
    let margins = |layer_start: &[usize],
                   (scene, routed): &(Scene<'_>, Vec<RoutedLabel<'_>>)|
     -> Option<(Vec<usize>, Vec<usize>)> {
        let mut enter = vec![0; layered.layer_count];
        let mut exit = vec![0; layered.layer_count];
        for (index, _, frame) in frames(scene, routed, true) {
            let Some((first, last)) = member_layers(chart.subgraphs.get(index)?, layered) else {
                continue;
            };
            let before = signed(*layer_start.get(first)?)? - frame.main.0;
            let after = frame.main.1 + 1 - signed(layer_start.get(last)? + layer_size.get(last)?)?;
            let enter = enter.get_mut(first)?;
            *enter = (*enter).max(unsigned(before.max(0))?);
            let exit = exit.get_mut(last)?;
            *exit = (*exit).max(unsigned(after.max(0))?);
        }
        Some((enter, exit))
    };
    // A frame reaches as far past its boxes wherever the layers are, or less when its
    // title is what widens it, so a drawing without room for frames measures how much
    // room to make.
    let unframed = vec![0; layered.layer_count];
    let unframed_start = layer_starts(&unframed, &unframed)?;
    let (mut enter, mut exit) = margins(&unframed_start, &place(&unframed_start, &unframed)?)?;
    let mut layer_start = layer_starts(&enter, &exit)?;
    let mut placed = place(&layer_start, &exit)?;
    // In a horizontal layout the title runs along the flow, and the room made moves the
    // tracks its links cross the top border on, so the title may need more room than
    // was made. The layers are spaced out again only once: the overlap check below
    // accepts or rejects the result, so no further round is needed.
    if axis == Axis::Horizontal {
        let (needed_enter, needed_exit) = margins(&layer_start, &placed)?;
        let short = |needed: &[usize], made: &[usize]| needed.iter().zip(made).any(|(n, m)| n > m);
        if short(&needed_enter, &enter) || short(&needed_exit, &exit) {
            let wider = |needed: &[usize], made: &[usize]| {
                needed.iter().zip(made).map(|(&n, &m)| n.max(m)).collect::<Vec<_>>()
            };
            (enter, exit) = (wider(&needed_enter, &enter), wider(&needed_exit, &exit));
            layer_start = layer_starts(&enter, &exit)?;
            placed = place(&layer_start, &exit)?;
        }
    }
    // The room left for a title clear of the links crossing its frame's top border
    // was worked out before the final placement; when that room falls short and a frame
    // covers another box or frame, every title stays at its corner, over the links,
    // rather than the drawing being given up.
    let (mut scene, mut routed) = placed;
    let mut framed = frames(&scene, &routed, true);
    if frames_overlap(chart, &framed, &scene, axis) {
        framed = frames(&scene, &routed, false);
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
    shift_labels(&mut routed, main_by, cross_by)?;
    shift_scene(&mut scene, main_by, cross_by);
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

/// The first and last layers holding members of `subgraph`.
fn member_layers(subgraph: &Subgraph, layered: &Layered) -> Option<(usize, usize)> {
    let layers = subgraph.members.iter().filter_map(|&member| layered.slots.get(member));
    let first = layers.clone().map(|slot| slot.layer).min()?;
    let last = layers.map(|slot| slot.layer).max()?;
    Some((first, last))
}

/// The first and last cells of a rectangle along the flow, then across it, which may lie
/// before the scene's first cells.
type Rectangle = ((isize, isize), (isize, isize));

/// The first and last cells of a frame's borders along and across the flow, which may
/// lie before the scene's first cells.
struct FrameBounds<'a> {
    title: &'a Label,
    /// Cells on screen from the top-left corner to the blank before the title.
    title_offset: usize,
    main: (isize, isize),
    cross: (isize, isize),
}

/// The first and last of `size` cells from `start`; `None` past `isize::MAX`.
fn cell_span(start: usize, size: usize) -> Option<(isize, isize)> {
    Some((signed(start)?, signed(start + size)? - 1))
}

/// The frame around `subgraph`'s boxes, the lines of the links between its members (its
/// members' self loops among them), the `routed` labels of those links and the frames
/// `inner` of the subgraphs nested in it, given by their first and last cells along and
/// across the flow with whether a link's marker lies just before and just after each
/// along the flow: the frame margins away from them, further where a link marker next
/// to a member box or just outside a nested frame would otherwise fall on the border,
/// further across the flow by the cells the subgraph grew by, and long enough to the right on
/// screen to fit `┌─ title ─┐`, with the title clear of the links crossing the top
/// border when `clear_links` holds.
fn frame_bounds<'a>(
    (subgraph, outer): (&'a Subgraph, &[usize]),
    is_member: &[bool],
    inner: &[(Rectangle, (bool, bool))],
    scene: &Scene<'_>,
    routed: &[RoutedLabel<'_>],
    direction: Direction,
    clear_links: bool,
) -> Option<FrameBounds<'a>> {
    let axis = direction.axis();
    let is_member = |end: End| end.node().is_some_and(|node| is_member.get(node) == Some(&true));
    let boxes =
        subgraph.members.iter().filter_map(|&member| scene.boxes.get(member)).map(|placed| {
            Some((
                cell_span(placed.main, axis.box_main_size(placed.node))?,
                cell_span(placed.cross, axis.box_cross_size(placed.node))?,
            ))
        });
    // A link between members runs inside the frame, as Mermaid puts its dummy nodes in
    // the cluster.
    let lines = scene
        .links
        .iter()
        .filter(|link| is_member(link.edge.from) && is_member(link.edge.to))
        .flat_map(|link| &link.points)
        .map(|&(main, cross)| Some((cell_span(main, 1)?, cell_span(cross, 1)?)));
    // The labels on links between members lie inside the frame like their boxes.
    let labels = routed.iter().filter_map(|&RoutedLabel { link, label, at: (main, cross) }| {
        let edge = scene.links.get(link)?.edge;
        (is_member(edge.from) && is_member(edge.to)).then_some(())?;
        let (main_size, cross_size) = axis.label_size(label);
        let (main_size, cross_size) = (signed(main_size)?, signed(cross_size)?);
        Some(Some(((main, main + main_size - 1), (cross, cross + cross_size - 1))))
    });
    let ((main_low, main_high), (cross_low, cross_high)) = boxes
        .chain(lines)
        .chain(labels)
        .chain(inner.iter().map(|&(rectangle, _)| Some(rectangle)))
        .collect::<Option<Vec<_>>>()?
        .into_iter()
        .reduce(
            |((main_a, main_b), (cross_a, cross_b)), ((main_c, main_d), (cross_c, cross_d))| {
                (
                    (main_a.min(main_c), main_b.max(main_d)),
                    (cross_a.min(cross_c), cross_b.max(cross_d)),
                )
            },
        )?;
    let markers = scene.links.iter().flat_map(|link| {
        let head = link.edge.head.filter(|_| is_member(link.edge.to)).and(link.points.last());
        let tail = link.edge.tail.filter(|_| is_member(link.edge.from)).and(link.points.first());
        head.into_iter().chain(tail)
    });
    // The markers of the links ending at a nested frame lie in the cell just outside its
    // border along the flow.
    let inner_markers = inner.iter().flat_map(|&(((low, high), (cross, _)), (before, after))| {
        before.then_some((low - 1, cross)).into_iter().chain(after.then_some((high + 1, cross)))
    });
    let (main_margin, cross_margin) = axis.frame_margins();
    let (main_margin, cross_margin) = (signed(main_margin)?, signed(cross_margin)?);
    // A marker next to a member box, or just outside a nested frame, stays inside the
    // frame rather than on its border, where the two glyphs would overwrite each other.
    let (main_before, main_after, cross_before, cross_after) = markers
        .map(|&(main, cross)| Some((signed(main)?, signed(cross)?)))
        .collect::<Option<Vec<_>>>()?
        .into_iter()
        .chain(inner_markers)
        .fold(
            (main_margin, main_margin, cross_margin, cross_margin),
            |(main_before, main_after, cross_before, cross_after), (main, cross)| {
                (
                    main_before.max(main_low + 1 - main),
                    main_after.max(main + 1 - main_high),
                    cross_before.max(cross_low + 1 - cross),
                    cross_after.max(cross + 1 - cross_high),
                )
            },
        );
    // The frame grew across the flow on both sides so that the links meeting its borders
    // have cells of their own (see `grow_frames`).
    let spread = signed(subgraph.spread)?;
    let mut main = (main_low - main_before, main_high + main_after);
    let mut cross = (cross_low - cross_before - spread, cross_high + cross_after + spread);
    // The top border on screen, and the frame's cells along it from the top-left corner.
    let (top, along, from_high) = match axis {
        Axis::Horizontal => (cross.0, &mut main, direction.points_backward()),
        Axis::Vertical => {
            let top = if direction.points_backward() { main.1 } else { main.0 };
            (top, &mut cross, false)
        }
    };
    let title_width = subgraph.title.width();
    let offset = if clear_links {
        title_offset(top_border_crossings(scene, outer, axis, top, *along, from_high), title_width)
    } else {
        TITLE_CORNER_OFFSET
    };
    // The cells from the first border to the last.
    let span = signed(frame_width(title_width, offset) - 1)?;
    if from_high {
        along.0 = along.0.min(along.1 - span);
    } else {
        along.1 = along.1.max(along.0 + span);
    }
    Some(FrameBounds { title: &subgraph.title, title_offset: offset, main, cross })
}

/// The cells, counted on screen from the frame's top-left corner, where links meet the
/// frame's top border at `top`, given the frame's first and last cells along it and the
/// subgraphs `outer` it is nested in. Links past the right end count too, since the frame
/// may widen to fit its title. The end segment of a link to or from one of `outer` does
/// not count: it was routed to a cell among that subgraph's members and is cut back to
/// the cell just outside that subgraph's frame afterwards, so it never reaches a frame
/// nested inside.
fn top_border_crossings(
    scene: &Scene<'_>,
    outer: &[usize],
    axis: Axis,
    top: isize,
    (low, high): (isize, isize),
    from_high: bool,
) -> Vec<usize> {
    let other_frame =
        |end: End| matches!(end, End::Subgraph(subgraph) if outer.contains(&subgraph));
    let mut crossings = Vec::new();
    let segments = scene.links.iter().flat_map(|link| {
        let last = link.points.len().saturating_sub(2);
        link.points.windows(2).enumerate().filter_map(move |(segment, pair)| {
            let cut = (segment == 0 && other_frame(link.edge.from))
                || (segment == last && other_frame(link.edge.to));
            (!cut).then_some(pair)
        })
    });
    for pair in segments {
        let &[(main_a, cross_a), (main_b, cross_b)] = pair else { continue };
        // The segment's cells across the border and along it, by the border's axis.
        let ((across_a, across_b), (along_a, along_b)) = match axis {
            Axis::Horizontal => ((cross_a, cross_b), (main_a, main_b)),
            Axis::Vertical => ((main_a, main_b), (cross_a, cross_b)),
        };
        let (Some(across_a), Some(across_b)) = (signed(across_a), signed(across_b)) else {
            continue;
        };
        if !(across_a.min(across_b)..=across_a.max(across_b)).contains(&top) {
            continue;
        }
        for cell in along_a.min(along_b)..=along_a.max(along_b) {
            let Some(cell) = signed(cell) else { continue };
            let offset = if from_high { high - cell } else { cell - low };
            crossings.extend(unsigned(offset));
        }
    }
    crossings
}

/// Whether a frame shares a cell with a box that is not one of its members or with
/// another frame other than one nested in it or enclosing it, which would show that
/// box, or the other frame's boxes, as belonging to its subgraph.
fn frames_overlap(
    chart: &Flowchart,
    framed: &[(usize, &[bool], FrameBounds<'_>)],
    scene: &Scene<'_>,
    axis: Axis,
) -> bool {
    let nested = |a: usize, b: usize| chart.encloses(a, b) || chart.encloses(b, a);
    let meet = |(low_a, high_a): (isize, isize), (low_b, high_b): (isize, isize)| {
        low_a <= high_b && low_b <= high_a
    };
    let covers = |frame: &FrameBounds<'_>, main: (isize, isize), cross: (isize, isize)| {
        meet(frame.main, main) && meet(frame.cross, cross)
    };
    framed.iter().enumerate().any(|(index, (subgraph, is_member, frame))| {
        let covers_a_stranger = scene.boxes.iter().enumerate().any(|(node, placed)| {
            is_member.get(node) != Some(&true)
                && cell_span(placed.main, axis.box_main_size(placed.node))
                    .zip(cell_span(placed.cross, axis.box_cross_size(placed.node)))
                    .is_none_or(|(main, cross)| covers(frame, main, cross))
        });
        covers_a_stranger
            || framed
                .iter()
                .skip(index + 1)
                .filter(|(other, ..)| !nested(*subgraph, *other))
                .any(|(.., other)| covers(frame, other.main, other.cross))
    })
}

/// Moves every `routed` label `main` cells on along the flow and `cross` cells across it.
/// `None` when a distance does not fit an `isize`.
fn shift_labels(routed: &mut [RoutedLabel<'_>], main: usize, cross: usize) -> Option<()> {
    let (main_by, cross_by) = (signed(main)?, signed(cross)?);
    for label in routed {
        label.at.0 += main_by;
        label.at.1 += cross_by;
    }
    Some(())
}

/// Moves every box, link and frame of `scene` `main` cells on along the flow and `cross`
/// cells across it.
pub(super) fn shift_scene(scene: &mut Scene<'_>, main: usize, cross: usize) {
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
}

fn unsigned(value: isize) -> Option<usize> {
    usize::try_from(value).ok()
}

/// The `routed` labels, each at the place the routing fixed for it, and the cells
/// everything else must move right by so that no label starts left of the first column.
pub(super) fn place_labels<'a>(routed: &[RoutedLabel<'a>]) -> (Vec<PlacedLabel<'a>>, usize) {
    // A label left of a line near the drawing's first column moves everything else on
    // across the flow, as a frame reaching past it does.
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
    // For each node, the links to other slots, with the order of the slot each goes to
    // (see [`Slot::order`](super::layout::Slot::order)), and the self loops.
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
/// border's ends (see [`Axis::port_range`]).
fn side_inset(direction: Direction, node: &Node, leaving: bool) -> usize {
    match (&node.body, direction.axis()) {
        // The side links leave by is the right one on screen, unless the flow points left.
        (Body::Box { shape, .. }, Axis::Horizontal) => {
            outline(*shape).side_inset(leaving != direction.points_backward())
        }
        _ => 0,
    }
}
