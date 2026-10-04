//! Positions along the flow, the paths of links between the placed boxes, and where
//! link labels go. Positions are `(main, cross)`: cells along and across the flow.

use std::collections::HashSet;
use std::ops::RangeInclusive;

use unicode_width::UnicodeWidthStr;

use super::layout::{
    Axis, Layered, SELF_LOOP_CELLS, TITLE_CORNER_OFFSET, frame_width, port_cells, title_offset,
};
use super::parse::{Direction, Edge, End, Flowchart, Node, Subgraph};
use super::signed;

/// A drawing before it is oriented on screen.
#[derive(Debug)]
pub(super) struct Scene<'a> {
    pub boxes: Vec<PlacedBox<'a>>,
    pub links: Vec<Route<'a>>,
    pub labels: Vec<PlacedLabel<'a>>,
    pub frames: Vec<PlacedFrame<'a>>,
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

/// Text starting at a cell; it extends along the flow in a horizontal layout and
/// across it in a vertical one, i.e. always to the right on screen.
#[derive(Debug)]
pub(super) struct PlacedLabel<'a> {
    pub text: &'a str,
    pub main: usize,
    pub cross: usize,
}

/// A subgraph frame, given by the cells of its borders along and across the flow.
#[derive(Debug)]
pub(super) struct PlacedFrame<'a> {
    pub title: &'a str,
    /// Cells on screen from the top-left corner to the blank before the title.
    pub title_offset: usize,
    pub main: RangeInclusive<usize>,
    pub cross: RangeInclusive<usize>,
}

