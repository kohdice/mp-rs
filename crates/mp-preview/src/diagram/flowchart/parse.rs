//! Flowchart source text to nodes and edges, following upstream Mermaid's grammar
//! (`packages/mermaid/src/diagrams/flowchart/parser/flow.jison` in
//! <https://github.com/mermaid-js/mermaid>).

mod builder;
mod front_matter;
mod link;
mod shape_data;
mod statements;
mod tokens;

use std::collections::{HashMap, HashSet};

use crate::diagram::{Failure, SyntaxError};
use crate::style::Line;

use super::label::Label;
use super::styling::Styling;

use self::builder::Builder;
use self::statements::{split_statements, without_comment_lines};

pub(super) use self::front_matter::front_matter;

#[derive(Clone, Debug)]
pub(super) struct Flowchart {
    pub direction: Direction,
    pub nodes: Vec<Node>,
    pub edges: Vec<Edge>,
    pub subgraphs: Vec<Subgraph>,
}

/// The direction the flow runs in: from the header's direction token, or inside a
/// subgraph from its `direction` statement.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(super) enum Direction {
    LeftToRight,
    RightToLeft,
    TopDown,
    BottomUp,
}

#[derive(Debug, Clone)]
pub(super) struct Node {
    pub body: Body,
    /// Cells the box is grown by across the flow on each side of its label, so that
    /// every link end on one of its borders has a cell of its own; measured by
    /// [`grow_boxes`](super::layout::grow_boxes) and
    /// [`grow_for_labels`](super::layout::grow_for_labels).
    pub spread: usize,
    /// What the `default` and `node` classes, then the classes attached to the node's
    /// id in the order they were attached, and then the `style` statements naming it
    /// set, each over the ones before.
    pub styling: Styling,
}

