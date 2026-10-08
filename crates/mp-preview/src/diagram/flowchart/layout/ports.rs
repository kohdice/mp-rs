//! The cells where links meet a box's or a frame's borders across the flow, and the cells
//! a self loop takes.

use std::ops::RangeInclusive;

use super::Axis;
use super::reach::{Crossing, Reach, label_cross_reach};
use crate::diagram::flowchart::label::Label;
use crate::diagram::flowchart::signed;

/// Cells a self loop runs through beyond its box along the flow: its legs leave and
/// re-enter the box's far border on the first, and its run joins them on the second.
pub(in crate::diagram::flowchart) const SELF_LOOP_CELLS: usize = 2;

/// Cells across the flow from a self loop's first leg to its second, which leaves the
/// cell between them free, as Mermaid centres a loop's span on its node.
pub(in crate::diagram::flowchart) const SELF_LOOP_SPAN: usize = 2;

/// The cells along the flow a self loop carrying `label` takes after its box, before
/// the tracks there (the cells where the lines to the next layer turn, see `assign_tracks`
/// in `route`): its legs and its run, then, outside the loop beyond the run, the
/// label's rows in a vertical layout, or in a horizontal one a blank cell and the label
/// running along the flow.
pub(in crate::diagram::flowchart) fn self_loop_cells(axis: Axis, label: Option<&Label>) -> usize {
    match (axis, label) {
        (_, None) => SELF_LOOP_CELLS,
        (Axis::Vertical, Some(label)) => SELF_LOOP_CELLS + label.height(),
        (Axis::Horizontal, Some(label)) => SELF_LOOP_CELLS + 1 + label.width(),
    }
}

/// Where `label`, on a self loop whose first leg is at `leg` across the flow, starts: the
/// cells along the flow after the box's far border, and the cell across the flow, which
/// may lie before cell 0. Mermaid puts the label outside the loop beyond its run, centred
/// on the loop: here directly beyond the run in a vertical layout, with the usual lean
/// left (see [`label_reach`](super::label_reach)), and one blank cell beyond it with its
/// middle row on the row between the legs in a horizontal one (see
/// [`label_cross_reach`]). Upstream keeps a 4 px gap from the run; a cell is the least
/// gap text has, and in a vertical layout a blank row would push the next layer further
/// away, so there the label touches the run.
pub(in crate::diagram::flowchart) fn self_loop_label_at(
    axis: Axis,
    leg: usize,
    label: &Label,
) -> Option<(usize, isize)> {
    let centre = signed(leg + SELF_LOOP_SPAN / 2)?;
    let (before, _) = label_cross_reach(axis, label);
    let along = match axis {
        Axis::Vertical => SELF_LOOP_CELLS,
        Axis::Horizontal => SELF_LOOP_CELLS + 1,
    };
    Some((along, centre - signed(before)?))
}

/// The cell where each link meets one border of a slot, counted from the slot's first
/// cell and indexed like `others`, the positions across the flow of the links' other
/// ends, given the reach of each end's label in `reach`, indexed like `others` (empty
/// when no end has a label). The links take the border's port `cells` about `port` in
/// the order of their other ends (see [`ranked_ports`], [`symmetric_offsets`] and
/// [`end_gaps`]). [`grow_boxes`](super::grow_boxes) and
/// [`grow_for_labels`](super::grow_for_labels) are expected to have made room; when the
/// cells still do not hold the links, they share them out from `port` outwards, several
/// to a cell.
pub(in crate::diagram::flowchart) fn port_cells<P: Ord>(
    axis: Axis,
    others: &[P],
    reach: &[Reach],
    cells: &RangeInclusive<usize>,
    port: usize,
) -> Option<Vec<usize>> {
    let ordered = reach_in_order(others, reach);
    let ranked = ranked_ports(axis, port, cells, &end_gaps(axis, &ordered), others.len())?;
    scatter_by_order(others, &ranked)
}

/// The reach of each end listed in the ends' order along the border, that is `others`
/// sorted, ties keeping their order (the order [`scatter_by_order`] ranks by); `reach` is
/// indexed like `others`, and an end missing from it has `None`.
pub(super) fn reach_in_order<P: Ord>(others: &[P], reach: &[Reach]) -> Vec<Reach> {
    let mut order: Vec<usize> = (0..others.len()).collect();
    order.sort_by_key(|&end| others.get(end));
    order.iter().map(|&end| reach.get(end).copied().flatten()).collect()
}

