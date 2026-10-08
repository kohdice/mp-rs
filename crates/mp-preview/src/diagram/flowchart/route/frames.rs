//! The cells each subgraph frame takes, around its members, the links between them and
//! its nested frames, and whether a frame covers a box or frame outside its subgraph.

use super::{RoutedLabel, Scene, unsigned};
use crate::diagram::flowchart::label::Label;
use crate::diagram::flowchart::layout::{Axis, TITLE_CORNER_OFFSET, frame_width, title_offset};
use crate::diagram::flowchart::parse::{Direction, End, Flowchart, Subgraph};
use crate::diagram::flowchart::signed;

/// The first and last cells of a rectangle along the flow, then across it, which may lie
/// before the scene's first cells.
type Rectangle = ((isize, isize), (isize, isize));

/// The first and last cells of a frame's borders along and across the flow, which may
/// lie before the scene's first cells.
pub(super) struct FrameBounds<'a> {
    pub(super) title: &'a Label,
    /// Cells on screen from the top-left corner to the blank before the title.
    pub(super) title_offset: usize,
    pub(super) main: (isize, isize),
    pub(super) cross: (isize, isize),
}

/// The first and last of `size` cells from `start`; `None` past `isize::MAX`.
fn cell_span(start: usize, size: usize) -> Option<(isize, isize)> {
    Some((signed(start)?, signed(start + size)? - 1))
}

/// The frame around `subgraph`'s content: its boxes, the lines of the links between its
/// members (its members' self loops among them), the `routed` labels of those links and
/// the frames `inner` of the subgraphs nested in it, each given by its first and last
/// cells along and across the flow with whether a link's marker lies just before and just
/// after it along the flow. The frame margins away from the content, further where a link
/// marker next to a member box or just outside a nested frame would otherwise fall on the
/// border, further across the flow by the cells the subgraph grew by, and long enough to
/// the right on screen to fit `┌─ title ─┐`, with the title clear of the links crossing
/// the top border when `clear_links` holds. `outer` lists the subgraphs this one is
/// nested in, and `is_member` is indexed by node. `None` when the subgraph has nothing to
/// enclose or a cell does not fit an `isize`.
pub(super) fn frame_bounds<'a>(
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
/// box, or the other frame's boxes, as belonging to its subgraph. A box whose cells do
/// not fit in `isize` counts as covered.
pub(super) fn frames_overlap(
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
        let covers_a_stray = scene.boxes.iter().enumerate().any(|(node, placed)| {
            is_member.get(node) != Some(&true)
                && cell_span(placed.main, axis.box_main_size(placed.node))
                    .zip(cell_span(placed.cross, axis.box_cross_size(placed.node)))
                    .is_none_or(|(main, cross)| covers(frame, main, cross))
        });
        covers_a_stray
            || framed
                .iter()
                .skip(index + 1)
                .filter(|(other, ..)| !nested(*subgraph, *other))
                .any(|(.., other)| covers(frame, other.main, other.cross))
    })
}