/// What a node is drawn as.
#[derive(Debug, Clone)]
pub(super) enum Body {
    /// A box drawn around `label` in `shape`.
    Box { label: Label, shape: Shape },
    /// One blank cell: the member an empty subgraph's frame is drawn around.
    Hidden,
    /// A subgraph laid out in a direction of its own and drawn on its own, frame
    /// included, which the node stands for in the enclosing layout; made by
    /// [`embed_units`](super::unit::embed_units). `title_width` is the display width of
    /// the title of the drawing's outermost frame, which links keep clear of where they
    /// meet the box in a vertical layout.
    Drawing { lines: Vec<Line>, title_width: usize },
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(super) enum Shape {
    /// `A` or `A[label]`.
    Rectangle,
    /// `A(label)`.
    Rounded,
    /// `A{label}`.
    Diamond,
    /// `A([label])`.
    Stadium,
    /// `A[[label]]`.
    Subroutine,
    /// `A{{label}}`.
    Hexagon,
    /// `A>label]`.
    Asymmetric,
    /// `A[/label/]`, leaning right.
    LeanRight,
    /// `A[\label\]`, leaning left.
    LeanLeft,
    /// `A[/label\]`, wider at the bottom.
    Trapezoid,
    /// `A[\label/]`, wider at the top.
    InvTrapezoid,
    /// `A((label))`.
    Circle,
    /// `A(((label)))`.
    DoubleCircle,
    /// `A(-label-)`.
    Ellipse,
    /// `A[(label)]`.
    Cylinder,
    /// `notch-rect`, a card: the top-left corner cut.
    NotchedRectangle,
    /// `lin-rect`, a lined process: a line inside along the left side.
    LinedRectangle,
    /// `div-rect`, a divided process: a line inside along the top.
    DividedRectangle,
    /// `tag-rect`, a tagged process: a tag at the bottom-right corner.
    TaggedRectangle,
    /// `notch-pent`, a loop limit: both top corners cut.
    NotchedPentagon,
    /// `sl-rect`, a manual input: the top edge rising to the right.
    SlopedRectangle,
    /// `delay`: the right side a half circle.
    Delay,
    /// `bow-rect`, stored data: both sides curving the same way.
    BowTieRectangle,
    /// `curv-trap`, a display: a pointed left side and a curved right side.
    CurvedTrapezoid,
    /// `console`: a terminal window.
    Console,
    /// `browser`: a window with a control in its title bar.
    Browser,
    /// `bucket`: narrower at the bottom.
    Bucket,
    /// `doc`, a document: a wavy bottom.
    Document,
    /// `lin-doc`, a lined document: a document with a line inside along the left side.
    LinedDocument,
    /// `tag-doc`, a tagged document: a document with a tag at the bottom-right corner.
    TaggedDocument,
    /// `flag`, a paper tape: a wavy top and bottom.
    Flag,
    /// `docs`, documents: a document with a second one behind it.
    Documents,
    /// `st-rect`, processes: a rectangle with a second one behind it.
    StackedRectangle,
    /// `folder`: a tab on the top-left.
    Folder,
    /// `win-pane`, internal storage: lines inside along the top and the left side.
    WindowPane,
    /// `h-cyl`, direct access storage: a cylinder on its side.
    HorizontalCylinder,
    /// `lin-cyl`, disk storage: a cylinder with a second line under its top.
    LinedCylinder,
    /// `datastore`: two horizontal lines with open sides.
    Datastore,
    /// `tri`, an extract: a triangle pointing up.
    Triangle,
    /// `flip-tri`, a manual file: a triangle pointing down.
    FlippedTriangle,
    /// `hourglass`, a collate: two triangles meeting at a point.
    Hourglass,
    /// `brace`, a comment: a curly brace on the left, no box.
    Brace,
    /// `brace-r`: a curly brace on the right, no box.
    BraceRight,
    /// `braces`: curly braces on both sides, no box.
    Braces,
    /// `bang`: a spiky explosion.
    Bang,
    /// `cloud`: a ring of arcs.
    Cloud,
    /// `bolt`, a com link: a lightning bolt.
    Bolt,
    /// `person`: a round head above a rounded body.
    Person,
    /// `text`, a text block: the label without a border.
    Text,
    /// `fork`, a fork or join: a filled bar, no label.
    Fork,
    /// `sm-circ`, a start: a small circle, no label.
    SmallCircle,
    /// `fr-circ`, a stop: a small circle with a dot inside, no label.
    FramedCircle,
    /// `f-circ`, a junction: a small filled circle, no label.
    FilledCircle,
    /// `cross-circ`, a summary: a small circle with a cross inside, no label.
    CrossedCircle,
}

#[derive(Debug, Clone)]
pub(super) struct Edge {
    pub from: End,
    pub to: End,
    pub stroke: Stroke,
    /// The marker drawn where the link leaves the source, if any.
    pub tail: Option<Marker>,
    /// The marker drawn where the link meets the target, if any.
    pub head: Option<Marker>,
    pub label: Option<Label>,
    /// The fewest layers the link spans: one, plus one for each extra `-`, `=`, `.` or `~`,
    /// up to [`MAX_LINK_LENGTH`](link::MAX_LINK_LENGTH).
    pub length: usize,
    /// What the classes attached to the link's id, in the order they were attached,
    /// then the last `linkStyle default` statement and then the last `linkStyle`
    /// statement naming the link's index set, each over the ones before.
    pub styling: Styling,
}

/// One end of a link.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(super) enum End {
    /// Index into [`Flowchart::nodes`].
    Node(usize),
    /// Index into [`Flowchart::subgraphs`]: the link attaches to the subgraph's frame,
    /// as upstream allows with the `flowchart` graph type.
    Subgraph(usize),
}

impl End {
    /// The node at this end, unless it is a subgraph.
    pub(super) fn node(self) -> Option<usize> {
        match self {
            Self::Node(node) => Some(node),
            Self::Subgraph(_) => None,
        }
    }
}

impl Edge {
    /// The nodes at both ends, unless either end is a subgraph.
    pub(super) fn nodes(&self) -> Option<(usize, usize)> {
        Some((self.from.node()?, self.to.node()?))
    }
}

/// A `subgraph … end` block, drawn as a titled frame.
#[derive(Debug, Clone)]
pub(super) struct Subgraph {
    pub title: Label,
    /// Indices into [`Flowchart::nodes`] of the nodes referenced inside the block or
    /// inside the subgraphs nested in it, in order of first reference.
    pub members: Vec<usize>,
    /// Index into [`Flowchart::subgraphs`] of the subgraph this one is nested in.
    pub parent: Option<usize>,
    /// The direction of the block's `direction` statement, if it has one; of several,
    /// the last wins.
    pub direction: Option<Direction>,
    /// Cells the frame is grown by across the flow on each side of its members, so that
    /// every link end on one of its borders has a cell of its own; measured by
    /// [`grow_frames`](super::layout::grow_frames).
    pub spread: usize,
    /// Whether the last `view` an `id@{ view: … }` statement after the subgraph's `end`
    /// gives its id is `collapsed`, which draws it as one box (see
    /// [`collapse`](super::collapse::collapse)).
    pub collapsed: bool,
    /// What the classes attached to the subgraph's id, in the order they were attached,
    /// and then the `style` statements naming it set, each over the ones before; unlike
    /// a node, a subgraph takes no `default` or `node` class.
    pub styling: Styling,
}