/// Whether a link end at `cell` whose label has `reach` lies as far from each of
/// `crossings` as two neighbouring ends must ([`pair_gap`]).
fn clear_of(axis: Axis, cell: isize, reach: Reach, crossings: &[Crossing]) -> bool {
    let apart = |distance: isize, before: Reach, after: Reach| {
        usize::try_from(distance).is_ok_and(|distance| distance >= pair_gap(axis, before, after))
    };
    crossings.iter().all(|&(at, crossing)| {
        if cell < at {
            apart(at - cell, reach, crossing)
        } else {
            apart(cell - at, crossing, reach)
        }
    })
}

/// The cells, in order along a border, of link ends whose labels have the reach in
/// `ordered`, packed from `first`: each end at the first cell clear of `crossings`
/// ([`clear_of`]) from `first`, or from its gap in `gaps` after the end before it.
/// `None` when an end would lie past `last`.
fn packed_ports(
    axis: Axis,
    ordered: &[Reach],
    gaps: &[usize],
    crossings: &[Crossing],
    first: isize,
    last: isize,
) -> Option<Vec<isize>> {
    let mut cells: Vec<isize> = Vec::with_capacity(ordered.len());
    for (index, &reach) in ordered.iter().enumerate() {
        let mut cell = match index.checked_sub(1) {
            Some(before) => cells.get(before)? + signed(*gaps.get(before)?)?,
            None => first,
        };
        while cell <= last && !clear_of(axis, cell, reach, crossings) {
            cell += 1;
        }
        if cell > last {
            return None;
        }
        cells.push(cell);
    }
    Some(cells)
}

/// The cells a box would give link ends whose labels have the reach in `ordered`, `gaps`
/// apart about `middle` (see [`symmetric_offsets`]), when they all lie within
/// `low..=high` clear of `crossings`.
fn box_spread(
    axis: Axis,
    ordered: &[Reach],
    gaps: &[usize],
    crossings: &[Crossing],
    (low, high): (isize, isize),
    middle: isize,
) -> Option<Vec<isize>> {
    let cells: Vec<isize> =
        symmetric_offsets(ordered.len(), gaps)?.into_iter().map(|offset| middle + offset).collect();
    let fits = cells.iter().zip(ordered).all(|(&cell, &reach)| {
        (low..=high).contains(&cell) && clear_of(axis, cell, reach, crossings)
    });
    fits.then_some(cells)
}

/// The cells, in order along a border running from `low` to `high`, of a frame's own link
/// ends whose labels have the reach in `ordered`, clear of the `crossings` of links
/// passing the border into the frame. They are spread about `middle` as on a box where
/// that fits ([`box_spread`]); otherwise they take the packing ([`packed_ports`]) whose
/// farthest end lies nearest `middle`, ties going to the one with the fewest crossings
/// between its ends, so the frame's own links stay together. `None` when nothing fits.
pub(super) fn frame_ports(
    axis: Axis,
    ordered: &[Reach],
    crossings: &[Crossing],
    (low, high): (isize, isize),
    middle: isize,
) -> Option<Vec<isize>> {
    let gaps = end_gaps(axis, ordered);
    if let Some(cells) = box_spread(axis, ordered, &gaps, crossings, (low, high), middle) {
        return Some(cells);
    }
    if crossings.is_empty() {
        return None;
    }
    let mut best: Option<((isize, usize, isize), Vec<isize>)> = None;
    for first in low..=high {
        // Packing from a later cell puts no end before the one from an earlier cell.
        let Some(cells) = packed_ports(axis, ordered, &gaps, crossings, first, high) else {
            break;
        };
        let distance = |cell: &isize| (cell - middle).abs();
        let farthest = cells.iter().map(distance).max().unwrap_or(0);
        let between = match (cells.first(), cells.last()) {
            (Some(&first), Some(&last)) => {
                crossings.iter().filter(|&&(at, _)| first < at && at < last).count()
            }
            _ => 0,
        };
        let key = (farthest, between, cells.iter().map(distance).sum());
        let past_middle = cells.first().is_some_and(|&cell| cell >= middle);
        let beaten = best.as_ref().is_some_and(|((best, ..), _)| farthest > *best);
        if best.as_ref().is_none_or(|(best, _)| key < *best) {
            best = Some((key, cells));
        }
        // From here on no end moves nearer the middle, so the farthest only grows.
        if past_middle && beaten {
            break;
        }
    }
    best.map(|(_, cells)| cells)
}

