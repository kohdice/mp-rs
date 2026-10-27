//! Links between a subgraph and what lies inside it, which upstream draws no line for.

use crate::diagram::Failure;

use super::chart::{End, Flowchart};

/// `chart` without the links between a subgraph and a node or subgraph inside it, at any
/// depth, in either direction; unsupported when such a link has a label.
///
/// Upstream draws no line for such a link: `adjustClustersAndEdges` (in
/// `rendering-util/layout-algorithms/dagre/mermaid-graphlib.js`) moves a link's subgraph
/// end onto a node inside the subgraph, and the edge renderer cuts the path where it
/// crosses that subgraph's border, which leaves nothing of a path lying wholly inside it.
/// The layout still places the moved link, so a browser keeps room for it — a loop below
/// the node, or a member pushed one layer on; that room follows from the node upstream
/// picks, not from anything in the source, and is not kept here. A label still shows in
/// a browser, against the node's border with no line under it; with no line to carry it
/// here either, the chart falls back rather than dropping or floating the text, as it does
/// for a labelled `~~~` link.
pub(super) fn drop_own_frame_links(chart: Flowchart) -> Result<Flowchart, Failure> {
    let Flowchart { direction, spelled_br, nodes, edges, subgraphs } = chart;
    let chart = Flowchart { direction, spelled_br, nodes, edges: Vec::new(), subgraphs };
    let edges = edges
        .into_iter()
        .filter_map(|edge| {
            if !(holds(&chart, edge.from, edge.to) || holds(&chart, edge.to, edge.from)) {
                Some(Ok(edge))
            } else if edge.label.is_some() {
                Some(Err(Failure::Unsupported))
            } else {
                None
            }
        })
        .collect::<Result<_, _>>()?;
    Ok(Flowchart { edges, ..chart })
}

/// Whether `outer` is a subgraph and `inner` a node among its members or a subgraph
/// nested in it, at any depth.
fn holds(chart: &Flowchart, outer: End, inner: End) -> bool {
    let End::Subgraph(outer) = outer else {
        return false;
    };
    match inner {
        End::Node(node) => {
            chart.subgraphs.get(outer).is_some_and(|outer| outer.members.contains(&node))
        }
        End::Subgraph(inner) => chart.encloses(outer, inner),
    }
}
