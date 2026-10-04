//! Subgraphs laid out in a direction of their own. Upstream lays such a subgraph out
//! as a diagram of its own and embeds it in the enclosing layout as one node; here it
//! is drawn on its own, frame included, and stands in the enclosing layout as a box of
//! that drawing's size.

use crate::diagram::Failure;
use crate::style::Line;

use super::parse::{Body, Edge, End, Flowchart, Node};
use super::render_chart;

/// Replaces each outermost subgraph that is laid out in a direction of its own (see
/// [`own_direction`]) by one node holding its drawing: its members, the links among
/// them and the subgraphs nested in it are drawn as a diagram of their own at the
/// default spacing, and links to the subgraph end at that node. A link between one of
/// its members or nested subgraphs and anything outside it has no such node to end at,
/// so the chart is unsupported then.
pub(super) fn embed_units(chart: Flowchart) -> Result<Flowchart, Failure> {
    let own: Vec<bool> =
        (0..chart.subgraphs.len()).map(|subgraph| own_direction(&chart, subgraph)).collect();
    let roots: Vec<usize> = (0..chart.subgraphs.len())
        .filter(|&subgraph| {
            own.get(subgraph) == Some(&true)
                && !chart.enclosing(subgraph).iter().any(|&outer| own.get(outer) == Some(&true))
        })
        .collect();
    if roots.is_empty() {
        return Ok(chart);
    }
    let mut node_unit = vec![None; chart.nodes.len()];
    for (unit, &subgraph) in roots.iter().enumerate() {
        for &member in &chart.subgraphs.get(subgraph).ok_or(Failure::Unsupported)?.members {
            *node_unit.get_mut(member).ok_or(Failure::Unsupported)? = Some(unit);
        }
    }
    let subgraph_unit = (0..chart.subgraphs.len())
        .map(|subgraph| {
            roots
                .iter()
                .position(|&unit| unit == subgraph || chart.enclosing(subgraph).contains(&unit))
        })
        .collect();
    let units = Units { roots, node_unit, subgraph_unit };
    let drawings = (0..units.roots.len())
        .map(|unit| draw_unit(&chart, &units, unit))
        .collect::<Result<Vec<_>, _>>()?;
    outer_chart(chart, &units, drawings)
}

/// The outermost subgraphs laid out in a direction of their own, numbered as units, and
/// the unit each node and subgraph lies in.
struct Units {
    /// The subgraph each unit is.
    roots: Vec<usize>,
    /// The unit each node lies in, indexed like [`Flowchart::nodes`].
    node_unit: Vec<Option<usize>>,
    /// The unit each subgraph lies in, a unit lying in itself, indexed like
    /// [`Flowchart::subgraphs`].
    subgraph_unit: Vec<Option<usize>>,
}

impl Units {
    /// The unit an end lies inside of, which a link to the unit itself does not.
    fn inside(&self, end: End) -> Option<usize> {
        match end {
            End::Node(node) => self.node_unit.get(node).copied().flatten(),
            End::Subgraph(subgraph) => self
                .subgraph_unit
                .get(subgraph)
                .copied()
                .flatten()
                .filter(|&unit| self.roots.get(unit) != Some(&subgraph)),
        }
    }
}

/// Draws `unit` of `chart` as a diagram of its own: its nodes, the subgraphs lying in
/// it with its own frame outermost, and the links with both ends inside it, laid out
/// in the unit's direction.
fn draw_unit(chart: &Flowchart, units: &Units, unit: usize) -> Result<Vec<Line>, Failure> {
    let root = *units.roots.get(unit).ok_or(Failure::Unsupported)?;
    // The unit's nodes and subgraphs, numbered within it in their order.
    let mut node_index = vec![None; chart.nodes.len()];
    let mut nodes = Vec::new();
    for (index, node) in chart.nodes.iter().enumerate() {
        if units.node_unit.get(index) == Some(&Some(unit)) {
            *node_index.get_mut(index).ok_or(Failure::Unsupported)? = Some(nodes.len());
            nodes.push(node.clone());
        }
    }
    let mut subgraph_index = vec![None; chart.subgraphs.len()];
    let mut held = Vec::new();
    for index in 0..chart.subgraphs.len() {
        if units.subgraph_unit.get(index) == Some(&Some(unit)) {
            *subgraph_index.get_mut(index).ok_or(Failure::Unsupported)? = Some(held.len());
            held.push(index);
        }
    }
    let renumber = |indices: &[Option<usize>], index: usize| {
        indices.get(index).copied().flatten().ok_or(Failure::Unsupported)
    };
    let mut direction = chart.direction;
    let mut subgraphs = Vec::with_capacity(held.len());
    for index in held {
        let mut subgraph = chart.subgraphs.get(index).ok_or(Failure::Unsupported)?.clone();
        for member in &mut subgraph.members {
            *member = renumber(&node_index, *member)?;
        }
        if index == root {
            // The unit's own frame is the outermost one of its drawing.
            direction = subgraph.direction.take().unwrap_or(chart.direction);
            subgraph.parent = None;
        } else {
            subgraph.parent =
                subgraph.parent.map(|parent| renumber(&subgraph_index, parent)).transpose()?;
        }
        subgraphs.push(subgraph);
    }
    let end_in = |end: End| match end {
        End::Node(node) => Ok(End::Node(renumber(&node_index, node)?)),
        End::Subgraph(subgraph) => Ok(End::Subgraph(renumber(&subgraph_index, subgraph)?)),
    };
    let mut edges = Vec::new();
    for edge in &chart.edges {
        if units.inside(edge.from) != Some(unit) || units.inside(edge.to) != Some(unit) {
            continue;
        }
        let mut edge = edge.clone();
        edge.from = end_in(edge.from)?;
        edge.to = end_in(edge.to)?;
        edges.push(edge);
    }
    render_chart(Flowchart { direction, nodes, edges, subgraphs }, None)
}