/// The fewest cells a frame grows by on each side of its members' cells `low..=high` for
/// [`frame_ports`] to place its own link ends, whose labels have the reach in `ordered`,
/// about `middle` clear of `crossings`. Growing never makes them fit less well, so the
/// count is found by halving an interval between a count that does not fit and one that
/// does. The top of the interval — past the members' cells and the crossings, room for
/// every gap and one more step on either side — is taken to fit and is never tested.
pub(super) fn frame_growth(
    axis: Axis,
    ordered: &[Reach],
    crossings: &[Crossing],
    (low, high): (isize, isize),
    middle: isize,
) -> Option<usize> {
    let gaps = end_gaps(axis, ordered);
    let fits = |grown: isize| {
        let (low, high) = (low - grown, high + grown);
        box_spread(axis, ordered, &gaps, crossings, (low, high), middle).is_some()
            || (!crossings.is_empty()
                && packed_ports(axis, ordered, &gaps, crossings, low, high).is_some())
    };
    let widest = |reach: Reach| reach.map_or(0, |(before, after)| before.max(after));
    let reach = crossings.iter().map(|&(_, reach)| reach).chain(ordered.iter().copied());
    let step = signed(axis.entry_spacing().max(2 * reach.map(widest).max().unwrap_or(0) + 2))?;
    let outside = crossings.iter().map(|&(at, _)| (low - at).max(at - high)).max().unwrap_or(0);
    let total = gaps.iter().try_fold(0, |total: isize, &gap| Some(total + signed(gap)?))?;
    let (mut short, mut enough) = (-1, (high - low) + total + outside.max(0) + 2 * step);
    while enough - short > 1 {
        let grown = short + (enough - short) / 2;
        if fits(grown) { enough = grown } else { short = grown }
    }
    usize::try_from(enough).ok()
}

/// The cells of `ranked`, listed in their order along a border, given back in the order
/// of `keys`: the end at each rank is the one that rank takes when `keys` are sorted, ties
/// keeping the order of `keys`.
pub(in crate::diagram::flowchart) fn scatter_by_order<P: Ord>(
    keys: &[P],
    ranked: &[usize],
) -> Option<Vec<usize>> {
    let mut order: Vec<usize> = (0..keys.len()).collect();
    order.sort_by_key(|&end| keys.get(end));
    let mut cells_of = vec![0; keys.len()];
    for (&end, &cell) in order.iter().zip(ranked) {
        *cells_of.get_mut(end)? = cell;
    }
    Some(cells_of)
}

/// The cells where the links leaving a box meet its far border along the flow, counted
/// from the box's first cell, as [`exit_cells`] spreads them.
#[derive(Debug)]
pub(in crate::diagram::flowchart) struct Exits {
    /// The cells of the links to other slots, in rank order (see [`scatter_by_order`]).
    pub targets: Vec<usize>,
    /// The cell of each self loop's first leg; its second is [`SELF_LOOP_SPAN`] further.
    pub legs: Vec<usize>,
}

/// The cells where the links leaving a box meet its far border along the flow, as
/// [`port_cells`] gives them: first `targets` links to other slots, then a self loop
/// for each of `loop_labels`, the labels of the box's self loops in the order of their
/// edges, each taking two cells [`SELF_LOOP_SPAN`] apart (see [`exit_gaps`]). Upstream
/// centres a loop on the border and lets other edges cross it and its label; crossing
/// glyphs would merge the loop with those lines, so the loops take the last cells
/// instead. `None` when a loop's legs do not get two cells of `cells` exactly that far
/// apart.
pub(in crate::diagram::flowchart) fn exit_cells(
    axis: Axis,
    targets: usize,
    loop_labels: &[Option<&Label>],
    cells: &RangeInclusive<usize>,
    port: usize,
) -> Option<Exits> {
    let gaps = exit_gaps(axis, targets, loop_labels);
    let ranked = ranked_ports(axis, port, cells, &gaps, targets + 2 * loop_labels.len())?;
    let legs = ranked
        .get(targets..)?
        .chunks(2)
        .map(|pair| match *pair {
            [first, second] if second == first + SELF_LOOP_SPAN && cells.contains(&second) => {
                Some(first)
            }
            _ => None,
        })
        .collect::<Option<Vec<_>>>()?;
    let targets = ranked.get(..targets)?.to_vec();
    Some(Exits { targets, legs })
}

