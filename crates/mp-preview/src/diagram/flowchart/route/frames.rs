//! The cells each subgraph frame takes, around its members, the links between them and
//! its nested frames, and whether a frame covers a box or frame outside its subgraph.

use std::ops::RangeInclusive;

use super::{PlacedBox, RoutedLabel, Unframed, unsigned};
use crate::diagram::flowchart::chart::{Direction, End, Flowchart, Subgraph};
use crate::diagram::flowchart::label::Label;
use crate::diagram::flowchart::layout::{Axis, TITLE_CORNER_OFFSET, frame_width, title_offset};
use crate::diagram::flowchart::signed;

/// The first and last cells of a rectangle along the flow, then across it, which may lie
/// before the scene's first cells.
type Rectangle = (RangeInclusive<isize>, RangeInclusive<isize>);

/// The first and last cells of a frame's borders along and across the flow, which may
/// lie before the scene's first cells.
pub(super) struct FrameBounds<'a> {
    pub(super) title: &'a Label,
    /// Cells on screen from the top-left corner to the blank before the title.
    pub(super) title_offset: usize,
    pub(super) main: RangeInclusive<isize>,
    pub(super) cross: RangeInclusive<isize>,
}

/// The first and last of `size` cells from `start`; `None` past `isize::MAX`.
fn cell_span(start: usize, size: usize) -> Option<RangeInclusive<isize>> {
    Some(signed(start)?..=signed(start + size)? - 1)
}

/// A subgraph among the others: the subgraph, its index, the cells its frame grows by
/// (see [`Growth::frames`](crate::diagram::flowchart::layout::Growth::frames)), and, by
/// subgraph and then subgraph, whether the second is nested in the first at any depth.
pub(super) struct Nesting<'a, 'n> {
    pub(super) subgraph: &'a Subgraph,
    pub(super) index: usize,
    pub(super) spread: usize,
    pub(super) nested: &'n [Vec<bool>],
}

impl Nesting<'_, '_> {
    /// Whether subgraph `other` is nested, at any depth, in this one.
    fn encloses(&self, other: usize) -> bool {
        self.nested.get(self.index).and_then(|row| row.get(other)) == Some(&true)
    }

    /// Whether this subgraph is nested, at any depth, in subgraph `other`.
    fn is_inside(&self, other: usize) -> bool {
        self.nested.get(other).and_then(|row| row.get(self.index)) == Some(&true)
    }
}

/// Whether a drawn link's marker lies in the cell just before a frame along the flow,
/// and in the cell just after it.
#[derive(Clone, Copy, Debug, Default)]
pub(super) struct Marked {
    pub(super) before: bool,
    pub(super) after: bool,
}

/// Where a frame's title goes along its top border.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(super) enum TitlePlacement {
    /// Clear of the links crossing the top border, the frame widening for it as needed.
    ClearOfLinks,
    /// At the top-left corner, over any link crossing the border there.
    AtCorner,
}