/// `chart` with each unit replaced by one node holding its drawing from `drawings`,
/// indexed by unit: outside the units, every node keeps its place, and each unit's node
/// takes the place of its first member; links to a unit end at its node.
fn outer_chart(
    chart: Flowchart,
    units: &Units,
    drawings: Vec<Vec<Line>>,
) -> Result<Flowchart, Failure> {
    let Flowchart { direction, nodes, edges, subgraphs } = chart;
    let mut drawings: Vec<Option<Vec<Line>>> = drawings.into_iter().map(Some).collect();
    let mut node_index = vec![None; nodes.len()];
    let mut unit_node = vec![None; units.roots.len()];
    let mut outer_nodes = Vec::new();
    for (index, node) in nodes.into_iter().enumerate() {
        match units.node_unit.get(index).copied().flatten() {
            None => {
                *node_index.get_mut(index).ok_or(Failure::Unsupported)? = Some(outer_nodes.len());
                outer_nodes.push(node);
            }
            Some(unit) if unit_node.get(unit) == Some(&None) => {
                *unit_node.get_mut(unit).ok_or(Failure::Unsupported)? = Some(outer_nodes.len());
                let drawing =
                    drawings.get_mut(unit).and_then(Option::take).ok_or(Failure::Unsupported)?;
                outer_nodes.push(Node { body: Body::Drawing(drawing), spread: 0 });
            }
            Some(_) => {}
        }
    }
    let node_at = |node: usize| match units.node_unit.get(node).copied().flatten() {
        Some(unit) => unit_node.get(unit).copied().flatten(),
        None => node_index.get(node).copied().flatten(),
    };
    let unit_of = |subgraph: usize| units.subgraph_unit.get(subgraph).copied().flatten();
    let mut subgraph_index = vec![None; subgraphs.len()];
    let mut kept = 0;
    for (index, slot) in subgraph_index.iter_mut().enumerate() {
        if unit_of(index).is_none() {
            *slot = Some(kept);
            kept += 1;
        }
    }
    let mut outer_subgraphs = Vec::with_capacity(kept);
    for (index, mut subgraph) in subgraphs.into_iter().enumerate() {
        if unit_of(index).is_some() {
            continue;
        }
        let mut members = Vec::with_capacity(subgraph.members.len());
        for &member in &subgraph.members {
            let member = node_at(member).ok_or(Failure::Unsupported)?;
            if !members.contains(&member) {
                members.push(member);
            }
        }
        subgraph.members = members;
        subgraph.parent = subgraph
            .parent
            .map(|parent| subgraph_index.get(parent).copied().flatten().ok_or(Failure::Unsupported))
            .transpose()?;
        outer_subgraphs.push(subgraph);
    }
    let end_at = |end: End| match end {
        End::Node(node) => node_at(node).map(End::Node),
        End::Subgraph(subgraph) => match unit_of(subgraph) {
            Some(unit) => unit_node.get(unit).copied().flatten().map(End::Node),
            None => subgraph_index.get(subgraph).copied().flatten().map(End::Subgraph),
        },
    };
    let mut outer_edges = Vec::new();
    for mut edge in edges {
        match (units.inside(edge.from), units.inside(edge.to)) {
            // Drawn in the unit's own drawing.
            (Some(from), Some(to)) if from == to => continue,
            (None, None) => {}
            // An edge with one end inside a unit and the other outside it has no cell to
            // start or end at in the unit's drawing.
            _ => return Err(Failure::Unsupported),
        }
        edge.from = end_at(edge.from).ok_or(Failure::Unsupported)?;
        edge.to = end_at(edge.to).ok_or(Failure::Unsupported)?;
        outer_edges.push(edge);
    }
    Ok(Flowchart { direction, nodes: outer_nodes, edges: outer_edges, subgraphs: outer_subgraphs })
}

/// Whether `subgraph` is laid out in the direction of its own `direction` statement:
/// it has one, and none of its members is linked to a node outside it. Otherwise it
/// inherits the enclosing direction, as upstream does ("If any of a subgraph's nodes
/// are linked to the outside, subgraph direction will be ignored").
fn own_direction(chart: &Flowchart, subgraph: usize) -> bool {
    let Some(subgraph) = chart.subgraphs.get(subgraph) else { return false };
    subgraph.direction.is_some()
        && chart
            .edges
            .iter()
            .filter_map(Edge::nodes)
            .all(|(from, to)| subgraph.members.contains(&from) == subgraph.members.contains(&to))
}
