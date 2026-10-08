//! Collapsed subgraphs drawn as one box, as upstream's collapsible subgraphs are
//! (<https://mermaid.js.org/syntax/flowchart.html>, "Collapsible subgraphs").

use super::parse::{
    Body, Edge, End, Flowchart, Node, Shape, Subgraph, compact_index, remap_members,
};

/// `chart` with each subgraph marked [`Subgraph::collapsed`] drawn as one rectangle
/// showing its title: its members and the subgraphs nested in it are not drawn, a link
/// with an end among them attaches to the box instead, and a link with both ends among
/// them is dropped. Of nested collapsed subgraphs, the outermost one's box stands for
/// everything inside it. The box takes the place of the first member in the order of
/// nodes, for the layout to place it where the members were. `None` when an index in
/// `chart` is out of range.
pub(super) fn collapse(chart: Flowchart) -> Option<Flowchart> {
    let outermost = outermost_collapsed(&chart);
    if outermost.iter().all(Option::is_none) {
        return Some(chart);
    }
    let enclosing: Vec<Vec<usize>> =
        (0..chart.subgraphs.len()).map(|subgraph| chart.enclosing(subgraph).collect()).collect();
    let Flowchart { direction, nodes, edges, subgraphs } = chart;
    let placed = collapse_nodes(nodes, &subgraphs, &outermost)?;
    // The index each subgraph keeps among those no collapsed subgraph holds.
    let subgraph_index =
        compact_index(outermost.len(), |subgraph| outermost.get(subgraph) == Some(&None));
    let edges = collapse_edges(edges, &placed, &outermost, &subgraph_index)?;
    let subgraphs =
        collapse_subgraphs(&subgraphs, &placed, &outermost, &subgraph_index, &enclosing)?;
    Some(Flowchart { direction, nodes: placed.nodes, edges, subgraphs })
}

/// The outermost collapsed subgraph each subgraph lies in, itself included, indexed like
/// [`Flowchart::subgraphs`].
fn outermost_collapsed(chart: &Flowchart) -> Vec<Option<usize>> {
    (0..chart.subgraphs.len())
        .map(|subgraph| {
            // enclosing runs from the parent outwards, so the last collapsed holder is
            // the outermost.
            std::iter::once(subgraph)
                .chain(chart.enclosing(subgraph))
                .filter(|&holder| {
                    chart.subgraphs.get(holder).is_some_and(|holder| holder.collapsed)
                })
                .last()
        })
        .collect()
}

/// Where the nodes went once collapsed subgraphs became boxes.
struct Placed {
    /// The nodes kept, each collapsed subgraph's box among them.
    nodes: Vec<Node>,
    /// The index among `nodes` of each node of the chart, the box standing for it when
    /// it was hidden.
    node_index: Vec<usize>,
    /// The index among `nodes` of each collapsed subgraph's box, indexed like
    /// [`Flowchart::subgraphs`]; `None` for a subgraph that has no box.
    box_index: Vec<Option<usize>>,
    /// The collapsed subgraph whose box stands for each node of the chart, if any.
    hidden_in: Vec<Option<usize>>,
}