/// The frame around the content of `nesting`'s subgraph: its boxes, the lines of the
/// links between its members and the subgraphs nested in it (its members' self loops
/// among them), the `routed` labels of those links and the frames `inner` of the
/// subgraphs nested in it directly, each given by its first and last cells along and
/// across the flow with where a link's marker lies next to it along the flow. The frame
/// margins away from the content, further where a link marker next to a member box or
/// just outside a nested frame would otherwise fall on the border, further across the
/// flow by the cells the subgraph grew by, and long enough to the right on screen to fit
/// `┌─ title ─┐`, its title placed as `title` says. `is_member` is indexed by node.
/// `None` when the subgraph has nothing to enclose or a cell does not fit an `isize`.
pub(super) fn frame_bounds<'a>(
    nesting: &Nesting<'a, '_>,
    is_member: &[bool],
    inner: &[(Rectangle, Marked)],
    routed: &Unframed<'_>,
    direction: Direction,
    title: TitlePlacement,
) -> Option<FrameBounds<'a>> {
    let subgraph = nesting.subgraph;
    let axis = direction.axis();
    let is_member = |end: End| end.node().is_some_and(|node| is_member.get(node) == Some(&true));
    let inside = |end: End| match end {
        End::Node(_) => is_member(end),
        End::Subgraph(other) => nesting.encloses(other),
    };
    let boxes =
        subgraph.members.iter().filter_map(|&member| routed.boxes.get(member)).map(|placed| {
            Some((
                cell_span(placed.main, axis.box_main_size(placed.node))?,
                cell_span(placed.cross, axis.box_cross_size(placed.node, placed.spread))?,
            ))
        });
    // A link between members, or between a member and a nested subgraph, runs inside the
    // frame, as Mermaid puts its dummy nodes in the cluster.
    let lines = routed
        .links
        .iter()
        .filter(|link| inside(link.edge.from) && inside(link.edge.to))
        .flat_map(|link| &link.points)
        .map(|cell| Some((cell_span(cell.main, 1)?, cell_span(cell.cross, 1)?)));
    // The labels on those links lie inside the frame like their boxes.
    let labels =
        routed.labels.iter().filter_map(|&RoutedLabel { link, label, at: (main, cross) }| {
            let edge = routed.links.get(link)?.edge;
            (inside(edge.from) && inside(edge.to)).then_some(())?;
            let (main_size, cross_size) = axis.label_size(label);
            let (main_size, cross_size) = (signed(main_size)?, signed(cross_size)?);
            Some(Some((main..=main + main_size - 1, cross..=cross + cross_size - 1)))
        });
    let hull = |a: RangeInclusive<isize>, b: RangeInclusive<isize>| {
        *a.start().min(b.start())..=*a.end().max(b.end())
    };
    let (main, cross) = boxes
        .chain(lines)
        .chain(labels)
        .chain(inner.iter().map(|(rectangle, _)| Some(rectangle.clone())))
        .collect::<Option<Vec<_>>>()?
        .into_iter()
        .reduce(|(main_a, cross_a), (main_b, cross_b)| {
            (hull(main_a, main_b), hull(cross_a, cross_b))
        })?;
    let ((main_low, main_high), (cross_low, cross_high)) = (main.into_inner(), cross.into_inner());
    let markers = routed.links.iter().flat_map(|link| {
        let head = link.edge.head.filter(|_| is_member(link.edge.to)).and(link.points.last());
        let tail = link.edge.tail.filter(|_| is_member(link.edge.from)).and(link.points.first());
        head.into_iter().chain(tail)
    });
    // The markers of the links ending at a nested frame lie in the cell just outside its
    // border along the flow.
    let inner_markers = inner.iter().flat_map(|((main, cross), Marked { before, after })| {
        let cross = *cross.start();
        let before = before.then_some((main.start() - 1, cross));
        before.into_iter().chain(after.then_some((main.end() + 1, cross)))
    });
    let (main_margin, cross_margin) = axis.frame_margins();
    let (main_margin, cross_margin) = (signed(main_margin)?, signed(cross_margin)?);
    // A marker next to a member box, or just outside a nested frame, stays inside the
    // frame rather than on its border, where the two glyphs would overwrite each other.
    let (main_before, main_after, cross_before, cross_after) = markers
        .map(|cell| Some((signed(cell.main)?, signed(cell.cross)?)))
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
    let spread = signed(nesting.spread)?;
    let main = main_low - main_before..=main_high + main_after;
    let cross = cross_low - cross_before - spread..=cross_high + cross_after + spread;
    // The top border on screen, and the frame's cells along it from the top-left corner.
    let (top, along, from_high) = match axis {
        Axis::Horizontal => (*cross.start(), main.clone(), direction.points_backward()),
        Axis::Vertical => {
            let top = if direction.points_backward() { *main.end() } else { *main.start() };
            (top, cross.clone(), false)
        }
    };
    let title_width = subgraph.title.width();
    let offset = match title {
        TitlePlacement::ClearOfLinks => title_offset(
            top_border_crossings(routed, nesting, axis, top, &along, from_high),
            title_width,
        ),
        TitlePlacement::AtCorner => TITLE_CORNER_OFFSET,
    };
    // The cells from the first border to the last.
    let span = signed(frame_width(title_width, offset) - 1)?;
    let along = widened(along, span, from_high);
    let (main, cross) = match axis {
        Axis::Horizontal => (along, cross),
        Axis::Vertical => (main, along),
    };
    Some(FrameBounds { title: &subgraph.title, title_offset: offset, main, cross })
}

