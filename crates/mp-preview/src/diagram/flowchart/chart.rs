//! The chart a flowchart's source describes: its direction, nodes, links and subgraphs,
//! which [`parse`](super::parse::parse) produces and every later stage reads.

use std::collections::HashSet;

use super::label::Label;
use super::styling::Styling;
use crate::style::Line;

#[derive(Clone, Debug)]
pub(super) struct Flowchart {
    pub(super) direction: Direction,
    /// Whether the header spells the direction `BR`, which is drawn top-down but, unlike
    /// `TD` and `v`, is not normalized to `TB` by upstream (see `crosswise` in the `unit`
    /// module).
    pub(super) spelled_br: bool,
    pub(super) nodes: Vec<Node>,
    pub(super) edges: Vec<Edge>,
    pub(super) subgraphs: Vec<Subgraph>,
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
    pub(super) body: Body,
    /// What the `default` and `node` classes, then the classes attached to the node's
    /// id in the order they were attached, and then the `style` statements naming it
    /// set, each over the ones before.
    pub(super) styling: Styling,
    /// The node's id when it is a canonical array index such as `0` or `42` (not `01`),
    /// which upstream's graph lists before every other id, in ascending order, as
    /// JavaScript's `Object.keys` does; `None` for any other id and for a node that
    /// stands for a subgraph.
    pub(super) array_index: Option<u32>,
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
    pub(super) from: End,
    pub(super) to: End,
    pub(super) stroke: Stroke,
    /// The marker drawn where the link leaves the source, if any.
    pub(super) tail: Option<Marker>,
    /// The marker drawn where the link meets the target, if any.
    pub(super) head: Option<Marker>,
    pub(super) label: Option<Label>,
    /// The fewest layers the link spans: one, plus one for each extra `-`, `=`, `.` or `~`,
    /// up to [`MAX_LINK_LENGTH`](super::parse::link::MAX_LINK_LENGTH).
    pub(super) length: usize,
    /// What the classes attached to the link's id, in the order they were attached,
    /// then the last `linkStyle default` statement and then the last `linkStyle`
    /// statement naming the link's index set, each over the ones before.
    pub(super) styling: Styling,
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

    /// The subgraph the link leaves and enters, if both ends are that one subgraph: the
    /// link is a self loop on its frame. That holds for an empty subgraph too, though the
    /// layout places its hidden member in its stead for any other link.
    pub(super) fn frame_loop(&self) -> Option<usize> {
        match (self.from, self.to) {
            (End::Subgraph(from), End::Subgraph(to)) if from == to => Some(from),
            _ => None,
        }
    }
}

/// The `subgraph … end` blocks with one id, drawn as a titled frame. The first block to
/// end gives the title and direction; a node or subgraph id listed in blocks of several
/// subgraphs belongs to the one whose block ended first, as upstream's `addSubGraph`
/// and `makeUniq` decide.
#[derive(Debug, Clone)]
pub(super) struct Subgraph {
    pub(super) title: Label,
    /// Indices into [`Flowchart::nodes`] of the nodes the subgraph holds, directly or
    /// through the subgraphs nested in it, in the order its blocks list them.
    pub(super) members: Vec<usize>,
    /// Index into [`Flowchart::subgraphs`] of the subgraph this one is nested in.
    pub(super) parent: Option<usize>,
    /// The direction of the first block's `direction` statement, if it has one; of
    /// several in that block, the last wins. Those in later blocks with the same id are
    /// ignored, as upstream's `addSubGraph` adds only nodes to an existing subgraph.
    pub(super) direction: Option<Direction>,
    /// Whether that `direction` statement spells top-down `TD` rather than `TB`. Upstream
    /// normalizes `TD` to `TB` for the chart only (`setDirection` in `flowDb.ts`) and
    /// hands a subgraph's direction to the layout as written, where only `TB` counts as
    /// top-down (see `crosswise` in the `unit` module).
    pub(super) spelled_td: bool,
    /// Whether the last `view` an `id@{ view: … }` statement after the subgraph's `end`
    /// gives its id is `collapsed`, which draws it as one box (see
    /// [`collapse`](super::collapse::collapse)).
    pub(super) collapsed: bool,
    /// What the classes attached to the subgraph's id, in the order they were attached,
    /// and then the `style` statements naming it set, each over the ones before; unlike
    /// a node, a subgraph takes no `default` or `node` class.
    pub(super) styling: Styling,
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

impl Flowchart {
    /// The subgraph `subgraph` is declared in, if any.
    fn parent_of(&self, subgraph: usize) -> Option<usize> {
        self.subgraphs.get(subgraph)?.parent
    }

    /// The subgraphs `subgraph` is nested in, from its parent outwards.
    pub(super) fn enclosing(&self, subgraph: usize) -> impl Iterator<Item = usize> {
        // The parents are not known to be acyclic when resolve_subgraph_ids calls this to
        // detect a cycle, so the bound is what makes that check terminate.
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
        order.sort_by_cached_key(|&subgraph| std::cmp::Reverse(self.depth(subgraph)));
        order
    }
}

/// The index each of `len` elements takes once those `keep` rejects are dropped; `None`
/// for a dropped one.
pub(super) fn compact_index(len: usize, keep: impl Fn(usize) -> bool) -> Vec<Option<usize>> {
    (0..len)
        .scan(0, |kept, index| {
            let compacted = keep(index).then_some(*kept);
            *kept += usize::from(compacted.is_some());
            Some(compacted)
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
