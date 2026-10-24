//! The layer of each node along the flow, dagre's `rank`: the pairs of nodes closing a
//! cycle are reversed, then each node goes in the first layer its incoming links allow.

use crate::diagram::flowchart::parse::{Edge, End, Flowchart};

/// Marks the pairs of nodes (see [`link_order`]) to lay out reversed so that no cycle
/// is left, indexed like [`Flowchart::edges`] and then like the link's pairs, as dagre's
/// `dfsFAS` picks them: a depth-first search from each node in turn follows the pairs
/// leaving a node in the order of their links, and reverses a pair whose target is
/// still on the search's path. Only such back pairs are reversed, so no new cycle forms.
/// A self loop orders nothing and is never reversed. `None` when a link joins a subgraph
/// to itself or to one of its own members.
///
/// Upstream reroutes a link to or from a subgraph to one anchor member and reverses that
/// single edge; here the link stands in the search for every member pair, and a link
/// whose pairs disagree is drawn in the direction of its unreversed pairs (see
/// [`reversed_links`]), as one line cannot run both ways.
pub(super) fn cycle_closing_edges(chart: &Flowchart) -> Option<Vec<Vec<bool>>> {
    let pairs =
        chart.edges.iter().map(|edge| link_order(chart, edge)).collect::<Option<Vec<_>>>()?;
    // The pairs leaving each node, as (link, pair, target).
    let mut leaving = vec![Vec::new(); chart.nodes.len()];
    for (link, pairs) in pairs.iter().enumerate() {
        for (pair, &(from, to)) in pairs.iter().enumerate() {
            if from != to {
                leaving.get_mut(from)?.push((link, pair, to));
            }
        }
    }
    let mut closing: Vec<Vec<bool>> = pairs.iter().map(|pairs| vec![false; pairs.len()]).collect();
    // Upstream's graph lists the nodes in the order they first appear, those with an
    // array-index id first.
    let mut roots: Vec<usize> = (0..chart.nodes.len()).collect();
    roots.sort_by_key(|&node| {
        let index = chart.nodes.get(node).and_then(|node| node.array_index);
        (index.is_none(), index)
    });
    let mut visited = vec![false; chart.nodes.len()];
    let mut on_path = vec![false; chart.nodes.len()];
    for root in roots {
        if *visited.get(root)? {
            continue;
        }
        // Each node on the search's path, with the next of its pairs to follow.
        let mut path = vec![(root, 0)];
        *visited.get_mut(root)? = true;
        *on_path.get_mut(root)? = true;
        while let Some((node, next)) = path.last_mut() {
            let node = *node;
            let Some(&(link, pair, to)) = leaving.get(node)?.get(*next) else {
                *on_path.get_mut(node)? = false;
                path.pop();
                continue;
            };
            *next += 1;
            if *on_path.get(to)? {
                *closing.get_mut(link)?.get_mut(pair)? = true;
            } else if !*visited.get(to)? {
                *visited.get_mut(to)? = true;
                *on_path.get_mut(to)? = true;
                path.push((to, 0));
            }
        }
    }
    Some(closing)
}

/// Whether each link is laid out from its target, given its reversed pairs as
/// [`cycle_closing_edges`] marks them: only when every pair of the link is reversed.
pub(super) fn reversed_links(closing: &[Vec<bool>]) -> Vec<bool> {
    closing
        .iter()
        .map(|pairs| !pairs.is_empty() && pairs.iter().all(|&reversed| reversed))
        .collect()
}

/// The pairs of nodes `(from, to)` whose order along the flow `edge` imposes, in its
/// written direction: a link between nodes orders its ends; a link into a subgraph
/// puts every member after the source, a link out of one puts the target after every
/// member, and a link between two puts every member of the second after every member
/// of the first. `None` when the link joins a subgraph to itself or to one of its own
/// members, which has no layout here.
fn link_order(chart: &Flowchart, edge: &Edge) -> Option<Vec<(usize, usize)>> {
    if let Some(pair) = edge.nodes() {
        return Some(vec![pair]);
    }
    let nodes_at = |end: End| -> Option<Vec<usize>> {
        match end {
            End::Node(node) => Some(vec![node]),
            End::Subgraph(subgraph) => Some(chart.subgraphs.get(subgraph)?.members.clone()),
        }
    };
    let mut pairs = Vec::new();
    for from in nodes_at(edge.from)? {
        for to in nodes_at(edge.to)? {
            if from == to {
                return None;
            }
            pairs.push((from, to));
        }
    }
    Some(pairs)
}

/// The order a link imposes on the layers of two nodes.
#[derive(Clone, Copy, Debug)]
pub(super) struct Constraint {
    /// The node the link leaves.
    pub from: usize,
    /// The node the link enters, at least `length` layers after `from`.
    pub to: usize,
    /// The link's length in layers.
    pub length: usize,
}

/// The pairs of nodes whose layers the links impose (see [`link_order`]), each pair in
/// layout order, those marked in `reversed` (see [`cycle_closing_edges`]) turned round.
/// `None` when a link joins a subgraph to itself or to one of its own members.
pub(super) fn layering_constraints(
    chart: &Flowchart,
    reversed: &[Vec<bool>],
) -> Option<Vec<Constraint>> {
    let mut constraints = Vec::new();
    for (edge, reversed) in chart.edges.iter().zip(reversed) {
        for ((from, to), &reversed) in link_order(chart, edge)?.into_iter().zip(reversed) {
            let (from, to) = if reversed { (to, from) } else { (from, to) };
            constraints.push(Constraint { from, to, length: edge.length });
        }
    }
    Some(constraints)
}

/// The layer of each of `count` nodes: a node with no incoming constraint takes layer 0,
/// every other node the largest `parent layer + length` over its constraints. A
/// constraint from a node to itself is ignored. `None` when the constraints form a cycle
/// or name a node outside `0..count`.
pub(super) fn longest_path_layers(count: usize, constraints: &[Constraint]) -> Option<Vec<usize>> {
    let mut unplaced_parents = vec![0_usize; count];
    let mut children = vec![Vec::new(); count];
    for constraint in constraints.iter().filter(|c| c.from != c.to) {
        children.get_mut(constraint.from)?.push((constraint.to, constraint.length));
        *unplaced_parents.get_mut(constraint.to)? += 1;
    }
    let mut layer_of = vec![0_usize; count];
    let mut ready: Vec<usize> =
        (0..count).filter(|&node| unplaced_parents.get(node) == Some(&0)).collect();
    let mut placed = 0;
    while let Some(node) = ready.pop() {
        placed += 1;
        let layer = *layer_of.get(node)?;
        for &(child, length) in children.get(node)? {
            let child_layer = layer_of.get_mut(child)?;
            *child_layer = (*child_layer).max(layer.saturating_add(length));
            let unplaced = unplaced_parents.get_mut(child)?;
            *unplaced -= 1;
            if *unplaced == 0 {
                ready.push(child);
            }
        }
    }
    (placed == count).then_some(layer_of)
}