/// The first and last cells `low..=high` of a frame along its top border, moved apart
/// to at least `span` cells from the first to the last: `low` moves back when
/// `from_high`, the frame's top-left corner being at `high`, and `high` moves on
/// otherwise.
fn widened(along: RangeInclusive<isize>, span: isize, from_high: bool) -> RangeInclusive<isize> {
    let (low, high) = along.into_inner();
    if from_high { low.min(high - span)..=high } else { low..=high.max(low + span) }
}

/// The cells, counted on screen from the frame's top-left corner, where links meet the
/// frame's top border at `top`, given the frame's first and last cells along it and the
/// `nesting` of its subgraph. Links past the right end count too, since the frame may
/// widen to fit its title. The end segment of a link to or from a subgraph the frame's
/// subgraph is nested in does not count: it was routed to a cell among that subgraph's
/// members and is cut back to the cell just outside that subgraph's frame afterwards, so
/// it never reaches a frame nested inside.
fn top_border_crossings(
    routed: &Unframed<'_>,
    nesting: &Nesting<'_, '_>,
    axis: Axis,
    top: isize,
    along: &RangeInclusive<isize>,
    from_high: bool,
) -> Vec<usize> {
    let other_frame =
        |end: End| matches!(end, End::Subgraph(subgraph) if nesting.is_inside(subgraph));
    let mut crossings = Vec::new();
    let segments = routed.links.iter().flat_map(|link| {
        let last = link.points.len().saturating_sub(2);
        link.points.windows(2).enumerate().filter_map(move |(segment, pair)| {
            let cut = (segment == 0 && other_frame(link.edge.from))
                || (segment == last && other_frame(link.edge.to));
            (!cut).then_some(pair)
        })
    });
    for pair in segments {
        let &[a, b] = pair else { continue };
        let ((across_a, across_b), (along_a, along_b)) = match axis {
            Axis::Horizontal => ((a.cross, b.cross), (a.main, b.main)),
            Axis::Vertical => ((a.main, b.main), (a.cross, b.cross)),
        };
        let (Some(across_a), Some(across_b)) = (signed(across_a), signed(across_b)) else {
            continue;
        };
        if !(across_a.min(across_b)..=across_a.max(across_b)).contains(&top) {
            continue;
        }
        for cell in along_a.min(along_b)..=along_a.max(along_b) {
            let Some(cell) = signed(cell) else { continue };
            let offset = if from_high { along.end() - cell } else { cell - along.start() };
            crossings.extend(unsigned(offset));
        }
    }
    crossings
}

/// Whether a frame shares a cell with one of `boxes` that is not one of its members or
/// with another frame other than one nested in it or enclosing it, which would show
/// that box, or the other frame's boxes, as belonging to its subgraph. A box whose
/// cells do not fit in `isize` counts as covered.
pub(super) fn frames_overlap(
    chart: &Flowchart,
    framed: &[(usize, &[bool], FrameBounds<'_>)],
    boxes: &[PlacedBox<'_>],
    axis: Axis,
) -> bool {
    let nested = |a: usize, b: usize| chart.encloses(a, b) || chart.encloses(b, a);
    let meet = |a: &RangeInclusive<isize>, b: &RangeInclusive<isize>| {
        a.start() <= b.end() && b.start() <= a.end()
    };
    let covers =
        |frame: &FrameBounds<'_>, main: &RangeInclusive<isize>, cross: &RangeInclusive<isize>| {
            meet(&frame.main, main) && meet(&frame.cross, cross)
        };
    framed.iter().enumerate().any(|(index, (subgraph, is_member, frame))| {
        let covers_a_stray = boxes.iter().enumerate().any(|(node, placed)| {
            is_member.get(node) != Some(&true)
                && cell_span(placed.main, axis.box_main_size(placed.node))
                    .zip(cell_span(placed.cross, axis.box_cross_size(placed.node, placed.spread)))
                    .is_none_or(|(main, cross)| covers(frame, &main, &cross))
        });
        covers_a_stray
            || framed
                .iter()
                .skip(index + 1)
                .filter(|(other, ..)| !nested(*subgraph, *other))
                .any(|(.., other)| covers(frame, &other.main, &other.cross))
    })
}
