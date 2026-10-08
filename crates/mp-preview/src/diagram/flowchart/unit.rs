//! Subgraphs laid out in a direction of their own. Upstream lays such a subgraph out
//! as a diagram of its own and embeds it in the enclosing layout as one node; here it
//! is drawn on its own, frame included, and stands in the enclosing layout as a box
//! holding that drawing. Upstream's curved edges enter the embedded cluster at any point
//! of its border; here they meet the box's border at port cells, the only places a
//! line of glyphs can end, and the box grows for them as a node's box does. In a
//! vertical layout those ports keep clear of the frame's title, and the box grows wider
//! when they need the room; a frame drawn within the enclosing chart instead moves its
//! title clear of the links crossing it.

use crate::diagram::Failure;
use crate::style::Line;

use super::parse::{Body, Direction, End, Flowchart, Node, compact_index, remap_members};
use super::styling::Styling;

/// The most subgraphs with a direction of their own a chart is drawn inside of. Each
/// one is drawn by a call nested in the call drawing the chart around it, and a source at
/// Mermaid's `maxTextSize` of 50,000 characters can nest thousands of them (`subgraph`
/// and `end` on lines of their own take 13 characters, and every non-empty subgraph
/// with no link to the outside gets a direction of its own), which would overflow the
/// stack and abort the process; 32 is far past what a reader follows in a terminal.
const MAX_UNIT_DEPTH: usize = 32;

