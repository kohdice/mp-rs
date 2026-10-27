//! The order of the slots within each layer, after dagre's `order` (`lib/order` in
//! <https://github.com/dagrejs/dagre>): an order by a depth-first search, then
//! alternating barycentre sweeps, keeping the order with the fewest crossings. The
//! passing slots of long links play the part of dagre's dummy nodes and the ends of
//! links at frames that of its border nodes, which take no place in a stack. Where dagre
//! sorts a subgraph's members as one entry, each layer here keeps a subgraph's slots
//! together at the place of the first of them (see [`group_subgraph_members`]). A slot's
//! *rank* here is its place within its layer's stack, not dagre's `rank`, which is the
//! layer itself (see the `layers` module).

use std::collections::HashMap;

/// The end of a link at a frame, which takes no place in its layer's stack. The functions
/// here take these ends listed by layer, indexed like the stacks.
#[derive(Clone, Copy, Debug)]
pub(super) struct FrameEnd {
    /// The slot of the end.
    pub slot: usize,
    /// The subgraph whose frame the link meets.
    pub subgraph: usize,
}

/// Whether `slot` lies in `subgraph`, directly or in a subgraph nested in it, given the
/// subgraphs each slot lies in, `chain_of`.
pub(super) fn in_subgraph(chain_of: &[Vec<usize>], slot: usize, subgraph: usize) -> bool {
    chain_of.get(slot).is_some_and(|chain| chain.contains(&subgraph))
}

/// The rank of each of `slot_count` slots in its layer's stack (see
/// [`Slot::order`](super::Slot::order)), the stacks being `members` by layer and the ends
/// of links at frames `frame_ends`, as [`rank_layer`] ranks them.
pub(super) fn stack_ranks(
    members: &[Vec<usize>],
    frame_ends: &[Vec<FrameEnd>],
    chain_of: &[Vec<usize>],
    slot_count: usize,
) -> Option<Vec<usize>> {
    let mut rank = vec![0; slot_count];
    for (layer, stack) in members.iter().enumerate() {
        for (slot, slot_rank) in rank_layer(stack, frame_ends.get(layer)?, chain_of) {
            *rank.get_mut(slot)? = slot_rank;
        }
    }
    Some(rank)
}

/// Each slot of a layer with its rank, the layer's stack being `stack`: twice its index
/// in the stack plus one, and for the end of a link at a frame, twice the index of the
/// frame's first member there (or 0 without one), so that the end ranks just before that
/// member. `ends` lists the ends of links at frames in this layer, which take no place in
/// the stack, each with its subgraph; `chain_of` lists the subgraphs each slot lies in.
/// The ends at a frame's border are placed after its members (see `frame_ports`), which
/// prefers the earlier of two equally good cells, so an end ranked before the member it
/// is placed beside lets the links to both leave their source straight.
fn rank_layer(
    stack: &[usize],
    ends: &[FrameEnd],
    chain_of: &[Vec<usize>],
) -> impl Iterator<Item = (usize, usize)> {
    let in_stack = stack.iter().enumerate().map(|(index, &slot)| (slot, 2 * index + 1));
    // The index of each subgraph's first slot in the stack, found in one pass rather than
    // by a search of the stack for each end.
    let mut first_at = HashMap::new();
    if !ends.is_empty() {
        for (index, &slot) in stack.iter().enumerate() {
            for &subgraph in chain_of.get(slot).into_iter().flatten() {
                first_at.entry(subgraph).or_insert(index);
            }
        }
    }
    let at_frames = ends.iter().map(move |end| {
        let first = first_at.get(&end.subgraph).copied();
        (end.slot, 2 * first.unwrap_or(0))
    });
    in_stack.chain(at_frames)
}

/// The count at which the search stops, counted as dagre's `order` counts `lastBest`:
/// from 0, back to 0 on a sweep that brings fewer crossings, and up by one after every
/// sweep, that one included. The search thus stops after four sweeps when it never
/// improves on the first order, and three sweeps after its last improvement otherwise.
const SWEEPS_WITHOUT_IMPROVEMENT: usize = 4;

