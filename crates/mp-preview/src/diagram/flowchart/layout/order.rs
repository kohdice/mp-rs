//! The order of the slots within each layer, dagre's `order`: a barycentre sweep that
//! keeps a new order only when fewer links cross, and each subgraph's slots kept
//! together. A slot's *rank* here is its place within its layer's stack, not dagre's
//! `rank`, which is the layer itself (see the `layers` module).

use std::collections::HashMap;

/// The end of a link at a frame, which takes no place in its layer's stack.
#[derive(Clone, Copy, Debug)]
pub(super) struct FrameEnd {
    /// The slot of the end.
    pub slot: usize,
    /// The layer the end lies in.
    pub layer: usize,
    /// The subgraph whose frame the link meets.
    pub subgraph: usize,
}

/// Whether `slot` lies in `subgraph`, directly or in a subgraph nested in it, given the
/// subgraphs each slot lies in, `chain_of`.
pub(super) fn in_subgraph(chain_of: &[Vec<usize>], slot: usize, subgraph: usize) -> bool {
    chain_of.get(slot).is_some_and(|chain| chain.contains(&subgraph))
}

/// The rank of each of `slot_count` slots in its layer's stack (see
/// [`Slot::order`](super::Slot::order)), the stacks being `members` by layer, as
/// [`rank_layer`] ranks them.
pub(super) fn stack_ranks(
    members: &[Vec<usize>],
    frame_ends: &[FrameEnd],
    chain_of: &[Vec<usize>],
    slot_count: usize,
) -> Option<Vec<usize>> {
    let mut rank = vec![0; slot_count];
    for layer in 0..members.len() {
        rank_layer(&mut rank, members, layer, frame_ends, chain_of)?;
    }
    Some(rank)
}

/// Sets the rank of each slot of `layer` in `rank`: twice its index in the stack plus
/// one, and for the end of a link at a frame, twice the index of the frame's first
/// member there (or 0 without one), so that the end ranks just before that member.
/// `frame_ends` lists those ends, which take no place in the stack, each with its layer
/// and its subgraph; `chain_of` lists the subgraphs each slot lies in. The ends at
/// a frame's border are placed after its members (see `frame_ports`), which prefers the
/// earlier of two equally good cells, so an end ranked before the member it is placed
/// beside lets the links to both leave their source straight.
fn rank_layer(
    rank: &mut [usize],
    members: &[Vec<usize>],
    layer: usize,
    frame_ends: &[FrameEnd],
    chain_of: &[Vec<usize>],
) -> Option<()> {
    let stack = members.get(layer)?;
    for (index, &slot) in stack.iter().enumerate() {
        *rank.get_mut(slot)? = 2 * index + 1;
    }
    for end in frame_ends.iter().filter(|end| end.layer == layer) {
        let first = stack.iter().position(|&member| in_subgraph(chain_of, member, end.subgraph));
        *rank.get_mut(end.slot)? = 2 * first.unwrap_or(0);
    }
    Some(())
}

/// `members` with each layer after the first in the order of the mean rank of its slots'
/// parents in the layer before, ranked as [`rank_layer`] ranks them, the ends of links
/// at frames included; a layer keeps its old order unless the new one has fewer links
/// crossing between the two layers. A single sweep from the first layer keeps the work
/// bounded; ties keep the earlier order, which starts as declaration order.
pub(super) fn reduce_crossings(
    mut members: Vec<Vec<usize>>,
    parents: &[Vec<usize>],
    frame_ends: &[FrameEnd],
    chain_of: &[Vec<usize>],
) -> Option<Vec<Vec<usize>>> {
    let mut position = stack_ranks(&members, frame_ends, chain_of, parents.len())?;
    for layer_index in 1..members.len() {
        let layer = members.get(layer_index)?;
        let barycentre = |slot: usize| -> Option<(u64, u64)> {
            let parents = parents.get(slot)?;
            if parents.is_empty() {
                return Some((u64::try_from(*position.get(slot)?).ok()?, 1));
            }
            let sum = parents.iter().map(|&parent| position.get(parent)).sum::<Option<usize>>()?;
            Some((u64::try_from(sum).ok()?, u64::try_from(parents.len()).ok()?))
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
            *members.get_mut(layer_index)? = reordered;
            rank_layer(&mut position, &members, layer_index, frame_ends, chain_of)?;
        }
    }
    Some(members)
}

/// The slots of `layer` with those of each subgraph moved up to the first of them, so
/// that no other slot lies between them, and those of a nested subgraph likewise within
/// the run of the one enclosing it, in the order of `layer` otherwise. `chain_of` lists
/// the subgraphs each slot lies in, outermost first.
pub(super) fn group_subgraph_members(
    layer: &[usize],
    chain_of: &[Vec<usize>],
) -> Option<Vec<usize>> {
    let mut first_at = HashMap::new();
    for (index, &slot) in layer.iter().enumerate() {
        for &subgraph in chain_of.get(slot)? {
            first_at.entry(subgraph).or_insert(index);
        }
    }
    // A slot sorts by where each of its subgraphs first appears, outermost first, then
    // by its own place.
    let mut keyed = layer
        .iter()
        .enumerate()
        .map(|(index, &slot)| {
            let mut key = chain_of
                .get(slot)?
                .iter()
                .map(|subgraph| first_at.get(subgraph).copied())
                .collect::<Option<Vec<_>>>()?;
            key.push(index);
            Some((key, slot))
        })
        .collect::<Option<Vec<_>>>()?;
    keyed.sort_by(|(key_a, _), (key_b, _)| key_a.cmp(key_b));
    Some(keyed.into_iter().map(|(_, slot)| slot).collect())
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
