//! The layer of each node along the flow, dagre's `rank`: the links closing a cycle are
//! reversed, then each node goes in the first layer its incoming links allow.

use crate::diagram::flowchart::parse::{Edge, End, Flowchart};

/// Marks the links that would close a cycle with the links declared before them, each
/// link standing for the order it imposes on the nodes (see [`link_order`]); a marked
/// link is laid out reversed, which leaves no cycle. Earlier links keep their
/// direction, so a cycle is broken at its last declared link. Upstream's dagre picks the
/// links by a depth-first search over its own internal graph, whose node order a text
/// renderer cannot reproduce exactly; declaration order reverses the same links in the
/// cyclic examples of Mermaid's flowchart documentation
/// (<https://mermaid.js.org/syntax/flowchart.html>). `None` when a link joins a subgraph
/// to itself or to one of its own members.
pub(super) fn cycle_closing_edges(chart: &Flowchart) -> Option<Vec<bool>> {
    let mut children = vec![Vec::new(); chart.nodes.len()];
    let mut closing = Vec::with_capacity(chart.edges.len());
    for edge in &chart.edges {
        // A self loop orders nothing.
        let pairs: Vec<(usize, usize)> =
            link_order(chart, edge)?.into_iter().filter(|(from, to)| from != to).collect();
        // Every source of the link precedes every target, so the link closes a cycle
        // when any source can already be reached from any target.
        let mut reached = vec![false; chart.nodes.len()];
        let mut stack: Vec<usize> = pairs.iter().map(|&(_, to)| to).collect();
        while let Some(node) = stack.pop() {
            let seen = reached.get_mut(node)?;
            if !*seen {
                *seen = true;
                stack.extend(children.get(node)?);
            }
        }
        let closes = pairs.iter().any(|&(from, _)| reached.get(from) == Some(&true));
        for (from, to) in pairs {
            let (from, to) = if closes { (to, from) } else { (from, to) };
            children.get_mut(from)?.push(to);
        }
        closing.push(closes);
    }
    Some(closing)
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

/// The pairs of nodes whose layers the links impose (see [`link_order`]), each link in
/// layout order, `reversed` closing a cycle. `None` when a link joins a subgraph to
/// itself or to one of its own members.
pub(super) fn layering_constraints(
    chart: &Flowchart,
    reversed: &[bool],
) -> Option<Vec<Constraint>> {
    let mut constraints = Vec::new();
    for (edge, &reversed) in chart.edges.iter().zip(reversed) {
        for (from, to) in link_order(chart, edge)? {
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