impl Subgraph {
    /// Whether each of `node_count` nodes, indexed like [`Flowchart::nodes`], is a
    /// member; `None` when a member's index is not below `node_count`.
    pub(super) fn membership(&self, node_count: usize) -> Option<Vec<bool>> {
        let mut is_member = vec![false; node_count];
        for &member in &self.members {
            *is_member.get_mut(member)? = true;
        }
        Some(is_member)
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(super) enum Stroke {
    /// `--`.
    Solid,
    /// `-.`.
    Dotted,
    /// `==`.
    Thick,
    /// `~~~`: the link places its target like any other but is not drawn.
    Invisible,
}

impl Stroke {
    /// Whether a link of this stroke is drawn, and so takes cells at its ends.
    pub(super) fn is_visible(self) -> bool {
        self != Self::Invisible
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(super) enum Marker {
    Arrow,
    Circle,
    Cross,
}

/// Parses the flowchart `source`, whose first line is line `first_line` of the
/// diagram's source. `source` uses `\n` line endings: the Markdown layer
/// (`sanitize_block_body`) normalises CR before any diagram source reaches the parser.
pub(super) fn parse(source: &str, first_line: usize) -> Result<Flowchart, Failure> {
    let (cleaned, source_lines) = without_comment_lines(source);
    let mut statements = split_statements(&cleaned)
        .into_iter()
        .map(|(line, text)| {
            (first_line + source_lines.get(line).copied().unwrap_or(line), text.trim())
        })
        .filter(|(_, text)| !text.is_empty());
    let (header_line, header) = statements.next().ok_or(Failure::Unsupported)?;
    let direction = direction(header_line, header)?;
    let mut builder = Builder {
        chart: Flowchart { direction, nodes: Vec::new(), edges: Vec::new(), subgraphs: Vec::new() },
        index_of: HashMap::new(),
        open: Vec::new(),
        subgraph_ids: Vec::new(),
        edge_ids: HashSet::new(),
        collapsed: HashSet::new(),
        closed_subgraphs: HashSet::new(),
        styles: HashMap::new(),
        class_defs: HashMap::new(),
        classes: HashMap::new(),
        edge_names: Vec::new(),
        link_styles: HashMap::new(),
        default_link_style: Styling::default(),
    };
    for (line, text) in statements {
        builder.statement(line, text)?;
    }
    if let Some(open) = builder.open.last() {
        return Err(syntax_error(open.line, "subgraph is not closed with \"end\""));
    }
    builder.apply_styles();
    builder.resolve_subgraph_ids()?;
    let mut chart = builder.chart;
    // Mermaid draws an empty subgraph as a frame showing only its title, placed in the
    // flow like a node: a hidden member gives it a place and its frame the least size.
    // Inner subgraphs come first, so that a subgraph holding only an empty one is not
    // empty itself.
    for subgraph in chart.innermost_first() {
        if chart.subgraphs.get(subgraph).is_some_and(|subgraph| subgraph.members.is_empty()) {
            let hidden = chart.nodes.len();
            chart.nodes.push(Node { body: Body::Hidden, spread: 0, styling: Styling::default() });
            let holders: Vec<usize> =
                std::iter::once(subgraph).chain(chart.enclosing(subgraph)).collect();
            for holder in holders {
                if let Some(holder) = chart.subgraphs.get_mut(holder) {
                    holder.members.push(hidden);
                }
            }
        }
    }
    Ok(chart)
}

impl Flowchart {
    /// The subgraph `subgraph` is declared in, if any.
    fn parent_of(&self, subgraph: usize) -> Option<usize> {
        self.subgraphs.get(subgraph)?.parent
    }

    /// The subgraphs `subgraph` is nested in, from its parent outwards.
    pub(super) fn enclosing(&self, subgraph: usize) -> impl Iterator<Item = usize> {
        // Parsing leaves no cycle of parents; the bound keeps a walk finite regardless.
        std::iter::successors(self.parent_of(subgraph), move |&parent| self.parent_of(parent))
            .take(self.subgraphs.len())
    }

    /// How many subgraphs `subgraph` is nested in.
    pub(super) fn depth(&self, subgraph: usize) -> usize {
        self.enclosing(subgraph).count()
    }

    /// Whether subgraph `inner` is nested, at any depth, in subgraph `outer`.
    pub(super) fn encloses(&self, outer: usize, inner: usize) -> bool {
        self.enclosing(inner).any(|enclosing| enclosing == outer)
    }

    /// For each subgraph, whether each node is a member (see [`Subgraph::membership`]);
    /// `None` when a member is not a node.
    pub(super) fn memberships(&self) -> Option<Vec<Vec<bool>>> {
        self.subgraphs.iter().map(|subgraph| subgraph.membership(self.nodes.len())).collect()
    }

    /// Every subgraph's index, those nested deeper first.
    pub(super) fn innermost_first(&self) -> Vec<usize> {
        let mut order: Vec<usize> = (0..self.subgraphs.len()).collect();
        order.sort_by_key(|&subgraph| std::cmp::Reverse(self.depth(subgraph)));
        order
    }
}

/// The index each of `len` elements takes once those `keep` rejects are dropped; `None`
/// for a dropped one.
pub(super) fn compact_index(len: usize, keep: impl Fn(usize) -> bool) -> Vec<Option<usize>> {
    let mut kept = 0;
    (0..len)
        .map(|index| {
            keep(index).then(|| {
                kept += 1;
                kept - 1
            })
        })
        .collect()
}

/// `members` mapped through `map`, each new index kept at its first occurrence only;
/// `None` when `map` maps a member to `None`.
pub(super) fn remap_members(
    members: &[usize],
    map: impl Fn(usize) -> Option<usize>,
) -> Option<Vec<usize>> {
    let mut seen = HashSet::new();
    let mut remapped = Vec::with_capacity(members.len());
    for &member in members {
        let member = map(member)?;
        if seen.insert(member) {
            remapped.push(member);
        }
    }
    Some(remapped)
}

/// Reads the direction of a `flowchart`, `graph` or `flowchart-elk` header; `TD`, `TB`,
/// `v`, `BR` and an omitted direction mean top-down, and `>`, `<` and `^` mean `LR`, `RL`
/// and `BT`, as Mermaid does; any other word is a syntax error. `flowchart-elk` only
/// chooses upstream's layout engine, which has no counterpart in a text drawing. Other
/// diagram kinds are unsupported. Like Mermaid, the direction must end the statement:
/// anything after it on the same line needs a `;` first.
fn direction(line: usize, header: &str) -> Result<Direction, Failure> {
    let mut words = header.split_ascii_whitespace();
    if !matches!(words.next(), Some("flowchart" | "graph" | "flowchart-elk")) {
        return Err(Failure::Unsupported);
    }
    let direction = match words.next() {
        Some("LR" | ">") => Direction::LeftToRight,
        None | Some("TD" | "TB" | "v") => Direction::TopDown,
        // Upstream's lexer reads `BR` as a direction and keeps it, and its layout draws a
        // direction it does not know top-down.
        Some("BR") => Direction::TopDown,
        Some("BT" | "^") => Direction::BottomUp,
        Some("RL" | "<") => Direction::RightToLeft,
        Some(other) => return Err(syntax_error(line, format!("unknown direction \"{other}\""))),
    };
    if words.next().is_some() {
        return Err(syntax_error(line, "expected a new line or \";\" after the direction"));
    }
    Ok(direction)
}

/// The direction a `direction` statement's word names, if it names one.
fn direction_word(word: &str) -> Option<Direction> {
    match word {
        "TB" | "TD" => Some(Direction::TopDown),
        "BT" => Some(Direction::BottomUp),
        "LR" => Some(Direction::LeftToRight),
        "RL" => Some(Direction::RightToLeft),
        _ => None,
    }
}

fn syntax_error(line: usize, message: impl Into<String>) -> Failure {
    Failure::Syntax(SyntaxError { line: Some(line), message: message.into() })
}
