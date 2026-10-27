//! The layer of each node along the flow, dagre's `rank`: the pairs of nodes closing a
//! cycle are reversed, then network simplex (see
//! [`network_simplex_layers`](super::simplex::network_simplex_layers)) puts each node where
//! the links together span the fewest layers.

use super::simplex::Constraint;
use crate::diagram::flowchart::chart::{Edge, End, Flowchart};

/// Bounds the pairs of nodes the links order (see [`member_pairs`]). A link between two
/// subgraphs orders every member of one against every member of the other, so the pairs
/// grow with the product of their sizes times the number of links: two subgraphs of a few
/// thousand members with a few hundred links between them fit in Mermaid's `maxTextSize`
/// and would ask for billions of pairs, more memory than a machine has. A million pairs
/// take 16 MB, and the marks and constraints made from them a few times that, which a
/// preview can afford, while no chart a reader could follow comes near it. The bound is
/// this crate's own, not upstream's, which ties each such link to one member of either
/// subgraph instead: a terminal drawing must bound its memory and time even when no
/// width limits it.
const MAX_MEMBER_PAIRS: usize = 1_000_000;

/// The pairs of nodes each link of `chart` orders along the flow (see [`link_order`]),
/// indexed like [`Flowchart::edges`]. `None` when a link joins a subgraph to one of its
/// own members, or when the links would order more than [`MAX_MEMBER_PAIRS`] pairs,
/// which is counted before any pair is made.
pub(super) fn member_pairs(chart: &Flowchart) -> Option<Vec<Vec<(usize, usize)>>> {
    let count = chart.edges.iter().try_fold(0_usize, |count, edge| {
        if edge.frame_loop().is_some() {
            return Some(count);
        }
        let (sources, targets) = (nodes_at(chart, &edge.from)?, nodes_at(chart, &edge.to)?);
        count.checked_add(sources.len().checked_mul(targets.len())?)
    })?;
    if count > MAX_MEMBER_PAIRS {
        return None;
    }
    chart.edges.iter().map(|edge| link_order(chart, edge)).collect()
}

/// Marks the pairs of nodes of `chart`'s links, `pairs` as [`member_pairs`] lists them, to
/// lay out reversed so that no cycle is left, indexed like `pairs`, as dagre's `dfsFAS`
/// picks them: a depth-first search from each node in turn follows the pairs leaving a
/// node in the order of their links, and reverses a pair whose target is still on the
/// search's path. Only such back pairs are reversed, so no new cycle forms. A reversed
/// pair holds its position among the reversed pairs, in the order the search reverses
/// them; a kept pair holds `None`. A self loop orders nothing and is never reversed.
/// `None` when a pair names a node outside the chart.
///
/// Upstream reroutes a link to or from a subgraph to one anchor member and reverses that
/// single edge; here the link stands in the search for every member pair, and a link
/// whose pairs disagree is drawn in the direction of its unreversed pairs (see
/// [`reversed_links`]), as one line cannot run both ways.
pub(super) fn cycle_closing_edges(
    chart: &Flowchart,
    pairs: &[Vec<(usize, usize)>],
) -> Option<Vec<Vec<Option<usize>>>> {
    // The pairs leaving each node, as (link, pair, target).
    let mut leaving = vec![Vec::new(); chart.nodes.len()];
    for (link, pairs) in pairs.iter().enumerate() {
        for (pair, &(from, to)) in pairs.iter().enumerate() {
            if from != to {
                leaving.get_mut(from)?.push((link, pair, to));
            }
        }
    }
    let mut closing: Vec<Vec<Option<usize>>> =
        pairs.iter().map(|pairs| vec![None; pairs.len()]).collect();
    let mut reversed = 0;
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
                *closing.get_mut(link)?.get_mut(pair)? = Some(reversed);
                reversed += 1;
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
pub(super) fn reversed_links(closing: &[Vec<Option<usize>>]) -> Vec<bool> {
    closing.iter().map(|pairs| !pairs.is_empty() && pairs.iter().all(Option::is_some)).collect()
}

/// The pairs of nodes `(from, to)` whose order along the flow `edge` imposes, in its
/// written direction: a link between nodes orders its ends; a link into a subgraph
/// puts every member after the source, a link out of one puts the target after every
/// member, and a link between two puts every member of the second after every member
/// of the first. A link from a subgraph to itself orders nothing, as a node's self loop
/// orders nothing. `None` when the link joins a subgraph to one of its own members,
/// which has no layout here.
fn link_order(chart: &Flowchart, edge: &Edge) -> Option<Vec<(usize, usize)>> {
    if let Some(pair) = edge.nodes() {
        return Some(vec![pair]);
    }
    if edge.from == edge.to {
        return Some(Vec::new());
    }
    let (sources, targets) = (nodes_at(chart, &edge.from)?, nodes_at(chart, &edge.to)?);
    let mut pairs = Vec::with_capacity(sources.len() * targets.len());
    for &from in sources {
        for &to in targets {
            if from == to {
                return None;
            }
            pairs.push((from, to));
        }
    }
    Some(pairs)
}

/// The nodes a link meets at `end`: the node itself, or a subgraph's members.
fn nodes_at<'a>(chart: &'a Flowchart, end: &'a End) -> Option<&'a [usize]> {
    match end {
        End::Node(node) => Some(std::slice::from_ref(node)),
        End::Subgraph(subgraph) => Some(&chart.subgraphs.get(*subgraph)?.members),
    }
}

/// The pairs of nodes whose layers `chart`'s links impose, `pairs` as [`member_pairs`]
/// lists them, each pair in layout order, those marked in `reversed` (see
/// [`cycle_closing_edges`]) turned round. The pairs come in the order dagre's graph holds
/// its edges, which breaks ties when it ranks them: those kept in the order of their
/// links, then those reversed in the order they were reversed, as `acyclic.run` removes
/// each reversed edge and adds it back last.
///
/// With subgraphs, upstream's `nestingGraph.run` also ranks a top and a bottom border
/// node for each subgraph, linked to its members by heavy edges that keep it compact.
/// The frames here are drawn around their members' layers rather than laid out as nodes,
/// so only the member pairs are constrained, and the layers in and around a subgraph may
/// differ from upstream's.
pub(super) fn layering_constraints(
    chart: &Flowchart,
    pairs: &[Vec<(usize, usize)>],
    reversed: &[Vec<Option<usize>>],
) -> Vec<Constraint> {
    let mut kept = Vec::new();
    let mut turned = Vec::new();
    for ((edge, pairs), reversed) in chart.edges.iter().zip(pairs).zip(reversed) {
        for (&(from, to), &reversed) in pairs.iter().zip(reversed) {
            match reversed {
                Some(order) => {
                    turned.push((order, Constraint { from: to, to: from, length: edge.length }));
                }
                None => kept.push(Constraint { from, to, length: edge.length }),
            }
        }
    }
    turned.sort_by_key(|&(order, _)| order);
    kept.extend(turned.into_iter().map(|(_, constraint)| constraint));
    kept
}