/// `members`, the slots of each layer's stack, ordered to reduce the links crossing
/// between neighbouring layers, as dagre's `order` orders them: first by a depth-first
/// search over the links (see [`initial_order`]), then by barycentre sweeps alternating
/// upward and downward (see [`sweep`]), keeping the order with the fewest crossings over
/// all layers. A sweep that only ties the best keeps the earlier order, the search's
/// included, so a chart the search leaves without crossings keeps that order; dagre
/// takes the first sweep's order even then. Every layer keeps each subgraph's slots
/// together (see [`group_subgraph_members`]). `parents` and `children` list each slot's
/// neighbours in the layers before and after it, the slots before `node_count` are the
/// nodes, `frame_ends` lists the ends of links at frames by layer, which take no place in
/// the stacks, and `chain_of` the subgraphs each slot lies in.
pub(super) fn reduce_crossings(
    members: &[Vec<usize>],
    parents: &[Vec<usize>],
    children: &[Vec<usize>],
    node_count: usize,
    frame_ends: &[Vec<FrameEnd>],
    chain_of: &[Vec<usize>],
) -> Option<Vec<Vec<usize>>> {
    let mut layers = initial_order(members, children, node_count, frame_ends, chain_of)?;
    let mut position = stack_ranks(&layers, frame_ends, chain_of, parents.len())?;
    let mut fewest = total_crossings(&layers, parents, &position)?;
    let mut best = layers.clone();
    // Counted as in dagre's loop, which resets the count on an improvement and then
    // advances it with every sweep, the improving one included.
    let mut since_best = 0;
    let mut sweeps = 0_usize;
    while since_best < SWEEPS_WITHOUT_IMPROVEMENT && fewest > 0 {
        let pass = Pass { downward: sweeps % 2 == 1, bias_right: sweeps % 4 >= 2 };
        let neighbours = if pass.downward { parents } else { children };
        sweep(&mut layers, &mut position, neighbours, pass, frame_ends, chain_of)?;
        let crossed = total_crossings(&layers, parents, &position)?;
        if crossed < fewest {
            fewest = crossed;
            best.clone_from(&layers);
            since_best = 0;
        }
        since_best += 1;
        sweeps += 1;
    }
    Some(best)
}

/// The stacks of `members`' layers in the order a depth-first search over `children`
/// first reaches their slots, starting from every slot in turn, those of earlier layers
/// first and in slot order within a layer, as dagre's `initOrder` orders them (Gansner
/// et al., "A Technique for Drawing Directed Graphs", 1993). A slot's children are
/// searched in link order, those that are passing slots last: dagre's `normalize`
/// replaces each long edge by a chain of dummy nodes only after the layering, so the
/// edge into the first dummy node follows every other edge of its source. The slots
/// from `node_count` on that take a place in a stack are passing slots. The search
/// passes through the ends of links at frames, `frame_ends` by layer, which take no place
/// in the stacks. Each layer then keeps each subgraph's slots together.
fn initial_order(
    members: &[Vec<usize>],
    children: &[Vec<usize>],
    node_count: usize,
    frame_ends: &[Vec<FrameEnd>],
    chain_of: &[Vec<usize>],
) -> Option<Vec<Vec<usize>>> {
    let slot_count = children.len();
    // The layer of each slot, and whether it takes a place in that layer's stack.
    let mut layer_of: Vec<Option<(usize, bool)>> = vec![None; slot_count];
    for (layer, stack) in members.iter().enumerate() {
        for &slot in stack {
            *layer_of.get_mut(slot)? = Some((layer, true));
        }
    }
    for (layer, ends) in frame_ends.iter().enumerate() {
        for end in ends {
            *layer_of.get_mut(end.slot)? = Some((layer, false));
        }
    }
    let mut roots: Vec<(usize, usize)> = layer_of
        .iter()
        .enumerate()
        .filter_map(|(slot, held)| held.map(|(layer, _)| (layer, slot)))
        .collect();
    roots.sort_unstable();
    let mut layers = vec![Vec::new(); members.len()];
    let mut visited = vec![false; slot_count];
    // Marking a slot when it is popped rather than when it is pushed visits the slots in
    // the order a recursive search would.
    let mut pending = Vec::new();
    for (_, root) in roots {
        pending.push(root);
        while let Some(slot) = pending.pop() {
            let seen = visited.get_mut(slot)?;
            if *seen {
                continue;
            }
            *seen = true;
            if let Some((layer, true)) = *layer_of.get(slot)? {
                layers.get_mut(layer)?.push(slot);
            }
            let next = children.get(slot)?;
            let passing = |&&child: &&usize| {
                child >= node_count && matches!(layer_of.get(child), Some(Some((_, true))))
            };
            pending.extend(next.iter().rev().filter(passing));
            pending.extend(next.iter().rev().filter(|child| !passing(child)));
        }
    }
    layers.iter().map(|layer| group_subgraph_members(layer, chain_of)).collect()
}