/// The nodes kept, each collapsed subgraph's box in the place of its first member, or
/// after every node when it has none.
fn collapse_nodes(
    nodes: Vec<Node>,
    subgraphs: &[Subgraph],
    outermost: &[Option<usize>],
) -> Option<Placed> {
    let mut hidden_in = vec![None; nodes.len()];
    for (subgraph, holder) in subgraphs.iter().zip(outermost) {
        if let Some(holder) = *holder {
            for &member in &subgraph.members {
                *hidden_in.get_mut(member)? = Some(holder);
            }
        }
    }
    let box_node = |subgraph: usize| {
        let subgraph = subgraphs.get(subgraph)?;
        Some(Node {
            body: Body::Box { label: subgraph.title.clone(), shape: Shape::Rectangle },
            spread: 0,
            styling: subgraph.styling,
        })
    };
    let mut box_index = vec![None; subgraphs.len()];
    let mut node_index = Vec::with_capacity(nodes.len());
    let mut kept = Vec::new();
    for (node, holder) in nodes.into_iter().zip(&hidden_in) {
        let index = match *holder {
            Some(holder) => match *box_index.get(holder)? {
                Some(index) => index,
                None => {
                    kept.push(box_node(holder)?);
                    *box_index.get_mut(holder)? = Some(kept.len() - 1);
                    kept.len() - 1
                }
            },
            None => {
                kept.push(node);
                kept.len() - 1
            }
        };
        node_index.push(index);
    }
    for (subgraph, holder) in outermost.iter().enumerate() {
        if *holder == Some(subgraph) && box_index.get(subgraph)?.is_none() {
            kept.push(box_node(subgraph)?);
            *box_index.get_mut(subgraph)? = Some(kept.len() - 1);
        }
    }
    Some(Placed { nodes: kept, node_index, box_index, hidden_in })
}

/// The links with their ends moved onto the boxes that stand for them, without those
/// whose ends both went into the same box; `subgraph_index` gives each subgraph's index
/// among those no collapsed subgraph holds.
fn collapse_edges(
    edges: Vec<Edge>,
    placed: &Placed,
    outermost: &[Option<usize>],
    subgraph_index: &[Option<usize>],
) -> Option<Vec<Edge>> {
    // Where an end goes, and whether it went into a box.
    let moved = |end: End| -> Option<(End, bool)> {
        match end {
            End::Node(node) => Some((
                End::Node(*placed.node_index.get(node)?),
                placed.hidden_in.get(node)?.is_some(),
            )),
            End::Subgraph(subgraph) => match *outermost.get(subgraph)? {
                Some(holder) => Some((End::Node((*placed.box_index.get(holder)?)?), true)),
                None => Some((End::Subgraph((*subgraph_index.get(subgraph)?)?), false)),
            },
        }
    };
    let mut kept = Vec::with_capacity(edges.len());
    for edge in edges {
        let ((from, from_moved), (to, to_moved)) = (moved(edge.from)?, moved(edge.to)?);
        if from_moved && to_moved && from == to {
            continue;
        }
        kept.push(Edge { from, to, ..edge });
    }
    Some(kept)
}

/// The subgraphs no collapsed subgraph holds, their members renumbered, each holding the
/// boxes of the collapsed subgraphs nested in it; `subgraph_index` is as for
/// [`collapse_edges`].
fn collapse_subgraphs(
    subgraphs: &[Subgraph],
    placed: &Placed,
    outermost: &[Option<usize>],
    subgraph_index: &[Option<usize>],
    enclosing: &[Vec<usize>],
) -> Option<Vec<Subgraph>> {
    // The boxes each subgraph holds, also those of collapsed subgraphs that had no
    // members to stand for.
    let mut boxes_in = vec![Vec::new(); subgraphs.len()];
    for (inner, holder) in outermost.iter().enumerate() {
        if *holder == Some(inner)
            && let Some(box_index) = *placed.box_index.get(inner)?
        {
            for &holder in enclosing.get(inner)? {
                boxes_in.get_mut(holder)?.push(box_index);
            }
        }
    }
    let mut kept = Vec::new();
    for ((subgraph, holder), boxes) in subgraphs.iter().zip(outermost).zip(&boxes_in) {
        if holder.is_some() {
            continue;
        }
        let mut members =
            remap_members(&subgraph.members, |member| placed.node_index.get(member).copied())?;
        for &box_index in boxes {
            if !members.contains(&box_index) {
                members.push(box_index);
            }
        }
        let parent = match subgraph.parent {
            Some(parent) => Some((*subgraph_index.get(parent)?)?),
            None => None,
        };
        kept.push(Subgraph { members, parent, ..subgraph.clone() });
    }
    Some(kept)
}