impl Axis {
    /// Cells a label takes along and across the flow.
    pub(super) fn label_size(self, text: &str) -> (usize, usize) {
        match self {
            Self::Horizontal => (text.width(), 1),
            Self::Vertical => (1, text.width()),
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
            let (main_size, cross_size) = axis.label_size(label.text);
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
/// more, and routes each edge from its source's port to its target, leaving the labels
/// to [`place_labels`]. `None` when a subgraph frame would cover a box outside its
/// subgraph or a frame neither nested in it nor enclosing it.
///
/// Several links entering one box get their own entry cells along its border, in the
/// order of the cells they come from.
///
/// A link segment whose ends are at different positions across the flow turns on a
/// track in the gap between the layers; all segments of one source share its track,
/// and each source in a gap has its own. A frame title moves clear of the links
/// crossing its frame's top border, unless that makes a frame cover a box outside its
/// subgraph or a frame neither nested in it nor enclosing it.
pub(super) fn route<'a>(
    chart: &'a Flowchart,
    layered: &Layered,
    axis: Axis,
    layer_gap: usize,
) -> Option<Scene<'a>> {
    let slots = &layered.slots;
    let paths = &layered.paths;
    let mut layer_size = vec![0; layered.layer_count];
    for (node, slot) in chart.nodes.iter().zip(slots) {
        let size = layer_size.get_mut(slot.layer)?;
        *size = (*size).max(axis.box_main_size(node));
    }
    let entries = entry_ports(chart, layered, axis)?;
    let exits = exit_ports(chart, layered, axis)?;

    let mut turns = vec![false; slots.len()];
    let mut label_width = vec![0; layered.layer_count];
    for ((((edge, path), entries), &reversed), &exit) in
        chart.edges.iter().zip(paths).zip(&entries).zip(&layered.reversed).zip(&exits)
    {
        for (segment, (&from, &entry)) in path.iter().zip(entries).enumerate() {
            let leave = if segment == 0 { exit } else { slots.get(from)?.port };
            if leave != entry {
                *turns.get_mut(from)? = true;
            }
        }
        // `place_labels` looks for a place from the target, which a reversed edge's path
        // reaches first rather than last.
        // A self loop's path is its one slot, and its label widens no gap.
        let label_source = if reversed {
            path.first().copied().filter(|_| path.len() > 1)
        } else {
            last_source(path)
        };
        if let Some(label) = &edge.label
            && let Some(label_source) = label_source
        {
            let width = label_width.get_mut(slots.get(label_source)?.layer)?;
            *width = (*width).max(label.width());
        }
    }
    let (track_of, track_counts) = assign_tracks(layered, &turns)?;
    // A self loop runs through the cells after its box along the flow, so the tracks
    // after its layer start that much later rather than merging with it.
    let mut loop_cells = vec![0; layered.layer_count];
    for (node, _) in chart.edges.iter().filter_map(Edge::nodes).filter(|(from, to)| from == to) {
        *loop_cells.get_mut(slots.get(node)?.layer)? = SELF_LOOP_CELLS;
    }

    let gaps: Vec<usize> = track_counts
        .iter()
        .zip(&label_width)
        .zip(&loop_cells)
        .map(|((&tracks, &label), &loop_cells)| {
            let lead = if tracks == 0 { 0 } else { 1 + loop_cells + tracks };
            // After the last track, a horizontal gap holds the label with a blank column
            // on either side, and a vertical one the cell of the arrowhead.
            let needed = match axis {
                Axis::Horizontal => lead + label + 2,
                Axis::Vertical => lead + 1,
            };
            layer_gap.max(needed)
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
    let place = |layer_start: &[usize], exit: &[usize]| -> Option<Scene<'a>> {
        let gap_start = |layer: usize| Some(layer_start.get(layer)? + layer_size.get(layer)?);
        let mut scene =
            Scene { boxes: Vec::new(), links: Vec::new(), labels: Vec::new(), frames: Vec::new() };
        for (node, slot) in chart.nodes.iter().zip(slots) {
            scene.boxes.push(PlacedBox {
                node,
                main: *layer_start.get(slot.layer)?,
                cross: slot.cross,
            });
        }
        for ((((edge, path), entries), &reversed), &exit_cell) in
            chart.edges.iter().zip(paths).zip(&entries).zip(&layered.reversed).zip(&exits)
        {
            let (&first, &last) = (path.first()?, path.last()?);
            let first_slot = slots.get(first)?;
            if first == last {
                let node = chart.nodes.get(first)?;
                let main = *layer_start.get(first_slot.layer)?;
                let points = self_loop(axis, node, main, first_slot.cross, exit_cell);
                scene.links.push(Route { edge, points });
                continue;
            }
            let out = match chart.nodes.get(first) {
                Some(node) => layer_start.get(first_slot.layer)? + axis.box_main_size(node),
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
                        + track_of.get(from_index)?;
                    points.extend([(track, leave), (track, entry)]);
                }
            }
            let entry = *entries.last()?;
            let into = layer_start.get(slots.get(last)?.layer)?.checked_sub(1)?;
            points.push((into, entry));
            if reversed {
                points.reverse();
            }
            scene.links.push(Route { edge, points });
        }
        Some(scene)
    };

    let memberships: Vec<Vec<bool>> = chart
        .subgraphs
        .iter()
        .map(|subgraph| subgraph.membership(chart.nodes.len()))
        .collect::<Option<_>>()?;
    // Inner frames first, so that each outer one is drawn around them.
    let innermost_first = chart.innermost_first();
    let frames = |scene: &Scene<'_>, clear_links: bool| {
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
                .filter(|(inner, _)| inner.parent == Some(index))
                .filter_map(|(_, inner)| inner.as_ref().map(|inner| (inner.main, inner.cross)))
                .collect();
            if let Some(slot) = bounds.get_mut(index) {
                *slot =
                    frame_bounds(subgraph, is_member, &inner, scene, chart.direction, clear_links);
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
    let margins = |layer_start: &[usize], scene: &Scene<'_>| -> Option<(Vec<usize>, Vec<usize>)> {
        let mut enter = vec![0; layered.layer_count];
        let mut exit = vec![0; layered.layer_count];
        for (index, _, frame) in frames(scene, true) {
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
    let mut scene = place(&layer_start, &exit)?;
    // In a horizontal layout the title runs along the flow, and the room made moves the
    // tracks its links cross the top border on, so the title may need more room than
    // was made. The layers are spaced out again only once: the overlap check below
    // accepts or rejects the result, so no further round is needed.
    if axis == Axis::Horizontal {
        let (needed_enter, needed_exit) = margins(&layer_start, &scene)?;
        let short = |needed: &[usize], made: &[usize]| needed.iter().zip(made).any(|(n, m)| n > m);
        if short(&needed_enter, &enter) || short(&needed_exit, &exit) {
            let wider = |needed: &[usize], made: &[usize]| {
                needed.iter().zip(made).map(|(&n, &m)| n.max(m)).collect::<Vec<_>>()
            };
            (enter, exit) = (wider(&needed_enter, &enter), wider(&needed_exit, &exit));
            layer_start = layer_starts(&enter, &exit)?;
            scene = place(&layer_start, &exit)?;
        }
    }
    // The room left for a title clear of the links crossing its frame's top border
    // was worked out before the final placement; when that room falls short and a frame
    // covers another box or frame, every title stays at its corner, over the links,
    // rather than the drawing being given up.
    let mut framed = frames(&scene, true);
    if frames_overlap(chart, &framed, &scene, axis) {
        framed = frames(&scene, false);
        if frames_overlap(chart, &framed, &scene, axis) {
            return None;
        }
    }
    // Frames reaching past the first cells along or across the flow move everything
    // else on by as much.
    let (main_shift, cross_shift) = framed.iter().fold((0, 0), |(main, cross), (.., frame)| {
        (main.max(-frame.main.0), cross.max(-frame.cross.0))
    });
    shift(&mut scene, unsigned(main_shift)?, unsigned(cross_shift)?);
    let moved =
        |(low, high): (isize, isize), by: isize| Some(unsigned(low + by)?..=unsigned(high + by)?);
    let mut frame_of = vec![None; chart.subgraphs.len()];
    for (index, _, frame) in framed {
        let main = moved(frame.main, main_shift)?;
        let cross = moved(frame.cross, cross_shift)?;
        *frame_of.get_mut(index)? = Some(main.clone());
        scene.frames.push(PlacedFrame {
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
    for (link, &reversed) in scene.links.iter_mut().zip(&layered.reversed) {
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
    Some(scene)
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
    title: &'a str,
    /// Cells on screen from the top-left corner to the blank before the title.
    title_offset: usize,
    main: (isize, isize),
    cross: (isize, isize),
}

/// The frame around `subgraph`'s boxes, the self loops of its members and the frames
/// `inner` of the subgraphs nested in it, given by their first and last cells along and
/// across the flow: the frame margins away from them, further where a link marker next
/// to a member box would otherwise fall on the border, and long enough to the right on
/// screen to fit `┌─ title ─┐`, with the title clear of the links crossing the top
/// border when `clear_links` holds.
fn frame_bounds<'a>(
    subgraph: &'a Subgraph,
    is_member: &[bool],
    inner: &[Rectangle],
    scene: &Scene<'_>,
    direction: Direction,
    clear_links: bool,
) -> Option<FrameBounds<'a>> {
    let axis = direction.axis();
    let is_member = |end: End| end.node().is_some_and(|node| is_member.get(node) == Some(&true));
    let span = |start: usize, size: usize| Some((signed(start)?, signed(start + size)? - 1));
    let boxes =
        subgraph.members.iter().filter_map(|&member| scene.boxes.get(member)).map(|placed| {
            Some((
                span(placed.main, axis.box_main_size(placed.node))?,
                span(placed.cross, axis.box_cross_size(placed.node))?,
            ))
        });
    let loops = scene
        .links
        .iter()
        .filter(|link| link.edge.from == link.edge.to && is_member(link.edge.from))
        .flat_map(|link| &link.points)
        .map(|&(main, cross)| Some((span(main, 1)?, span(cross, 1)?)));
    let ((main_low, main_high), (cross_low, cross_high)) = boxes
        .chain(loops)
        .chain(inner.iter().copied().map(Some))
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
    let (main_margin, cross_margin) = axis.frame_margins();
    let (main_margin, cross_margin) = (signed(main_margin)?, signed(cross_margin)?);
    // A marker next to a member box stays inside the frame rather than on its border,
    // where the two glyphs would overwrite each other.
    let (main_before, main_after, cross_before, cross_after) = markers
        .map(|&(main, cross)| Some((signed(main)?, signed(cross)?)))
        .collect::<Option<Vec<_>>>()?
        .into_iter()
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
    let mut main = (main_low - main_before, main_high + main_after);
    let mut cross = (cross_low - cross_before, cross_high + cross_after);
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
        title_offset(top_border_crossings(scene, axis, top, *along, from_high), title_width)
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
/// frame's top border at `top`, given the frame's first and last cells along it.
/// Links past the right end count too, since the frame may widen to fit its title.
fn top_border_crossings(
    scene: &Scene<'_>,
    axis: Axis,
    top: isize,
    (low, high): (isize, isize),
    from_high: bool,
) -> Vec<usize> {
    let mut crossings = Vec::new();
    for pair in scene.links.iter().flat_map(|link| link.points.windows(2)) {
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
    let nested =
        |a: usize, b: usize| chart.enclosing(a).contains(&b) || chart.enclosing(b).contains(&a);
    let meet = |(low_a, high_a): (isize, isize), (low_b, high_b): (isize, isize)| {
        low_a <= high_b && low_b <= high_a
    };
    let covers = |frame: &FrameBounds<'_>, main: (isize, isize), cross: (isize, isize)| {
        meet(frame.main, main) && meet(frame.cross, cross)
    };
    framed.iter().enumerate().any(|(index, (subgraph, is_member, frame))| {
        let covers_a_stranger = scene.boxes.iter().enumerate().any(|(node, placed)| {
            let span =
                |start: usize, size: usize| Some((signed(start)?, signed(start + size)? - 1));
            is_member.get(node) != Some(&true)
                && span(placed.main, axis.box_main_size(placed.node))
                    .zip(span(placed.cross, axis.box_cross_size(placed.node)))
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

/// Moves every box and link `main` cells on along the flow and `cross` cells across it.
fn shift(scene: &mut Scene<'_>, main: usize, cross: usize) {
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

/// Puts each link's label beside the first straight run along the flow, counted from
/// the target, where it covers no box, line or earlier label and has a blank cell on
/// either side; if there is no such place, beside the run nearest the target anyway.
pub(super) fn place_labels(scene: &mut Scene<'_>, axis: Axis) {
    if scene.links.iter().all(|link| link.edge.label.is_none()) {
        return;
    }
    let mut occupied: HashSet<(isize, isize)> = occupied_cells(scene, axis)
        .into_iter()
        .filter_map(|(main, cross)| Some((signed(main)?, signed(cross)?)))
        .collect();
    let mut placed = Vec::new();
    for link in &scene.links {
        let Some(text) = &link.edge.label else { continue };
        let width = text.width();
        let mut candidates = label_candidates(&link.points, width, axis, &scene.frames);
        let Some(preferred) = candidates.clone().next() else { continue };
        // Text runs along the flow in a horizontal layout and across it in a vertical
        // one; `offset` counts cells in that direction from the cell before the label.
        let cell = |(main, cross): (isize, isize), offset: isize| match axis {
            Axis::Horizontal => (main + offset - 1, cross),
            Axis::Vertical => (main, cross + offset - 1),
        };
        let Some(width) = signed(width) else { continue };
        let start = candidates
            .find(|&start| (0..=width + 1).all(|offset| !occupied.contains(&cell(start, offset))))
            .unwrap_or(preferred);
        occupied.extend((1..=width).map(|offset| cell(start, offset)));
        placed.push((text, start));
    }
    // A label left of a line near the drawing's first column moves everything else on
    // across the flow, as a frame reaching past it does.
    let cross_shift = placed.iter().map(|&(_, (_, cross))| -cross).max().unwrap_or(0).max(0);
    if let Some(by) = unsigned(cross_shift) {
        shift(scene, 0, by);
    }
    for (text, (main, cross)) in placed {
        if let (Some(main), Some(cross)) = (unsigned(main), unsigned(cross + cross_shift)) {
            scene.labels.push(PlacedLabel { text, main, cross });
        }
    }
}

/// The cells covered by boxes, lines and frame borders.
fn occupied_cells(scene: &Scene<'_>, axis: Axis) -> HashSet<(usize, usize)> {
    let mut occupied = HashSet::new();
    for placed in &scene.boxes {
        let (main_size, cross_size) =
            (axis.box_main_size(placed.node), axis.box_cross_size(placed.node));
        for main in placed.main..placed.main + main_size {
            occupied.extend((placed.cross..placed.cross + cross_size).map(|cross| (main, cross)));
        }
    }
    for link in &scene.links {
        for pair in link.points.windows(2) {
            if let &[(main_a, cross_a), (main_b, cross_b)] = pair {
                for main in main_a.min(main_b)..=main_a.max(main_b) {
                    for cross in cross_a.min(cross_b)..=cross_a.max(cross_b) {
                        occupied.insert((main, cross));
                    }
                }
            }
        }
    }
    for frame in &scene.frames {
        let (main_ends, cross_ends) =
            ([*frame.main.start(), *frame.main.end()], [*frame.cross.start(), *frame.cross.end()]);
        for main in frame.main.clone() {
            occupied.extend(cross_ends.map(|cross| (main, cross)));
        }
        for cross in frame.cross.clone() {
            occupied.extend(main_ends.map(|main| (main, cross)));
        }
    }
    occupied
}

/// The places for a label `width` cells wide beside each straight run of `points`
/// along the flow, from the target backwards, a run crossing a frame's border counting
/// as separate runs on either side of it. In a horizontal layout the label is
/// centred on the run, or starts at the first column when centring it would start
/// before that, on the row above it and then on the row below; in a vertical one it
/// starts two columns right of the run and then ends two columns left of it, each on
/// the run's middle row and then on its other rows. A place left of the drawing's
/// first column has a negative cell across the flow.
fn label_candidates(
    points: &[(usize, usize)],
    width: usize,
    axis: Axis,
    frames: &[PlacedFrame<'_>],
) -> impl Iterator<Item = (isize, isize)> + Clone {
    let last = points.len().saturating_sub(1);
    points.windows(2).enumerate().rev().flat_map(move |(index, pair)| {
        let runs = match pair {
            &[(from, cross), (to, to_cross)] if cross == to_cross && from != to => {
                // A run's end where the line turns is a corner; a label beside it would
                // touch the line running on from that corner.
                let from_turns = usize::from(index > 0);
                let to_turns = usize::from(index + 1 < last);
                let (low, high) = if from < to {
                    (from + from_turns, to - to_turns)
                } else {
                    (to + to_turns, from - from_turns)
                };
                let mut pieces = split_at_borders(low, high, cross, frames);
                if from < to {
                    pieces.reverse();
                }
                pieces.into_iter().map(|(low, high)| (low, high, cross)).collect()
            }
            _ => Vec::new(),
        };
        runs.into_iter().flat_map(move |(low, high, cross)| {
            let places: Vec<(usize, isize)> = match axis {
                Axis::Horizontal => {
                    let start = (low + high + 1).saturating_sub(width) / 2;
                    let rows = [cross.checked_sub(1), Some(cross + 1)];
                    rows.into_iter()
                        .flatten()
                        .filter_map(|row| Some((start, signed(row)?)))
                        .collect()
                }
                Axis::Vertical if low <= high => {
                    let middle = (low + high) / 2;
                    let rows =
                        [middle].into_iter().chain((low..=high).filter(move |&row| row != middle));
                    let (Some(line), Some(width)) = (signed(cross), signed(width)) else {
                        return Vec::new();
                    };
                    [line + 2, line - 1 - width]
                        .into_iter()
                        .flat_map(|start| rows.clone().map(move |row| (row, start)))
                        .collect()
                }
                Axis::Vertical => Vec::new(),
            };
            places
                .into_iter()
                .filter_map(|(main, cross)| Some((signed(main)?, cross)))
                .collect::<Vec<_>>()
        })
    })
}

/// The parts of the run from `low` to `high` along the flow at `cross` that lie between
/// the borders of `frames` it crosses, in ascending order; the whole run when it
/// crosses none.
fn split_at_borders(
    low: usize,
    high: usize,
    cross: usize,
    frames: &[PlacedFrame<'_>],
) -> Vec<(usize, usize)> {
    let mut borders: Vec<usize> = frames
        .iter()
        .filter(|frame| frame.cross.contains(&cross))
        .flat_map(|frame| [*frame.main.start(), *frame.main.end()])
        .filter(|border| (low..=high).contains(border))
        .collect();
    if borders.is_empty() {
        return vec![(low, high)];
    }
    borders.sort_unstable();
    let mut pieces = Vec::new();
    let mut start = low;
    for border in borders {
        if start < border {
            pieces.push((start, border - 1));
        }
        start = border + 1;
    }
    if start <= high {
        pieces.push((start, high));
    }
    pieces
}

/// The slot a path's last segment leaves, or `None` for a self loop's path.
fn last_source(path: &[usize]) -> Option<usize> {
    path.get(path.len().checked_sub(2)?).copied()
}

/// Numbers the slots in `turns` within each layer by their port, giving each its own
/// track in the gap after its layer. Returns each slot's track and the number of tracks
/// in each gap.
fn assign_tracks(layered: &Layered, turns: &[bool]) -> Option<(Vec<usize>, Vec<usize>)> {
    let slots = &layered.slots;
    let mut turning = vec![Vec::new(); layered.layer_count];
    for ((index, slot), &turns) in slots.iter().enumerate().zip(turns) {
        if turns {
            turning.get_mut(slot.layer)?.push(index);
        }
    }
    let mut track_of = vec![0; slots.len()];
    for sources in &mut turning {
        sources.sort_by_key(|&index| slots.get(index).map(|slot| slot.port));
        for (track, &index) in sources.iter().enumerate() {
            *track_of.get_mut(index)? = track;
        }
    }
    Some((track_of, turning.iter().map(Vec::len).collect()))
}

/// For each edge, the cell across the flow where each of its segments enters the next
/// slot on its path, indexed like [`Layered::paths`] without their first slot.
fn entry_ports(chart: &Flowchart, layered: &Layered, axis: Axis) -> Option<Vec<Vec<usize>>> {
    let slots = &layered.slots;
    let mut arrivals = vec![Vec::new(); slots.len()];
    for (edge, path) in layered.paths.iter().enumerate() {
        for (segment, pair) in path.windows(2).enumerate() {
            if let &[from, to] = pair {
                arrivals.get_mut(to)?.push((slots.get(from)?.port, edge, segment));
            }
        }
    }
    let mut entries: Vec<Vec<usize>> =
        layered.paths.iter().map(|path| vec![0; path.len().saturating_sub(1)]).collect();
    for (slot_index, arrivals) in arrivals.iter().enumerate() {
        let slot = slots.get(slot_index)?;
        // A passing slot is a single cell.
        let cells = chart.nodes.get(slot_index).map_or(0..=0, |node| axis.port_range(node));
        // Arrivals are listed by edge, so equal sources keep the order of their edges.
        let sources: Vec<usize> = arrivals.iter().map(|&(port, ..)| port).collect();
        let cells = port_cells(axis, &sources, &cells, slot.port.checked_sub(slot.cross)?)?;
        for (&(_, edge, segment), cell) in arrivals.iter().zip(cells) {
            *entries.get_mut(edge)?.get_mut(segment)? = slot.cross + cell;
        }
    }
    Some(entries)
}

/// For each edge, the cell across the flow where it leaves the first slot of its path.
fn exit_ports(chart: &Flowchart, layered: &Layered, axis: Axis) -> Option<Vec<usize>> {
    let slots = &layered.slots;
    let mut leaving = vec![Vec::new(); chart.nodes.len()];
    let mut exits = vec![0; layered.paths.len()];
    for (edge, path) in layered.paths.iter().enumerate() {
        let first = *path.first()?;
        let towards = match path.get(1) {
            Some(&next) => (false, slots.get(next)?.port),
            None => (true, 0),
        };
        match leaving.get_mut(first) {
            Some(leaving) => leaving.push((towards, edge)),
            // A link leaving a frame starts at its single cell on the frame's border.
            None => *exits.get_mut(edge)? = slots.get(first)?.port,
        }
    }
    for ((node, slot), leaving) in chart.nodes.iter().zip(slots).zip(&leaving) {
        let others: Vec<_> = leaving.iter().map(|&(towards, _)| towards).collect();
        let port = slot.port.checked_sub(slot.cross)?;
        let cells = port_cells(axis, &others, &axis.port_range(node), port)?;
        for (&(_, edge), cell) in leaving.iter().zip(cells) {
            *exits.get_mut(edge)? = slot.cross + cell;
        }
    }
    Some(exits)
}

/// A loop that leaves the box through the middle of its far border along the flow,
/// runs through the [`SELF_LOOP_CELLS`] cells beyond that border and as many beyond the
/// box across the flow (which the layout keeps free), and enters the middle of the
/// box's border on that across-flow side, so that its marker is clear of every other
/// link's end.
fn self_loop(
    axis: Axis,
    node: &Node,
    main: usize,
    cross: usize,
    port: usize,
) -> Vec<(usize, usize)> {
    let out = main + axis.box_main_size(node);
    let middle = main + axis.box_main_size(node) / 2;
    let after = cross + axis.box_cross_size(node);
    // The last of the cells the loop runs through beyond the box, along and across.
    let (far_main, far_cross) = (out + SELF_LOOP_CELLS - 1, after + SELF_LOOP_CELLS - 1);
    vec![(out, port), (far_main, port), (far_main, far_cross), (middle, far_cross), (middle, after)]
}
