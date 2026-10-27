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

use super::chart::{Body, Direction, Edge, End, Flowchart, Node, Subgraph, remap_members};
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
/// them and the subgraphs nested in it are drawn by `draw` as a diagram of their own,
/// and links to the subgraph end at that node. `frame` is the subgraph whose drawing
/// `chart` is, which is laid out in the chart's direction. Each drawing is made at the
/// spacing of tightening step `level` (see [`Axis::spacing`](super::layout::Axis::spacing)),
/// the step the chart around it is to be drawn at, so the two tighten together.
///
/// The chart is unsupported when a link joins one of the subgraph's members or nested
/// subgraphs to anything outside it, since that link has no cell to end at in the
/// drawing. It is also unsupported when a subgraph with a direction of its own lies more
/// than [`MAX_UNIT_DEPTH`] deep, counting itself and the `depth` of them `chart` lies
/// inside of. `Ok(None)` when `draw` gives `Ok(None)` for a drawing at step `level`,
/// which a tighter step may still fit.
pub(super) fn embed_units(
    chart: Flowchart,
    depth: usize,
    frame: Option<usize>,
    level: usize,
    draw: &dyn Fn(Flowchart, usize) -> Result<Option<Vec<Line>>, Failure>,
) -> Result<Option<Flowchart>, Failure> {
    // The chart is laid out in its frame's own direction, as that frame spells it, or in
    // the direction its header spells.
    let other_than_tb = match frame {
        Some(frame) => chart.subgraphs.get(frame).is_some_and(|frame| frame.spelled_td),
        None => chart.spelled_br,
    };
    let mut has_child = vec![false; chart.subgraphs.len()];
    for parent in chart.subgraphs.iter().filter_map(|subgraph| subgraph.parent) {
        *has_child.get_mut(parent).ok_or(Failure::Unsupported)? = true;
    }
    let own: Vec<Option<Direction>> = has_child
        .iter()
        .enumerate()
        .map(|(subgraph, &has_child)| {
            if frame == Some(subgraph) {
                None
            } else {
                own_direction(&chart, subgraph, has_child, (chart.direction, other_than_tb))
            }
        })
        .collect();
    let is_own = |subgraph: usize| own.get(subgraph).is_some_and(Option::is_some);
    let mut roots: Vec<usize> = (0..chart.subgraphs.len())
        .filter(|&subgraph| is_own(subgraph) && !chart.enclosing(subgraph).any(is_own))
        .collect();
    if roots.is_empty() {
        return Ok(Some(chart));
    }
    let depth = depth + 1;
    if depth > MAX_UNIT_DEPTH {
        return Err(Failure::Unsupported);
    }
    // Numbered by their first node, the units' boxes come in unit order among the nodes
    // of the chart around them (see `Outer::with_boxes`).
    roots.sort_by_key(|&root| {
        chart.subgraphs.get(root).and_then(|root| root.members.iter().min().copied())
    });
    let directions = roots
        .iter()
        .map(|&root| own.get(root).copied().flatten())
        .collect::<Option<Vec<_>>>()
        .ok_or(Failure::Unsupported)?;
    let units = Units::new(&chart, roots)?;
    let (outer, unit_charts) = split(chart, &units, directions)?;
    let boxes = unit_charts
        .into_iter()
        .map(|(chart, frame)| draw_unit(chart, frame, depth, level, draw))
        .collect::<Result<Vec<_>, _>>()?;
    let Some(boxes) = boxes.into_iter().collect::<Option<Vec<_>>>() else {
        return Ok(None);
    };
    outer.with_boxes(boxes).map(Some).ok_or(Failure::Unsupported)
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
    /// The units of `chart` that `roots`, none nested in another, are, in their order.
    fn new(chart: &Flowchart, roots: Vec<usize>) -> Result<Self, Failure> {
        let mut node_unit = vec![None; chart.nodes.len()];
        let mut root_unit = vec![None; chart.subgraphs.len()];
        for (unit, &root) in roots.iter().enumerate() {
            *root_unit.get_mut(root).ok_or(Failure::Unsupported)? = Some(unit);
            for &member in &chart.subgraphs.get(root).ok_or(Failure::Unsupported)?.members {
                *node_unit.get_mut(member).ok_or(Failure::Unsupported)? = Some(unit);
            }
        }
        let subgraph_unit = (0..chart.subgraphs.len())
            .map(|subgraph| {
                std::iter::once(subgraph)
                    .chain(chart.enclosing(subgraph))
                    .find_map(|holder| root_unit.get(holder).copied().flatten())
            })
            .collect();
        Ok(Self { roots, node_unit, subgraph_unit })
    }

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

/// A chart with a place left among its nodes for each unit's box.
struct Outer {
    direction: Direction,
    spelled_br: bool,
    /// `None` where a unit's box is to stand, in unit order.
    nodes: Vec<Option<Node>>,
    edges: Vec<Edge>,
    subgraphs: Vec<Subgraph>,
}

impl Outer {
    /// The chart with `boxes`, one for each unit in unit order, in the places left for
    /// them; `None` when there are fewer boxes than places.
    fn with_boxes(self, boxes: Vec<Node>) -> Option<Flowchart> {
        let Self { direction, spelled_br, nodes, edges, subgraphs } = self;
        let mut boxes = boxes.into_iter();
        let nodes =
            nodes.into_iter().map(|node| node.or_else(|| boxes.next())).collect::<Option<_>>()?;
        Some(Flowchart { direction, spelled_br, nodes, edges, subgraphs })
    }
}

/// `chart` split into the chart around `units`, where each unit's box takes the place
/// of the unit's first node and links to a unit end at its box, and each unit's own
/// chart, laid out in its entry of `directions`, with the index of its frame: its nodes,
/// the subgraphs lying in it with its own frame outermost, and the links with both ends
/// inside it. Unsupported when a link joins the inside of a unit to anything outside it,
/// which has no cell to start or end at in the unit's drawing, or when a unit holds no
/// node, which leaves its box no place.
fn split(
    chart: Flowchart,
    units: &Units,
    directions: Vec<Direction>,
) -> Result<(Outer, Vec<(Flowchart, usize)>), Failure> {
    let Flowchart { direction, spelled_br, nodes, edges, subgraphs } = chart;
    let mut unit_charts: Vec<Flowchart> = directions
        .into_iter()
        .map(|direction| Flowchart {
            direction,
            spelled_br: false,
            nodes: Vec::new(),
            edges: Vec::new(),
            subgraphs: Vec::new(),
        })
        .collect();
    // Each node's index in the chart around the units or in its unit's, and the index of
    // each unit's box.
    let mut node_index = Vec::with_capacity(nodes.len());
    let mut unit_node = vec![None; unit_charts.len()];
    let mut outer_nodes = Vec::new();
    for (node, &unit) in nodes.into_iter().zip(&units.node_unit) {
        let Some(unit) = unit else {
            node_index.push(outer_nodes.len());
            outer_nodes.push(Some(node));
            continue;
        };
        let held = &mut unit_charts.get_mut(unit).ok_or(Failure::Unsupported)?.nodes;
        if held.is_empty() {
            *unit_node.get_mut(unit).ok_or(Failure::Unsupported)? = Some(outer_nodes.len());
            outer_nodes.push(None);
        }
        node_index.push(held.len());
        held.push(node);
    }
    let unit_node: Vec<usize> =
        unit_node.into_iter().collect::<Option<_>>().ok_or(Failure::Unsupported)?;
    // Each subgraph's index in the chart around the units or in its unit's, known before
    // the subgraphs are moved, as a parent may come after the subgraph it holds.
    let mut unit_counts = vec![0; unit_charts.len()];
    let mut outer_count = 0;
    let subgraph_index = units
        .subgraph_unit
        .iter()
        .map(|&unit| {
            let count = match unit {
                Some(unit) => unit_counts.get_mut(unit)?,
                None => &mut outer_count,
            };
            *count += 1;
            Some(*count - 1)
        })
        .collect::<Option<Vec<usize>>>()
        .ok_or(Failure::Unsupported)?;
    let subgraph_at = |subgraph: usize| subgraph_index.get(subgraph).copied();
    let node_in = |unit: Option<usize>, node: usize| {
        (units.node_unit.get(node) == Some(&unit)).then(|| node_index.get(node).copied()).flatten()
    };
    let outer_node = |node: usize| match units.node_unit.get(node).copied().flatten() {
        Some(unit) => unit_node.get(unit).copied(),
        None => node_in(None, node),
    };
    let mut outer_subgraphs = Vec::new();
    for (index, subgraph) in subgraphs.into_iter().enumerate() {
        let unit = units.subgraph_unit.get(index).copied().flatten();
        let members = match unit {
            Some(_) => remap_members(&subgraph.members, |node| node_in(unit, node)),
            None => remap_members(&subgraph.members, outer_node),
        }
        .ok_or(Failure::Unsupported)?;
        let parent = if unit.is_some_and(|unit| units.roots.get(unit) == Some(&index)) {
            // The unit's own frame is the outermost one of its drawing.
            None
        } else {
            subgraph
                .parent
                .map(|parent| subgraph_at(parent).ok_or(Failure::Unsupported))
                .transpose()?
        };
        let subgraph = Subgraph { members, parent, ..subgraph };
        match unit {
            Some(unit) => {
                unit_charts.get_mut(unit).ok_or(Failure::Unsupported)?.subgraphs.push(subgraph);
            }
            None => outer_subgraphs.push(subgraph),
        }
    }
    let end_in = |end: End| match end {
        End::Node(node) => node_index.get(node).copied().map(End::Node),
        End::Subgraph(subgraph) => subgraph_at(subgraph).map(End::Subgraph),
    };
    let outer_end = |end: End| match end {
        End::Node(node) => outer_node(node).map(End::Node),
        End::Subgraph(subgraph) => match units.subgraph_unit.get(subgraph).copied().flatten() {
            Some(unit) => unit_node.get(unit).copied().map(End::Node),
            None => subgraph_at(subgraph).map(End::Subgraph),
        },
    };
    let mut outer_edges = Vec::new();
    for edge in edges {
        match (units.inside(edge.from), units.inside(edge.to)) {
            (Some(from), Some(to)) if from == to => {
                let (Some(from_end), Some(to_end)) = (end_in(edge.from), end_in(edge.to)) else {
                    return Err(Failure::Unsupported);
                };
                let edge = Edge { from: from_end, to: to_end, ..edge };
                unit_charts.get_mut(from).ok_or(Failure::Unsupported)?.edges.push(edge);
            }
            (None, None) => {
                let (Some(from), Some(to)) = (outer_end(edge.from), outer_end(edge.to)) else {
                    return Err(Failure::Unsupported);
                };
                outer_edges.push(Edge { from, to, ..edge });
            }
            _ => return Err(Failure::Unsupported),
        }
    }
    let frames = units
        .roots
        .iter()
        .map(|&root| subgraph_at(root))
        .collect::<Option<Vec<_>>>()
        .ok_or(Failure::Unsupported)?;
    let outer = Outer {
        direction,
        spelled_br,
        nodes: outer_nodes,
        edges: outer_edges,
        subgraphs: outer_subgraphs,
    };
    Ok((outer, unit_charts.into_iter().zip(frames).collect()))
}

/// The node holding the drawing `draw` makes of a unit's `chart`, whose own frame is
/// subgraph `frame`, at the spacing of tightening step `level`. `depth` is how many
/// subgraphs with a direction of their own the unit's chart lies inside of. `Ok(None)`
/// when `draw` gives `Ok(None)` for the drawing or one nested in it.
fn draw_unit(
    chart: Flowchart,
    frame: usize,
    depth: usize,
    level: usize,
    draw: &dyn Fn(Flowchart, usize) -> Result<Option<Vec<Line>>, Failure>,
) -> Result<Option<Node>, Failure> {
    let title_width = chart.subgraphs.get(frame).ok_or(Failure::Unsupported)?.title.width();
    let Some(chart) = embed_units(chart, depth, Some(frame), level, draw)? else {
        return Ok(None);
    };
    Ok(draw(chart, level)?.map(|lines| Node {
        body: Body::Drawing { lines, title_width },
        styling: Styling::default(),
        array_index: None,
    }))
}

/// The direction `subgraph` is laid out in as a diagram of its own: its `direction`
/// statement's, else the one crosswise to `enclosing`, and `None` — the subgraph stays
/// in the enclosing layout — when it is empty or a link joins something inside it to
/// something outside it. `has_child` tells whether a subgraph is nested in it.
/// `enclosing` is the enclosing direction paired with whether it is spelled other than
/// `TB` (see [`crosswise`]). Mermaid's docs state the last rule as "If any of a
/// subgraph's nodes are linked to the outside, subgraph direction will be ignored"
/// (`docs/syntax/flowchart.md`, "Direction in subgraphs" → "Limitation"), which
/// `extractor` in `mermaid-graphlib.js` implements. A link touching the subgraph's own
/// frame is left out of the check: upstream keeps the direction then ("Link *to*
/// subgraph1: subgraph1 direction is maintained", a comment in that section's example),
/// as its `isDescendant` does not count a cluster among its own descendants; a link
/// between the frame and something inside it is not drawn at all (see
/// [`drop_own_frame_links`](super::own_frame::drop_own_frame_links)).
fn own_direction(
    chart: &Flowchart,
    subgraph: usize,
    has_child: bool,
    enclosing: (Direction, bool),
) -> Option<Direction> {
    let frame = chart.subgraphs.get(subgraph)?;
    // An empty subgraph's only member is the hidden node its frame is drawn around.
    let hidden =
        |&node: &usize| chart.nodes.get(node).is_some_and(|node| matches!(node.body, Body::Hidden));
    if !has_child && frame.members.iter().all(hidden) {
        return None;
    }
    let is_member = frame.membership(chart.nodes.len())?;
    let inside = |end: End| match end {
        End::Node(node) => Some(is_member.get(node) == Some(&true)),
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
/// statement, given the enclosing direction and whether it is spelled other than `TB`:
/// left to right inside a top-down layout spelled `TB`, and top to bottom inside any
/// other (`rankdir === 'TB' ? 'LR' : 'TB'` in `extractor`). The chart's own `TD` and `v`
/// count as `TB`, as upstream normalizes them, but its `BR`, which upstream keeps as
/// written, and a subgraph's `direction TD` do not.
fn crosswise((enclosing, other_than_tb): (Direction, bool)) -> Direction {
    match enclosing {
        Direction::TopDown if !other_than_tb => Direction::LeftToRight,
        _ => Direction::TopDown,
    }
}