/// Replaces each outermost subgraph that is laid out in a direction of its own (see
/// [`own_direction`]) by one node holding its drawing: its members, the links among
/// them and the subgraphs nested in it are drawn as a diagram of their own, and links
/// to the subgraph end at that node. `frame` is the subgraph whose drawing `chart` is,
/// which is laid out in the chart's direction. Each drawing is made at the spacing of
/// tightening step `level` (see [`Axis::spacing`](super::layout::Axis::spacing)), the
/// step the chart around it is to be drawn at, so the two tighten together.
///
/// The chart is unsupported when a link joins one of the subgraph's members or nested
/// subgraphs to anything outside it, since that link has no cell to end at in the
/// drawing. It is also unsupported when a subgraph with a direction of its own lies more
/// than [`MAX_UNIT_DEPTH`] deep, counting itself and the `depth` of them `chart` lies
/// inside of, or when its drawing spans more than [`MAX_CELLS`](super::MAX_CELLS) at
/// step `level`.
pub(super) fn embed_units(
    chart: Flowchart,
    depth: usize,
    frame: Option<usize>,
    level: usize,
) -> Result<Flowchart, Failure> {
    let own: Vec<Option<Direction>> = (0..chart.subgraphs.len())
        .map(|subgraph| {
            if frame == Some(subgraph) {
                None
            } else {
                own_direction(&chart, subgraph, chart.direction)
            }
        })
        .collect();
    let is_own = |subgraph: usize| own.get(subgraph).is_some_and(Option::is_some);
    let roots: Vec<usize> = (0..chart.subgraphs.len())
        .filter(|&subgraph| is_own(subgraph) && !chart.enclosing(subgraph).any(is_own))
        .collect();
    if roots.is_empty() {
        return Ok(chart);
    }
    let depth = depth + 1;
    if depth > MAX_UNIT_DEPTH {
        return Err(Failure::Unsupported);
    }
    let mut node_unit = vec![None; chart.nodes.len()];
    for (unit, &subgraph) in roots.iter().enumerate() {
        for &member in &chart.subgraphs.get(subgraph).ok_or(Failure::Unsupported)?.members {
            *node_unit.get_mut(member).ok_or(Failure::Unsupported)? = Some(unit);
        }
    }
    let subgraph_unit = (0..chart.subgraphs.len())
        .map(|subgraph| {
            roots.iter().position(|&unit| unit == subgraph || chart.encloses(unit, subgraph))
        })
        .collect();
    let units = Units { roots, node_unit, subgraph_unit };
    let drawings = units
        .roots
        .iter()
        .enumerate()
        .map(|(unit, &root)| {
            let direction = own.get(root).copied().flatten().ok_or(Failure::Unsupported)?;
            draw_unit(&chart, &units, unit, direction, depth, level)
        })
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

/// Draws `unit` of `chart` as a chart of its own: its nodes, the subgraphs lying in it
/// with its own frame outermost, and the links with both ends inside it, laid out in
/// `direction` at the spacing of tightening step `level`. `depth` is how many subgraphs
/// with a direction of their own the unit's chart lies inside of.
fn draw_unit(
    chart: &Flowchart,
    units: &Units,
    unit: usize,
    direction: Direction,
    depth: usize,
    level: usize,
) -> Result<Vec<Line>, Failure> {
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
    let mut subgraphs = Vec::with_capacity(held.len());
    for index in held {
        let mut subgraph = chart.subgraphs.get(index).ok_or(Failure::Unsupported)?.clone();
        for member in &mut subgraph.members {
            *member = renumber(&node_index, *member)?;
        }
        if index == root {
            // The unit's own frame is the outermost one of its drawing.
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
    let frame = renumber(&subgraph_index, root)?;
    let chart =
        embed_units(Flowchart { direction, nodes, edges, subgraphs }, depth, Some(frame), level)?;
    super::render_chart(chart, None, None, level)?.ok_or(Failure::Unsupported)
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
                let lines =
                    drawings.get_mut(unit).and_then(Option::take).ok_or(Failure::Unsupported)?;
                let root = units.roots.get(unit).and_then(|&root| subgraphs.get(root));
                let title_width = root.ok_or(Failure::Unsupported)?.title.width();
                outer_nodes.push(Node {
                    body: Body::Drawing { lines, title_width },
                    spread: 0,
                    styling: Styling::default(),
                });
            }
            Some(_) => {}
        }
    }
    let node_at = |node: usize| match units.node_unit.get(node).copied().flatten() {
        Some(unit) => unit_node.get(unit).copied().flatten(),
        None => node_index.get(node).copied().flatten(),
    };
    let unit_of = |subgraph: usize| units.subgraph_unit.get(subgraph).copied().flatten();
    let subgraph_index = compact_index(subgraphs.len(), |index| unit_of(index).is_none());
    let mut outer_subgraphs = Vec::new();
    for (index, mut subgraph) in subgraphs.into_iter().enumerate() {
        if unit_of(index).is_some() {
            continue;
        }
        subgraph.members = remap_members(&subgraph.members, node_at).ok_or(Failure::Unsupported)?;
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

/// The direction `subgraph` is laid out in as a diagram of its own: its `direction`
/// statement's, else the one crosswise to `enclosing`, and `None` — the subgraph stays in
/// the enclosing layout — when it is empty or a link joins something inside it to
/// something outside it. Upstream states the last rule as "If any of a subgraph's nodes
/// are linked to the outside, subgraph direction will be ignored" (`extractor` in
/// `mermaid-graphlib.js`). A link touching the subgraph's own frame is left out of the
/// check: upstream keeps the direction then ("Link *to* subgraph1: subgraph1 direction is
/// maintained" in the same section), as its `isDescendant` does not count a cluster among
/// its own descendants, and a link between a member and its own frame is never drawn
/// anyway.
fn own_direction(chart: &Flowchart, subgraph: usize, enclosing: Direction) -> Option<Direction> {
    let frame = chart.subgraphs.get(subgraph)?;
    // An empty subgraph's only member is the hidden node its frame is drawn around.
    let hidden =
        |&node: &usize| chart.nodes.get(node).is_some_and(|node| matches!(node.body, Body::Hidden));
    if frame.members.iter().all(hidden)
        && chart.subgraphs.iter().all(|other| other.parent != Some(subgraph))
    {
        return None;
    }
    let inside = |end: End| match end {
        End::Node(node) => Some(frame.members.contains(&node)),
        End::Subgraph(other) if other == subgraph => None,
        End::Subgraph(other) => Some(chart.encloses(subgraph, other)),
    };
    let isolated = chart.edges.iter().all(|edge| match (inside(edge.from), inside(edge.to)) {
        (Some(from), Some(to)) => from == to,
        _ => true,
    });
    isolated.then(|| frame.direction.unwrap_or(crosswise(enclosing)))
}

/// The direction upstream lays an isolated subgraph out in when it has no `direction`
/// statement (`rankdir === 'TB' ? 'LR' : 'TB'`).
fn crosswise(enclosing: Direction) -> Direction {
    match enclosing {
        Direction::TopDown => Direction::LeftToRight,
        Direction::LeftToRight | Direction::RightToLeft | Direction::BottomUp => Direction::TopDown,
    }
}