/// The direction of one sweep and how it breaks ties.
#[derive(Clone, Copy)]
struct Pass {
    /// Whether the sweep runs from the second layer to the last, ordering each layer by
    /// its slots' parents, rather than from the last but one to the first by their
    /// children.
    downward: bool,
    /// Whether slots with equal barycentres take the reverse of their order before the
    /// sweep, as under dagre's `biasRight`.
    bias_right: bool,
}

/// Orders each layer of `layers` after the first (downward) or before the last (upward)
/// by the barycentre of its slots' `neighbours` in the layer just ordered, as dagre's
/// `sortSubgraph` and `sort` order them: by the mean of the neighbours' ranks in
/// `position`, equal means in the slots' order before the sweep (reversed under
/// [`Pass::bias_right`]), and a slot without such neighbours keeping its index in the
/// stack. Each layer then keeps each subgraph's slots together, and `position` follows
/// the new order; `ends` lists the ends of links at frames by layer.
fn sweep(
    layers: &mut [Vec<usize>],
    position: &mut [usize],
    neighbours: &[Vec<usize>],
    pass: Pass,
    ends: &[Vec<FrameEnd>],
    chain_of: &[Vec<usize>],
) -> Option<()> {
    let count = layers.len();
    let order: Vec<usize> = if pass.downward {
        (1..count).collect()
    } else {
        (0..count.saturating_sub(1)).rev().collect()
    };
    for layer_index in order {
        let layer = layers.get(layer_index)?;
        let mut sortable = Vec::with_capacity(layer.len());
        let mut fixed = Vec::new();
        for (index, &slot) in layer.iter().enumerate() {
            let around = neighbours.get(slot)?;
            if around.is_empty() {
                fixed.push((index, slot));
                continue;
            }
            let sum = around.iter().map(|&other| position.get(other)).sum::<Option<usize>>()?;
            let (sum, weight) = (u64::try_from(sum).ok()?, u64::try_from(around.len()).ok()?);
            sortable.push((sum, weight, index, slot));
        }
        sortable.sort_by(|&(sum_a, weight_a, index_a, _), &(sum_b, weight_b, index_b, _)| {
            let by_place =
                if pass.bias_right { index_b.cmp(&index_a) } else { index_a.cmp(&index_b) };
            (sum_a * weight_b).cmp(&(sum_b * weight_a)).then(by_place)
        });
        // A slot without neighbours takes back its index once the slots before it are
        // placed, as in dagre's `consumeUnsortable`.
        let mut fixed = fixed.into_iter().peekable();
        let mut sorted = Vec::with_capacity(layer.len());
        for (.., slot) in sortable {
            while let Some((_, kept)) = fixed.next_if(|&(index, _)| index <= sorted.len()) {
                sorted.push(kept);
            }
            sorted.push(slot);
        }
        sorted.extend(fixed.map(|(_, slot)| slot));
        let grouped = group_subgraph_members(&sorted, chain_of)?;
        for (slot, slot_rank) in rank_layer(&grouped, ends.get(layer_index)?, chain_of) {
            *position.get_mut(slot)? = slot_rank;
        }
        *layers.get_mut(layer_index)? = grouped;
    }
    Some(())
}

/// The number of pairs of links that cross between each two neighbouring layers of
/// `layers`, given the ranks of the slots in `position`.
fn total_crossings(
    layers: &[Vec<usize>],
    parents: &[Vec<usize>],
    position: &[usize],
) -> Option<usize> {
    layers.iter().skip(1).map(|layer| crossings(layer, parents, position)).sum()
}

/// The slots of `layer` with those of each subgraph moved up to the first of them, so
/// that no other slot lies between them, and those of a nested subgraph likewise within
/// the run of the one enclosing it, in the order of `layer` otherwise. `chain_of` lists
/// the subgraphs each slot lies in, outermost first.
fn group_subgraph_members(layer: &[usize], chain_of: &[Vec<usize>]) -> Option<Vec<usize>> {
    if layer.iter().all(|&slot| chain_of.get(slot).is_some_and(Vec::is_empty)) {
        return Some(layer.to_vec());
    }
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