/// The distances between neighbouring ends on a box's far border along the flow, in the
/// order [`exit_cells`] lists them: `targets` links to other slots, then a self loop of
/// two legs for each of `loop_labels`. The legs of one loop are [`SELF_LOOP_SPAN`]
/// apart; any other neighbours are [`end_gaps`] apart, as the ends of labelled links
/// entering a box are. In a vertical layout a loop's label lies across the flow, centred
/// on the cell between the legs, so each leg carries the label's reach measured from it;
/// that keeps one blank cell between the label and the line or label next to it. In a
/// horizontal one the label's middle row is the row between the legs, and a leg whose
/// row the label reaches carries the rows reaching on past it, so that the end next to
/// the leg keeps a blank row from the label too.
pub(super) fn exit_gaps(axis: Axis, targets: usize, loop_labels: &[Option<&Label>]) -> Vec<usize> {
    let mut ordered: Vec<Reach> = vec![None; targets];
    let half = SELF_LOOP_SPAN / 2;
    for label in loop_labels {
        let legs = match (axis, label) {
            (Axis::Vertical, Some(label)) => {
                let (before, after) = label_cross_reach(axis, label);
                [
                    Some((before.saturating_sub(half), after + half)),
                    Some((before + half, after.saturating_sub(half))),
                ]
            }
            (Axis::Horizontal, Some(label)) => {
                let (before, after) = label_cross_reach(axis, label);
                [
                    (before >= half).then(|| (before - half, 0)),
                    (after >= half).then(|| (0, after - half)),
                ]
            }
            (_, None) => [None, None],
        };
        ordered.extend(legs);
    }
    let mut gaps = end_gaps(axis, &ordered);
    for index in 0..loop_labels.len() {
        if let Some(gap) = gaps.get_mut(targets + 2 * index) {
            *gap = SELF_LOOP_SPAN;
        }
    }
    gaps
}

/// The cell of each of `count` link ends in their order along a border: spread
/// symmetrically about `port` with `gaps` between them (see [`symmetric_offsets`]), or,
/// when those cells do not all lie within `cells`, shared out from `port` outwards,
/// [`Axis::entry_spacing`] apart and several to a cell.
fn ranked_ports(
    axis: Axis,
    port: usize,
    cells: &RangeInclusive<usize>,
    gaps: &[usize],
    count: usize,
) -> Option<Vec<usize>> {
    let ports = symmetric_ports(port, cells, count, gaps)
        .unwrap_or_else(|| spread_ports(port, cells, axis.entry_spacing(), count));
    (0..count).map(|rank| ports.get(rank * ports.len() / count).copied()).collect()
}

/// The distance between each pair of neighbouring ends, listed in order along the
/// border with the reach of each end's label (see [`pair_gap`]).
pub(super) fn end_gaps(axis: Axis, ordered: &[Reach]) -> Vec<usize> {
    ordered
        .windows(2)
        .map(|pair| match *pair {
            [before, after] => pair_gap(axis, before, after),
            _ => axis.entry_spacing(),
        })
        .collect()
}

/// The distance between two neighbouring ends along a border, the first with the reach
/// `before` and the second with `after`: [`Axis::entry_spacing`], or, where either end has
/// a label, enough for one blank cell between a label and the label or line next to it.
fn pair_gap(axis: Axis, before: Reach, after: Reach) -> usize {
    let spacing = axis.entry_spacing();
    match (before, after) {
        (None, None) => spacing,
        _ => {
            let after_first = before.map_or(0, |(_, after)| after);
            let before_second = after.map_or(0, |(before, _)| before);
            spacing.max(after_first + before_second + 2)
        }
    }
}

/// The offsets from a border's centre of `count` link ends, in ascending order, where
/// `gaps[i]` is the distance between the ends `i` and `i + 1`: for an odd count the
/// middle end at the centre; for an even count the centre skipped, the inner pair their
/// gap apart, the right one half of it out rounded up and the left one the rest, at
/// least one cell. The other ends follow outwards, each its gap from its neighbour
/// nearer the centre. `None` when `gaps` has fewer than `count - 1` entries.
pub(super) fn symmetric_offsets(count: usize, gaps: &[usize]) -> Option<Vec<isize>> {
    let gap = |index: usize| gaps.get(index).and_then(|&gap| isize::try_from(gap).ok());
    let mut offsets = vec![0; count];
    let middle = count / 2;
    let innermost_left = if count % 2 == 1 {
        middle
    } else if let Some(left) = middle.checked_sub(1) {
        let inner_gap = gap(left)?;
        let right = (inner_gap + 1) / 2;
        *offsets.get_mut(left)? = -(inner_gap - right).max(1);
        *offsets.get_mut(middle)? = right;
        left
    } else {
        return Some(offsets);
    };
    for index in middle + 1..count {
        *offsets.get_mut(index)? = offsets.get(index - 1)? + gap(index - 1)?;
    }
    for index in (0..innermost_left).rev() {
        *offsets.get_mut(index)? = offsets.get(index + 1)? - gap(index)?;
    }
    Some(offsets)
}

/// The cells [`symmetric_offsets`] gives `count` link ends with `gaps` between them
/// about `port`, or `None` when one of them lies outside `cells`.
fn symmetric_ports(
    port: usize,
    cells: &RangeInclusive<usize>,
    count: usize,
    gaps: &[usize],
) -> Option<Vec<usize>> {
    symmetric_offsets(count, gaps)?
        .into_iter()
        .map(|offset| port.checked_add_signed(offset).filter(|cell| cells.contains(cell)))
        .collect()
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
