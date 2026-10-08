//! Flowchart source text to nodes and edges, following upstream Mermaid's grammar
//! (`packages/mermaid/src/diagrams/flowchart/parser/flow.jison` in
//! <https://github.com/mermaid-js/mermaid>).

use std::borrow::Cow;
use std::collections::{HashMap, HashSet};

use crate::diagram::{Failure, SyntaxError};
use crate::style::Line;

use super::label::{Label, is_entity_name_char, leading_string, shown_text, unquoted};
use super::styling::Styling;

#[derive(Clone, Debug)]
pub(super) struct Flowchart {
    pub direction: Direction,
    pub nodes: Vec<Node>,
    pub edges: Vec<Edge>,
    pub subgraphs: Vec<Subgraph>,
}

/// The way links point, from the header's direction token.
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
    /// The fewest layers the link spans: one, plus one for each extra `-`, `=` or `.`, up
    /// to [`MAX_LINK_LENGTH`].
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

/// The opening brackets of a node label, each with the closing brackets it may end with
/// and the shape each pair gives. An opener comes before the shorter openers it starts
/// with.
const BRACKETS: [(&str, &[(&str, Shape)]); 13] = [
    ("[(", &[(")]", Shape::Cylinder)]),
    ("(((", &[(")))", Shape::DoubleCircle)]),
    ("((", &[("))", Shape::Circle)]),
    ("(-", &[("-)", Shape::Ellipse)]),
    ("[/", &[("/]", Shape::LeanRight), ("\\]", Shape::Trapezoid)]),
    ("[\\", &[("\\]", Shape::LeanLeft), ("/]", Shape::InvTrapezoid)]),
    ("{{", &[("}}", Shape::Hexagon)]),
    ("([", &[("])", Shape::Stadium)]),
    ("[[", &[("]]", Shape::Subroutine)]),
    ("[", &[("]", Shape::Rectangle)]),
    ("(", &[(")", Shape::Rounded)]),
    ("{", &[("}", Shape::Diamond)]),
    (">", &[("]", Shape::Asymmetric)]),
];

/// Mermaid's default `flowchart.maxEdges`.
const MAX_EDGES: usize = 500;

/// The most layers a link spans, however many line characters it has: upstream's
/// `addSingleLink` lowers a longer link's `length` to 10.
const MAX_LINK_LENGTH: usize = 10;

/// Statements that only affect interaction, which a text drawing cannot show.
const IGNORED_KEYWORDS: [&str; 1] = ["click"];

/// The classes upstream's `getData` compiles for every node before the node's own,
/// so that `classDef default` and `classDef node` style every node, its own classes
/// overriding them.
const NODE_CLASSES: [&str; 2] = ["default", "node"];

/// A diagram's source split at its frontmatter.
pub(super) struct FrontMatter<'a> {
    /// The `title` the frontmatter gives, which upstream draws above the diagram.
    pub title: Option<String>,
    /// The source after the frontmatter.
    pub body: &'a str,
    /// The 1-based line of `source` that `body` starts on.
    pub first_line: usize,
}

/// Splits off the frontmatter `source` starts with, as upstream's `extractFrontMatter`
/// (`packages/mermaid/src/diagram-api/frontmatter.ts`) does, and reads its top-level
/// `title`. The block opens with `---` on the source's first line, which may be indented
/// but not preceded by blank lines, and closes with the next line that is the same indent
/// followed by `---`, as `frontMatterRegex` in `diagram-api/regexes.ts`, anchored at the
/// start of the text (`^([^\S\n\r]*)-{3}` … `\1-{3}`), reads it; that indent is removed
/// from each line between before its keys are read, and a line without it is read as
/// written.
/// Every key but `title` is skipped: upstream applies `config` (theme, curve, HTML
/// labels, …) to the SVG it draws, and none of it has a counterpart in box-drawing text.
pub(super) fn front_matter(source: &str) -> Result<FrontMatter<'_>, Failure> {
    let none = FrontMatter { title: None, body: source, first_line: 1 };
    let mut lines = source.split_inclusive('\n').zip(1..);
    let Some((first, opening)) = lines.next() else { return Ok(none) };
    let fence = first.trim_start_matches(|c: char| c.is_whitespace() && c != '\n' && c != '\r');
    if !is_fence(fence) {
        return Ok(none);
    }
    let indent = first.get(..first.len() - fence.len()).unwrap_or_default();
    let mut offset = first.len();
    let mut title = None;
    for (line, number) in lines {
        offset += line.len();
        let stripped = line.strip_prefix(indent);
        // `\1-{3}` needs the closing fence to repeat the indent.
        if stripped.is_some_and(is_fence) {
            let body = source.get(offset..).unwrap_or_default();
            return Ok(FrontMatter { title, body, first_line: number + 1 });
        }
        if let Some(value) = title_value(stripped.unwrap_or(line)) {
            title = value;
        }
    }
    Err(syntax_error(opening, "unclosed front matter"))
}

/// Whether `line` is a frontmatter fence: `---` and nothing after it but blanks.
fn is_fence(line: &str) -> bool {
    line.strip_prefix("---").is_some_and(|after| after.trim().is_empty())
}

/// The title a frontmatter `line`, its block's indent removed, gives when it holds the
/// top-level `title` key: `Some(None)` when the value is falsy, `None` when the line
/// holds another key. Only a value on the key's line is read, not a block scalar (`|`,
/// `>`) or a plain scalar continued on the lines after it.
fn title_value(line: &str) -> Option<Option<String>> {
    let after_key = line.strip_prefix("title:")?.trim_end_matches(['\n', '\r']);
    // In YAML, a `:` separates a key from its value only before a blank or the line's
    // end, so `title:x` is one plain scalar and no key.
    if !(after_key.is_empty() || after_key.starts_with([' ', '\t'])) {
        return None;
    }
    let value = after_key.trim_start_matches([' ', '\t']);
    let text = match value.starts_with(['"', '\'']).then(|| scalar(value)).flatten() {
        // A quoted value is a string, falsy only when empty.
        Some((quoted, _)) => Some(quoted.text()).filter(|text| !text.is_empty()),
        // A bare value, which YAML's block context lets hold commas and braces, runs to
        // the end of the line or to a comment, a `#` after a blank (YAML 1.2 §6.6); the
        // blank after the colon counts, so `title: # note` is a comment alone. A `#`
        // right after other text, as in `Hello#tag`, is part of the value.
        None => {
            let end = after_key
                .match_indices('#')
                .find(|&(at, _)| {
                    after_key.get(..at).is_some_and(|before| before.ends_with([' ', '\t']))
                })
                .map_or(after_key.len(), |(at, _)| at);
            let plain = after_key.get(..end).unwrap_or_default().trim();
            (!is_falsy_plain_scalar(plain)).then_some(Cow::Borrowed(plain))
        }
    };
    Some(text.map(|text| shown_text(&text)))
}

/// Whether the bare YAML scalar `value` reads as a falsy JavaScript value under js-yaml's
/// `JSON_SCHEMA`, which upstream's `extractFrontMatter` loads the frontmatter with before
/// keeping the title only `if (parsed.title)`: empty, `~` or `null` (null), `false`
/// (boolean), or a number equal to 0.
fn is_falsy_plain_scalar(value: &str) -> bool {
    matches!(value, "" | "~" | "null" | "Null" | "NULL" | "false" | "False" | "FALSE")
        || value.parse::<f64>().is_ok_and(|number| number == 0.0)
}

/// Parses the flowchart `source`, whose first line is line `first_line` of the
/// diagram's source.
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

/// Reads the direction of a `flowchart`, `graph` or `flowchart-elk` header; `TB`, `v` and
/// an omitted direction mean top-down, and `>`, `<` and `^` mean `LR`, `RL` and `BT`, as
/// Mermaid does. `flowchart-elk` only chooses upstream's layout engine, which has no
/// counterpart in a text drawing. Other diagram kinds are unsupported. Like Mermaid, the
/// direction must end the statement: anything after it on the same line needs a `;` first.
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

/// `source` without its comment lines, with the 0-based line of `source` that each line
/// of the result was, plus one entry past the last for the point after a final line
/// break, so that errors name lines as written.
///
/// A comment line is one whose first non-blank characters are `%%` followed by a
/// character other than `{` on the same line, together with any blank lines right
/// before it: upstream's `cleanupComments`
/// (`packages/mermaid/src/diagram-api/comments.ts`) replaces
/// `/^\s*%%(?!{)[^\n]+\n?/gm` in the whole text before parsing, where `\s*` also spans
/// line breaks, so it removes such a line even inside a quoted string that spans lines.
fn without_comment_lines(source: &str) -> (String, Vec<usize>) {
    let mut cleaned = String::with_capacity(source.len());
    let mut source_lines = Vec::new();
    // Blank lines read since the last other line, held back until the next line tells
    // whether a comment removes them with it.
    let mut blanks: Vec<(usize, &str)> = Vec::new();
    let mut count = 0;
    for (index, line) in source.split_inclusive('\n').enumerate() {
        count = index + 1;
        let content = line.trim_start();
        if starts_comment(content) {
            blanks.clear();
        } else if content.is_empty() {
            blanks.push((index, line));
        } else {
            for (kept, text) in blanks.drain(..).chain(std::iter::once((index, line))) {
                cleaned.push_str(text);
                source_lines.push(kept);
            }
        }
    }
    for (kept, text) in blanks {
        cleaned.push_str(text);
        source_lines.push(kept);
    }
    source_lines.push(count);
    (cleaned, source_lines)
}

/// Splits `source` into statements, each with the 0-based line it starts on, for error
/// reports: at each line break and `;`, and drops each comment the text after a directive
/// starts (see below) and the text [`passed_over`] names, ignoring all of them inside a
/// double-quoted string (see [`starts_string`]), so that it may span lines as
/// `flow.jison`'s `string` lexer state lets it, inside `@{ … }` shape data up to its
/// closing brace (see [`shape_data_end`]), which upstream's `shapeData` lexer state reads
/// across lines, and inside the text of a `-- text -->` link, from its `--`, `==` or `-.`
/// to its closing link or the end of its statement, which upstream reads in its exclusive
/// `edgeText` lexer states, where a `"` starts a string anywhere. The `;` closing
/// an entity code such as `#quot;` splits nothing: upstream's `encodeEntities` in
/// `packages/mermaid/src/utils.ts` replaces every `#\w+;` in the source before parsing
/// it.
///
/// `source` comes without its comment lines (see [`without_comment_lines`]), but a line
/// whose text after a directive is blank and then a comment still holds that comment:
/// upstream removes directives before comments, so the comment then starts its line and
/// is removed too. A `%%` anywhere else is text: `flow.jison` has no comment rule, and
/// its `NODE_STRING` holds `%`.
fn split_statements(source: &str) -> Vec<(usize, &str)> {
    let mut statements: Vec<(usize, &str)> = Vec::new();
    let mut in_quotes = false;
    // Whether the scanner is inside the text of a link that does not close on its line:
    // upstream reads it in the exclusive `edgeText` lexer state, where a `"` starts a
    // string anywhere and `;` and `@{` are text. A link that closes on its line is passed
    // over whole below.
    let mut in_link_text = false;
    let mut line = 0;
    // The line and byte index the current statement starts at.
    let (mut start_line, mut start) = (0, 0);
    // Where a `%%` comment cut the current statement short.
    let mut comment: Option<usize> = None;
    // Whether the line being read holds nothing but blanks so far, which a comment needs.
    // A directive passed over counts as blank, as upstream removes directives before
    // comments.
    let mut line_blank = true;
    // How many name characters follow the last `#`, while they may still make an entity
    // code; `None` outside one.
    let mut entity_name: Option<usize> = None;
    // Whether the current statement holds nothing but blanks so far. An accessibility
    // statement is read only where a statement starts: this scanner does not track
    // bracket labels, so "accDescr:" inside A[see accDescr: x] must stay text, as
    // upstream's lexer keeps it in its text state.
    let mut blank = true;
    // Whether a statement with text, the header, has been split off.
    let mut after_header = false;
    // The byte index of the line break ending the line being read, or of the source's
    // end, found once per line for the `-- text -->` check.
    let mut line_end = 0;
    let mut chars = source.char_indices().peekable();
    while let Some((index, c)) = chars.next() {
        let mut ends_statement = false;
        // Whether a statement starts right after the text this character begins: after
        // text passed over, the next statement starts on the same line, so that point is
        // the start of a statement.
        let mut restarts = false;
        match c {
            '\n' => ends_statement = !in_quotes,
            _ if comment.is_some() => {}
            '"' if in_quotes => in_quotes = false,
            '"' => in_quotes = in_link_text || starts_string(source, index),
            ';' => {
                ends_statement =
                    !in_quotes && !in_link_text && !entity_name.is_some_and(|length| length > 0);
            }
            '%' | 'a' if !in_quotes && !in_link_text => {
                let rest = source.get(index..).unwrap_or_default();
                match passed_over(rest, after_header, blank) {
                    Some(length) => {
                        let before = source.get(start..index).unwrap_or_default();
                        after_header |= !before.trim().is_empty();
                        statements.push((start_line, before));
                        while let Some(&(at, skipped)) = chars.peek()
                            && at < index + length
                        {
                            line += usize::from(skipped == '\n');
                            chars.next();
                        }
                        start = index + length;
                        start_line = line;
                        restarts = true;
                    }
                    None if c == '%' && line_blank && starts_comment(rest) => {
                        comment = Some(index);
                    }
                    None => {}
                }
            }
            '-' | '=' if !in_quotes => {
                // Upstream reads the text of a `-- text -->` link in exclusive lexer
                // states (`edgeText` and its thick and dotted kin), where `@{`, `;` and
                // `%%` are text, so a link closing on its line is passed over whole, and
                // one that does not puts the scanner in its text until the link closes.
                if line_end <= index {
                    line_end = source
                        .get(index..)
                        .and_then(|rest| rest.find('\n'))
                        .map_or(source.len(), |at| index + at);
                }
                let line_rest = source.get(index..line_end).unwrap_or_default();
                match link(line_rest) {
                    Some((Link { label: Some(_), .. }, after)) => {
                        let end = index + line_rest.len() - after.len();
                        while chars.next_if(|&(at, _)| at < end).is_some() {}
                    }
                    // The rest of a run of line characters starts no link with text:
                    // upstream's lexer takes the run as one `LINK` token. A lone `-` is
                    // link text and leaves the state as it is.
                    _ => {
                        while chars.next_if(|&(_, next)| next == c).is_some() {}
                        if let Some((_, stroke, after_base)) = link_start(line_rest) {
                            in_link_text = opens_text(stroke, after_base);
                        }
                    }
                }
            }
            '@' if !in_quotes
                && !in_link_text
                && chars.peek().is_some_and(|&(_, next)| next == '{') =>
            {
                // Unclosed shape data runs to the end of the source, for the statement to
                // report it.
                let data = index + 2;
                let end = source
                    .get(data..)
                    .and_then(shape_data_end)
                    .map_or(source.len(), |end| data + end);
                while let Some(&(at, skipped)) = chars.peek()
                    && at < end
                {
                    line += usize::from(skipped == '\n');
                    chars.next();
                }
            }
            _ => {}
        }
        if ends_statement {
            in_link_text = false;
            let end = comment.take().unwrap_or(index);
            let text = source.get(start..end).unwrap_or_default();
            after_header |= !text.trim().is_empty();
            statements.push((start_line, text));
            start = index + c.len_utf8();
            start_line = line + usize::from(c == '\n');
        }
        blank = ends_statement || restarts || (blank && c.is_whitespace());
        line_blank = match c {
            '\n' => true,
            '%' if restarts => line_blank,
            _ => line_blank && c.is_whitespace(),
        };
        line += usize::from(c == '\n');
        entity_name = match c {
            '#' => Some(0),
            _ if is_entity_name_char(c) => entity_name.map(|length| length + 1),
            _ => None,
        };
    }
    let end = comment.unwrap_or(source.len());
    statements.push((start_line, source.get(start..end).unwrap_or_default()));
    statements
}

/// Whether `rest`, which starts a line after blanks, starts a comment: `%%` and a
/// character other than `{` on the same line.
fn starts_comment(rest: &str) -> bool {
    rest.strip_prefix("%%")
        .and_then(|after| after.chars().next())
        .is_some_and(|next| !matches!(next, '{' | '\n' | '\r'))
}

/// The byte length of the text at the start of `rest` that [`split_statements`] passes
/// over whole, ending the statement before it, where `after_header` tells whether the
/// header has been read and `blank` whether `rest` starts a statement:
///
/// - A directive, `%%{` and a keyword (`init: …`), on one line or several, up to and
///   including its `}%%`: upstream applies its configuration to the SVG, which a text
///   drawing has no counterpart for. Upstream's `directiveRegex` (`%{2}{\s*(?:(\w+)\s*:|
///   (\w+))`, `packages/mermaid/src/diagram-api/regexes.ts`) needs the keyword, so `%%{`
///   without one is neither a directive nor, as `cleanupComments` keeps a line starting
///   with `%%{`, a comment. An unclosed directive runs to the end of the source.
/// - An accessibility statement (see [`accessibility_end`]) after the header, which
///   draws nothing upstream. Before the header it is not passed over: upstream's
///   detector needs the text, without frontmatter, directives and comments, to start
///   with `flowchart` or `graph`.
fn passed_over(rest: &str, after_header: bool, blank: bool) -> Option<usize> {
    if let Some(after_open) = rest.strip_prefix("%%{") {
        let keyword =
            after_open.trim_start().starts_with(|c: char| c.is_ascii_alphanumeric() || c == '_');
        return keyword.then(|| rest.find("}%%").map_or(rest.len(), |at| at + "}%%".len()));
    }
    if after_header && blank { accessibility_end(rest) } else { None }
}

/// The byte length of the accessibility statement `text` starts with, `flow.jison`'s
/// `acc_title` and `acc_descr` rules: `accTitle:` or `accDescr:` and the rest of the
/// line, `;` included, or `accDescr {` up to the closing `}`. `None` when `text` starts
/// with none, or with an `accDescr {` that nothing closes.
fn accessibility_end(text: &str) -> Option<usize> {
    let blank = [' ', '\t'];
    let rest = match text.strip_prefix("accTitle") {
        Some(after) => after.trim_start_matches(blank).strip_prefix(':')?,
        None => {
            let after = text.strip_prefix("accDescr")?;
            match after.trim_start_matches(blank).strip_prefix(':') {
                Some(after_colon) => after_colon,
                None => {
                    let after_brace = after.trim_start().strip_prefix('{')?;
                    let close = after_brace.find('}')?;
                    return Some(text.len() - after_brace.len() + close + 1);
                }
            }
        }
    };
    Some(text.len() - rest.len() + rest.find('\n').unwrap_or(rest.len()))
}

struct Builder<'a> {
    chart: Flowchart,
    index_of: HashMap<&'a str, usize>,
    /// The subgraphs whose `end` has not been read yet, innermost last.
    open: Vec<OpenSubgraph>,
    /// The id of every subgraph, indexed like [`Flowchart::subgraphs`], which Mermaid
    /// lets a link point to.
    subgraph_ids: Vec<&'a str>,
    /// The ids given to links (`A e1@--> B`), which an `e1@{ … }` statement refers to.
    edge_ids: HashSet<&'a str>,
    /// The ids of closed subgraphs whose last `view` in `@{ … }` data, given after their
    /// `end`, is `collapsed`.
    collapsed: HashSet<&'a str>,
    /// The ids of the subgraphs whose `end` has been read, which upstream's
    /// `addSubGraph` registers only then.
    closed_subgraphs: HashSet<&'a str>,
    /// What the `style` statements naming each id set, the later ones over the earlier.
    styles: HashMap<&'a str, Styling>,
    /// What the `classDef` statements naming each class set, the later ones over the
    /// earlier.
    class_defs: HashMap<&'a str, Styling>,
    /// The classes attached to each id, in the order they were attached.
    classes: HashMap<&'a str, Vec<&'a str>>,
    /// The id of each link, indexed like [`Flowchart::edges`].
    edge_names: Vec<Option<&'a str>>,
    /// What the last `linkStyle` statement naming each link's index sets.
    link_styles: HashMap<usize, Styling>,
    /// What the last `linkStyle default` statement sets.
    default_link_style: Styling,
}

/// A subgraph whose `end` has not been read yet.
struct OpenSubgraph {
    /// The line its `subgraph` statement is on, for error reports.
    line: usize,
    /// Its index into [`Flowchart::subgraphs`].
    index: usize,
    /// The nodes already among its members.
    members: HashSet<usize>,
}

impl<'a> Builder<'a> {
    /// A `subgraph` or `end` line, or a group of nodes optionally followed by links to
    /// further groups; a link joins every node of the group before it to every node of
    /// the group after it.
    /// `line` is the statement's 1-based line number in the source, for error reports.
    fn statement(&mut self, line: usize, statement: &'a str) -> Result<(), Failure> {
        // A closed `accDescr {` block never reaches here (see [`accessibility_end`]); an
        // unclosed one is an error, as upstream's `acc_descr_multiline` lexer state reads
        // up to a `}` and fails at the end of the text.
        if statement
            .strip_prefix("accDescr")
            .is_some_and(|after| after.trim_start().starts_with('{'))
        {
            return Err(syntax_error(line, "unclosed accessibility description"));
        }
        if let Some(rest) = statement.strip_prefix("subgraph")
            && (rest.is_empty() || rest.starts_with(|c: char| c.is_ascii_whitespace()))
        {
            return self.open_subgraph(line, rest);
        }
        if statement == "end" {
            let closed = self
                .open
                .pop()
                .ok_or_else(|| syntax_error(line, "\"end\" without an open subgraph"))?;
            if let Some(&id) = self.subgraph_ids.get(closed.index) {
                self.closed_subgraphs.insert(id);
            }
            return Ok(());
        }
        // Upstream's grammar needs every part of a styling statement, so a statement
        // missing one is a syntax error.
        if let Some((id, list)) = keyword_statement(statement, "style") {
            if id.is_empty() {
                return Err(syntax_error(line, "style has no node id"));
            }
            if list.is_empty() {
                return Err(syntax_error(line, "style has no property list"));
            }
            // Upstream's `styleStatement` passes the whole id, commas included, to
            // `addVertex`, which declares a node; a node whose id names a subgraph is
            // turned into that subgraph by `resolve_subgraph_ids`, as upstream's
            // `getData` merges the two.
            self.declare(id);
            let styles = self.styles.entry(id).or_default();
            *styles = styles.then(Styling::parse(without_color_semicolon(list)));
            return Ok(());
        }
        if let Some((names, list)) = keyword_statement(statement, "classDef") {
            if names.is_empty() {
                return Err(syntax_error(line, "classDef has no class name"));
            }
            if list.is_empty() {
                return Err(syntax_error(line, "classDef has no property list"));
            }
            let styling = Styling::parse(without_color_semicolon(list));
            for name in names.split(',') {
                let class = self.class_defs.entry(name).or_default();
                *class = class.then(styling);
            }
            return Ok(());
        }
        if let Some((positions, list)) = keyword_statement(statement, "linkStyle") {
            if positions.is_empty() {
                return Err(syntax_error(line, "linkStyle has no link index"));
            }
            // `default` stands alone in upstream's grammar; anything else is a list of
            // numbers (`numList`).
            let indexes = match positions {
                "default" => None,
                _ => Some(
                    positions
                        .split(',')
                        .map(|position| link_index(line, position, self.chart.edges.len()))
                        .collect::<Result<Vec<usize>, Failure>>()?,
                ),
            };
            // `interpolate` and a curve name choose the curve upstream draws the link
            // along; lines of glyphs run along rows and columns only, so the curve is
            // read and dropped, and a statement giving no style list after it leaves
            // the style as it was, as upstream's `updateLinkInterpolate` does.
            let list = match keyword_statement(list, "interpolate") {
                Some(("", _)) => {
                    return Err(syntax_error(line, "linkStyle interpolate has no curve name"));
                }
                Some((_, "")) => return Ok(()),
                Some((_, after_curve)) => after_curve,
                None => list,
            };
            if list.is_empty() {
                return Err(syntax_error(line, "linkStyle has no property list"));
            }
            // Each statement replaces the style it names, as upstream's `updateLink`
            // assigns the list rather than appending to it.
            let styling = Styling::parse(list);
            let Some(indexes) = indexes else {
                self.default_link_style = styling;
                return Ok(());
            };
            for index in indexes {
                self.link_styles.insert(index, styling);
            }
            return Ok(());
        }
        // Upstream's `setClass` splits the ids at commas but not the class name.
        if let Some((ids, class)) = keyword_statement(statement, "class") {
            if ids.is_empty() || class.is_empty() || class.contains(char::is_whitespace) {
                return Err(syntax_error(line, "class needs one node id list and one class name"));
            }
            for id in ids.split(',') {
                self.attach_class(id, class);
            }
            return Ok(());
        }
        if statement
            .split_ascii_whitespace()
            .next()
            .is_some_and(|word| IGNORED_KEYWORDS.contains(&word))
        {
            return Ok(());
        }
        // `direction` before a direction word sets the direction a subgraph's members are
        // laid out in, the last such statement winning; Mermaid reads it only there and
        // ignores it at the top level. Before anything else, `direction` is a node id, as
        // upstream lexes it as a direction only before a direction word.
        let mut words = statement.split_ascii_whitespace();
        if words.next() == Some("direction")
            && let Some(direction) = words.next().and_then(direction_word)
        {
            if let Some(open) = self.open.last().map(|open| open.index)
                && let Some(open) = self.chart.subgraphs.get_mut(open)
            {
                open.direction = Some(direction);
            }
            return Ok(());
        }
        // Data for a link declared earlier with an id: its keys (`animate`, `animation`,
        // `curve`) have no text drawing, and upstream ignores the rest. Upstream animates
        // the edge or draws it along the named curve; a terminal drawing is static and
        // its lines run along rows and columns, so the keys are read and dropped.
        let (id, after_id) = split_id(statement);
        if self.edge_ids.contains(id)
            && let Some(data) = after_id.strip_prefix("@{")
        {
            let end =
                shape_data_end(data).ok_or_else(|| syntax_error(line, "unclosed shape data"))?;
            if data.get(end + 1..).is_some_and(|after| after.trim().is_empty()) {
                return Ok(());
            }
        }
        let (mut sources, mut rest) = self.group(line, statement)?;
        while !rest.is_empty() {
            // An id is an edge id only when a link starts right after its `@`, so that an
            // `@` inside a link's label stays label text.
            let with_id = edge_id(rest)
                .and_then(|(id, after_id)| Some((Some(id), link(after_id)?)))
                .or_else(|| Some((None, link(rest)?)));
            let Some((id, (Link { stroke, tail, head, label, length }, after_link))) = with_id
            else {
                // Line characters that do not make a whole link are a syntax error in
                // Mermaid, and so is anything else where a link should be, but for an id
                // followed by `@`: that may be the edge id of a link form not read here,
                // so it falls back rather than being reported as a syntax error.
                if rest.strip_prefix('<').unwrap_or(rest).starts_with(['-', '=', '~']) {
                    return Err(syntax_error(line, "unclosed link"));
                }
                let (id, after_id) = split_id(rest);
                if !id.is_empty() && after_id.starts_with('@') {
                    return Err(Failure::Unsupported);
                }
                return Err(syntax_error(line, "expected a link"));
            };
            // An id that an earlier link already took names no link here, as upstream's
            // `addSingleLink` gives a repeated id a generated one instead.
            let mut id = id.filter(|&id| self.edge_ids.insert(id));
            let (label, after_link) = match after_link.trim_start().strip_prefix('|') {
                // Mermaid's grammar accepts a `|text|` label after `~~~`, but a link
                // that is not drawn has no line to carry the text, so the block falls
                // back rather than dropping the text.
                Some(_) if !stroke.is_visible() => return Err(Failure::Unsupported),
                Some(after_pipe) => {
                    let (label, after_label) = bracket_label(after_pipe, "|")
                        .ok_or_else(|| syntax_error(line, "unclosed edge label"))?;
                    check_label(line, label, &LABEL_TOKENS)?;
                    (Some(label.trim()), after_label.trim_start())
                }
                None => {
                    check_link_text(line, label)?;
                    (label, after_link.trim_start())
                }
            };
            if after_link.is_empty() {
                return Err(syntax_error(line, "edge has no target"));
            }
            let (targets, after_targets) = self.group(line, after_link)?;
            // Upstream's `addLink` passes the id only for the link from the last source
            // to the first target; every other link of the group gets a generated one. The
            // id is taken once, as a group naming a node twice (`A & A`) repeats that pair
            // and `addSingleLink` refuses the id the second time.
            let named = (sources.last().copied(), targets.first().copied());
            for &from in &sources {
                for &to in &targets {
                    if self.chart.edges.len() == MAX_EDGES {
                        return Err(syntax_error(
                            line,
                            format!("too many edges (limit {MAX_EDGES})"),
                        ));
                    }
                    let name = if (Some(from), Some(to)) == named { id.take() } else { None };
                    self.edge_names.push(name);
                    self.chart.edges.push(Edge {
                        from: End::Node(from),
                        to: End::Node(to),
                        stroke,
                        tail,
                        head,
                        label: label.map(Label::parse_after_string),
                        length,
                        styling: Styling::default(),
                    });
                }
            }
            (sources, rest) = (targets, after_targets);
        }
        Ok(())
    }

    /// Starts the subgraph declared by `text`, the part of a `subgraph` statement after
    /// the keyword: an id, which may be a quoted string, followed by a bracketed title, or
    /// else the rest of the statement as the id, without the quotes around it, and as the
    /// title. Either title is read by [`Label::parse_after_string`], as it may go on in
    /// plain text after a leading string. A subgraph opened inside another is nested in
    /// it.
    fn open_subgraph(&mut self, line: usize, text: &'a str) -> Result<(), Failure> {
        let text = text.trim();
        let quoted = text.strip_prefix('"').and_then(|rest| rest.split_once('"'));
        let (id, after_id) = quoted.unwrap_or_else(|| split_id(text));
        let (id, title) = match after_id.trim_start().strip_prefix('[') {
            Some(after_open) => {
                let (title, _) = bracket_label(after_open, "]")
                    .ok_or_else(|| syntax_error(line, "unclosed subgraph title"))?;
                check_label(line, title, &LABEL_TOKENS)?;
                (id, Label::parse_after_string(title))
            }
            None => {
                check_unbracketed_title(line, text)?;
                (unquoted(text), Label::parse_after_string(text))
            }
        };
        self.subgraph_ids.push(id);
        let parent = self.open.last().map(|parent| parent.index);
        self.open.push(OpenSubgraph {
            line,
            index: self.chart.subgraphs.len(),
            members: HashSet::new(),
        });
        self.chart.subgraphs.push(Subgraph {
            title: title.joined(),
            members: Vec::new(),
            parent,
            direction: None,
            spread: 0,
            collapsed: false,
            styling: Styling::default(),
        });
        Ok(())
    }

    /// Turns every reference to an id that is also a subgraph's, declared before or
    /// after it, into a reference to the subgraph, as Mermaid reads it: a link end
    /// becomes [`End::Subgraph`], a subgraph listed inside another's block is nested in
    /// it, and the node declared for the id is dropped. A subgraph listed in two blocks
    /// that do not nest, or nested in itself through such lists, is not drawn. Each
    /// subgraph whose id was last given `view: collapsed` is marked
    /// [`Subgraph::collapsed`].
    fn resolve_subgraph_ids(&mut self) -> Result<(), Failure> {
        for (subgraph, id) in self.chart.subgraphs.iter_mut().zip(&self.subgraph_ids) {
            subgraph.collapsed = self.collapsed.contains(id);
        }
        let mut named = vec![None; self.chart.nodes.len()];
        for (subgraph, id) in self.subgraph_ids.iter().enumerate() {
            if let Some(&node) = self.index_of.get(id)
                && let Some(slot) = named.get_mut(node)
            {
                *slot = Some(subgraph);
            }
        }
        // Each listed subgraph's parent is the innermost block listing it, which every
        // other block listing it encloses.
        let mut parents = Vec::new();
        for (node, &listed) in named.iter().enumerate() {
            let Some(listed) = listed else { continue };
            let holders: Vec<usize> = (0..self.chart.subgraphs.len())
                .filter(|&holder| {
                    self.chart
                        .subgraphs
                        .get(holder)
                        .is_some_and(|holder| holder.members.contains(&node))
                })
                .collect();
            let Some(&holder) = holders.iter().max_by_key(|&&holder| self.chart.depth(holder))
            else {
                continue;
            };
            if holders.iter().any(|&other| other != holder && !self.chart.encloses(other, holder)) {
                return Err(Failure::Unsupported);
            }
            parents.push((listed, holder));
        }
        for (listed, holder) in parents {
            let listed = self.chart.subgraphs.get_mut(listed).ok_or(Failure::Unsupported)?;
            if listed.parent.is_some_and(|parent| parent != holder) {
                return Err(Failure::Unsupported);
            }
            listed.parent = Some(holder);
        }
        if (0..self.chart.subgraphs.len())
            .any(|subgraph| self.chart.enclosing(subgraph).any(|outer| outer == subgraph))
        {
            return Err(Failure::Unsupported);
        }
        for subgraph in &mut self.chart.subgraphs {
            subgraph.members.retain(|&member| named.get(member).is_some_and(Option::is_none));
        }
        // The members of a nested subgraph are members of every subgraph enclosing it.
        for subgraph in self.chart.innermost_first() {
            let Some(inner) = self.chart.subgraphs.get(subgraph) else { continue };
            let (members, parent) = (inner.members.clone(), inner.parent);
            if let Some(parent) = parent.and_then(|parent| self.chart.subgraphs.get_mut(parent)) {
                for member in members {
                    if !parent.members.contains(&member) {
                        parent.members.push(member);
                    }
                }
            }
        }
        // The index each node keeps once the nodes naming subgraphs are dropped.
        let renumbered = compact_index(named.len(), |node| named.get(node) == Some(&None));
        let resolve = |end: End| match end {
            End::Node(node) => match named.get(node).copied().flatten() {
                Some(subgraph) => Some(End::Subgraph(subgraph)),
                None => Some(End::Node((*renumbered.get(node)?)?)),
            },
            End::Subgraph(_) => Some(end),
        };
        for edge in &mut self.chart.edges {
            edge.from = resolve(edge.from).ok_or(Failure::Unsupported)?;
            edge.to = resolve(edge.to).ok_or(Failure::Unsupported)?;
        }
        for member in self.chart.subgraphs.iter_mut().flat_map(|subgraph| &mut subgraph.members) {
            *member = renumbered.get(*member).copied().flatten().ok_or(Failure::Unsupported)?;
        }
        let mut names = named.iter();
        self.chart.nodes.retain(|_| names.next().is_some_and(Option::is_none));
        Ok(())
    }

    /// Reads node references joined by `&` at the start of `text`, and returns their
    /// indices with the text after them.
    fn group(&mut self, line: usize, text: &'a str) -> Result<(Vec<usize>, &'a str), Failure> {
        let (first, mut rest) = self.node(line, text)?;
        let mut nodes = vec![first];
        while let Some(after_ampersand) = rest.strip_prefix('&') {
            let (node, after_node) = self.node(line, after_ampersand.trim_start())?;
            nodes.push(node);
            rest = after_node;
        }
        Ok((nodes, rest))
    }

    /// The index of the node `id` names, declared as a box showing the id when new.
    fn declare(&mut self, id: &'a str) -> usize {
        *self.index_of.entry(id).or_insert_with(|| {
            self.chart.nodes.push(Node {
                body: Body::Box { label: Label::plain(id), shape: Shape::Rectangle },
                spread: 0,
                styling: Styling::default(),
            });
            self.chart.nodes.len() - 1
        })
    }

    /// Gives each node, link and subgraph what its classes and its `style` or
    /// `linkStyle` statements set (see [`Node::styling`], [`Edge::styling`] and
    /// [`Subgraph::styling`]).
    /// Classes are looked up only now, so that a `classDef` may follow the `class`
    /// statement using it, as upstream compiles them when the diagram is drawn.
    fn apply_styles(&mut self) {
        let resolved: Vec<(usize, Styling)> = self
            .index_of
            .iter()
            .map(|(id, &index)| (index, self.styling_of(id, &NODE_CLASSES)))
            .collect();
        for (index, styling) in resolved {
            if let Some(node) = self.chart.nodes.get_mut(index) {
                node.styling = styling;
            }
        }
        // Upstream applies a link's classes as compiled styles and its `linkStyle` lists,
        // the default one first, as inline styles over them.
        let link_stylings: Vec<Styling> = (0..self.chart.edges.len())
            .map(|index| {
                let name = self.edge_names.get(index).copied().flatten();
                let classes = name.map(|id| self.class_styling(id, &[])).unwrap_or_default();
                let own = self.link_styles.get(&index).copied().unwrap_or_default();
                classes.then(self.default_link_style).then(own)
            })
            .collect();
        for (edge, styling) in self.chart.edges.iter_mut().zip(link_stylings) {
            edge.styling = styling;
        }
        // Upstream compiles no `default` class for a subgraph, only its own classes.
        let subgraph_stylings: Vec<Styling> =
            self.subgraph_ids.iter().map(|id| self.styling_of(id, &[])).collect();
        for (subgraph, styling) in self.chart.subgraphs.iter_mut().zip(subgraph_stylings) {
            subgraph.styling = styling;
        }
    }

    /// What the classes `base` and then the classes attached to `id` set, in order, and
    /// then the `style` statements naming it.
    fn styling_of(&self, id: &str, base: &[&str]) -> Styling {
        self.class_styling(id, base).then(self.styles.get(id).copied().unwrap_or_default())
    }

    /// What the classes `base` and then the classes attached to `id` set, in order; a
    /// class no `classDef` defines sets nothing.
    fn class_styling(&self, id: &str, base: &[&str]) -> Styling {
        let classes = base.iter().chain(self.classes.get(id).into_iter().flatten());
        classes
            .filter_map(|class| self.class_defs.get(class))
            .fold(Styling::default(), |styling, &later| styling.then(later))
    }

    /// Attaches `class` to `id` when a node, a link or a subgraph whose `end` has been
    /// read has the id, as upstream's `setClass` does: an id declared only later gets
    /// nothing.
    fn attach_class(&mut self, id: &'a str, class: &'a str) {
        let known = self.index_of.contains_key(id)
            || self.edge_ids.contains(id)
            || self.closed_subgraphs.contains(id);
        if known {
            self.classes.entry(id).or_default().push(class);
        }
    }

    /// Reads one node reference at the start of `text`, declaring the node on first
    /// use, and returns its index with the text after it.
    fn node(&mut self, line: usize, text: &'a str) -> Result<(usize, &'a str), Failure> {
        let (id, rest) = split_id(text);
        if id.is_empty() {
            return Err(syntax_error(line, "expected a node id"));
        }
        if id == "end" {
            return Err(syntax_error(line, "\"end\" cannot be a node id"));
        }
        let index = self.declare(id);
        // A node referenced in a nested block is a member of every enclosing one too.
        for open in &mut self.open {
            if open.members.insert(index)
                && let Some(subgraph) = self.chart.subgraphs.get_mut(open.index)
            {
                subgraph.members.push(index);
            }
        }
        let bracketed = BRACKETS
            .into_iter()
            .find_map(|(open, closers)| Some((open, rest.strip_prefix(open)?, closers)));
        let rest = match bracketed {
            Some((open, after_open, closers)) => {
                let (label, shape, after_label) = closers
                    .iter()
                    .filter_map(|&(close, shape)| {
                        let (label, after_label) = bracket_label(after_open, close)?;
                        Some((label, shape, after_label))
                    })
                    // The closer found first ends the label.
                    .min_by_key(|(label, ..)| label.len())
                    .ok_or_else(|| syntax_error(line, "unclosed node label"))?;
                match open {
                    "[/" | "[\\" => check_trap_label(line, label)?,
                    _ => check_label(line, label, &LABEL_TOKENS)?,
                }
                if let Some(Body::Box { label: node_label, shape: node_shape }) =
                    self.chart.nodes.get_mut(index).map(|node| &mut node.body)
                {
                    *node_label = Label::parse_after_string(label);
                    *node_shape = shape;
                }
                after_label
            }
            None => rest,
        };
        let rest = match rest.strip_prefix(":::") {
            Some(after) => {
                let (class, after_class) = split_id(after);
                self.attach_class(id, class);
                after_class
            }
            None => rest,
        };
        let rest = match rest.strip_prefix("@{") {
            Some(after_open) => {
                let closed_subgraph = self.closed_subgraphs.contains(id);
                let (data, after_data) = shape_data(line, after_open, closed_subgraph)?;
                if let Some(Body::Box { label, shape }) =
                    self.chart.nodes.get_mut(index).map(|node| &mut node.body)
                {
                    if let Some(data_shape) = data.shape {
                        *shape = data_shape;
                    }
                    if let Some(data_label) = data.label {
                        *label = data_label;
                    }
                }
                // Upstream's `addVertex` gives the data to a subgraph only once its `end`
                // has been read; before that the id is a node's, which has no view.
                match data.collapsed {
                    _ if !closed_subgraph => {}
                    Some(true) => _ = self.collapsed.insert(id),
                    Some(false) => _ = self.collapsed.remove(id),
                    None => {}
                }
                after_data
            }
            None => rest,
        };
        Ok((index, rest.trim_start()))
    }
}

/// What a node's `@{ … }` shape data sets.
struct ShapeData {
    shape: Option<Shape>,
    /// Replaces the label, also one given in brackets.
    label: Option<Label>,
    /// Whether the `view` value is `collapsed`, when one is given; it matters only on a
    /// subgraph's id.
    collapsed: Option<bool>,
}

/// Upstream's short names and aliases of every shape, the `shape` values of `@{ … }`,
/// from `shapesDefs` in `packages/mermaid/src/rendering-util/rendering-elements/shapes.ts`
/// in <https://github.com/mermaid-js/mermaid>, also listed in the table "Complete List of
/// New Shapes" in <https://mermaid.js.org/syntax/flowchart.html>, followed by the
/// lowercase names of that file's `undocumentedShapes` that have a text drawing, each
/// drawn as the documented shape it looks like.
const SHAPE_NAMES: [(&str, Shape); 145] = [
    ("rect", Shape::Rectangle),
    ("proc", Shape::Rectangle),
    ("process", Shape::Rectangle),
    ("rectangle", Shape::Rectangle),
    ("rounded", Shape::Rounded),
    ("event", Shape::Rounded),
    ("diam", Shape::Diamond),
    ("decision", Shape::Diamond),
    ("diamond", Shape::Diamond),
    ("question", Shape::Diamond),
    ("stadium", Shape::Stadium),
    ("pill", Shape::Stadium),
    ("terminal", Shape::Stadium),
    ("fr-rect", Shape::Subroutine),
    ("framed-rectangle", Shape::Subroutine),
    ("subproc", Shape::Subroutine),
    ("subprocess", Shape::Subroutine),
    ("subroutine", Shape::Subroutine),
    ("cyl", Shape::Cylinder),
    ("cylinder", Shape::Cylinder),
    ("database", Shape::Cylinder),
    ("db", Shape::Cylinder),
    ("circle", Shape::Circle),
    ("circ", Shape::Circle),
    ("dbl-circ", Shape::DoubleCircle),
    ("double-circle", Shape::DoubleCircle),
    ("doublecircle", Shape::DoubleCircle),
    ("hex", Shape::Hexagon),
    ("hexagon", Shape::Hexagon),
    ("prepare", Shape::Hexagon),
    ("lean-r", Shape::LeanRight),
    ("lean-right", Shape::LeanRight),
    ("in-out", Shape::LeanRight),
    ("lean-l", Shape::LeanLeft),
    ("lean-left", Shape::LeanLeft),
    ("out-in", Shape::LeanLeft),
    ("trap-b", Shape::Trapezoid),
    ("priority", Shape::Trapezoid),
    ("trapezoid", Shape::Trapezoid),
    ("trapezoid-bottom", Shape::Trapezoid),
    ("trap-t", Shape::InvTrapezoid),
    ("inv-trapezoid", Shape::InvTrapezoid),
    ("manual", Shape::InvTrapezoid),
    ("trapezoid-top", Shape::InvTrapezoid),
    ("odd", Shape::Asymmetric),
    ("notch-rect", Shape::NotchedRectangle),
    ("card", Shape::NotchedRectangle),
    ("notched-rectangle", Shape::NotchedRectangle),
    ("lin-rect", Shape::LinedRectangle),
    ("lined-rectangle", Shape::LinedRectangle),
    ("lined-process", Shape::LinedRectangle),
    ("lin-proc", Shape::LinedRectangle),
    ("shaded-process", Shape::LinedRectangle),
    ("div-rect", Shape::DividedRectangle),
    ("div-proc", Shape::DividedRectangle),
    ("divided-rectangle", Shape::DividedRectangle),
    ("divided-process", Shape::DividedRectangle),
    ("tag-rect", Shape::TaggedRectangle),
    ("tagged-rectangle", Shape::TaggedRectangle),
    ("tag-proc", Shape::TaggedRectangle),
    ("tagged-process", Shape::TaggedRectangle),
    ("notch-pent", Shape::NotchedPentagon),
    ("loop-limit", Shape::NotchedPentagon),
    ("notched-pentagon", Shape::NotchedPentagon),
    ("sl-rect", Shape::SlopedRectangle),
    ("manual-input", Shape::SlopedRectangle),
    ("sloped-rectangle", Shape::SlopedRectangle),
    ("delay", Shape::Delay),
    ("half-rounded-rectangle", Shape::Delay),
    ("bow-rect", Shape::BowTieRectangle),
    ("stored-data", Shape::BowTieRectangle),
    ("bow-tie-rectangle", Shape::BowTieRectangle),
    ("curv-trap", Shape::CurvedTrapezoid),
    ("curved-trapezoid", Shape::CurvedTrapezoid),
    ("display", Shape::CurvedTrapezoid),
    ("console", Shape::Console),
    ("browser", Shape::Browser),
    ("bucket", Shape::Bucket),
    ("doc", Shape::Document),
    ("document", Shape::Document),
    ("lin-doc", Shape::LinedDocument),
    ("lined-document", Shape::LinedDocument),
    ("tag-doc", Shape::TaggedDocument),
    ("tagged-document", Shape::TaggedDocument),
    ("flag", Shape::Flag),
    ("paper-tape", Shape::Flag),
    ("docs", Shape::Documents),
    ("documents", Shape::Documents),
    ("st-doc", Shape::Documents),
    ("stacked-document", Shape::Documents),
    ("st-rect", Shape::StackedRectangle),
    ("procs", Shape::StackedRectangle),
    ("processes", Shape::StackedRectangle),
    ("stacked-rectangle", Shape::StackedRectangle),
    ("folder", Shape::Folder),
    ("directory", Shape::Folder),
    ("win-pane", Shape::WindowPane),
    ("internal-storage", Shape::WindowPane),
    ("window-pane", Shape::WindowPane),
    ("h-cyl", Shape::HorizontalCylinder),
    ("das", Shape::HorizontalCylinder),
    ("horizontal-cylinder", Shape::HorizontalCylinder),
    ("lin-cyl", Shape::LinedCylinder),
    ("disk", Shape::LinedCylinder),
    ("lined-cylinder", Shape::LinedCylinder),
    ("datastore", Shape::Datastore),
    ("data-store", Shape::Datastore),
    ("tri", Shape::Triangle),
    ("extract", Shape::Triangle),
    ("triangle", Shape::Triangle),
    ("flip-tri", Shape::FlippedTriangle),
    ("manual-file", Shape::FlippedTriangle),
    ("flipped-triangle", Shape::FlippedTriangle),
    ("hourglass", Shape::Hourglass),
    ("collate", Shape::Hourglass),
    ("brace", Shape::Brace),
    ("comment", Shape::Brace),
    ("brace-l", Shape::Brace),
    ("brace-r", Shape::BraceRight),
    ("braces", Shape::Braces),
    ("bang", Shape::Bang),
    ("cloud", Shape::Cloud),
    ("bolt", Shape::Bolt),
    ("com-link", Shape::Bolt),
    ("lightning-bolt", Shape::Bolt),
    ("person", Shape::Person),
    ("text", Shape::Text),
    ("fork", Shape::Fork),
    ("join", Shape::Fork),
    ("sm-circ", Shape::SmallCircle),
    ("start", Shape::SmallCircle),
    ("small-circle", Shape::SmallCircle),
    ("fr-circ", Shape::FramedCircle),
    ("stop", Shape::FramedCircle),
    ("framed-circle", Shape::FramedCircle),
    ("f-circ", Shape::FilledCircle),
    ("junction", Shape::FilledCircle),
    ("filled-circle", Shape::FilledCircle),
    ("cross-circ", Shape::CrossedCircle),
    ("summary", Shape::CrossedCircle),
    ("crossed-circle", Shape::CrossedCircle),
    ("state", Shape::Rounded),
    ("choice", Shape::Diamond),
    ("note", Shape::Rectangle),
    ("composite", Shape::Rectangle),
];

/// The lowercase names of upstream's `undocumentedShapes` that a text drawing has no
/// counterpart for: an anchor is a point without a box, and an icon is a picture.
const UNDRAWN_SHAPE_NAMES: [&str; 2] = ["anchor", "icon"];

/// The byte index of the `}` closing the shape data `text`, which follows the `@{`.
fn shape_data_end(text: &str) -> Option<usize> {
    scan_shape_data(text).map(|(_, end)| end)
}

/// Reads the `key: value` pairs of `@{ … }` shape data from `text`, which follows the
/// `@{`, and returns them with the text after the closing `}`. Text between a quoted
/// value and the next separator is a syntax error, as YAML rejects two pairs without a
/// separator between them. An empty `shape` or `label` sets nothing, as upstream's
/// `addVertex` in `flowDb.ts` skips a falsy one. On the id of a `subgraph` whose `end`
/// has been read only `view` is read, as `addVertex` merges the data into the
/// subgraph's and returns before it looks at a shape, an icon or an image.
fn shape_data(
    line: usize,
    text: &str,
    closed_subgraph: bool,
) -> Result<(ShapeData, &str), Failure> {
    let (pairs, end) =
        scan_shape_data(text).ok_or_else(|| syntax_error(line, "unclosed shape data"))?;
    if pairs.iter().any(|pair| !pair.after_value.trim().is_empty()) {
        return Err(syntax_error(line, "invalid shape data"));
    }
    let mut data = ShapeData { shape: None, label: None, collapsed: None };
    let mut image = false;
    for DataPair { key, value, .. } in pairs {
        match key {
            "view" => data.collapsed = Some(value.text() == "collapsed"),
            _ if closed_subgraph => {}
            "shape" | "label" if value.text().is_empty() => {}
            // An image, and a shape drawn as one, has no text drawing; every other pair
            // is checked first, so that an error beside it is reported whatever the
            // order of the keys.
            "shape" if UNDRAWN_SHAPE_NAMES.contains(&&*value.text()) => image = true,
            "shape" => data.shape = Some(shape_named(line, &value.text())?),
            "label" => data.label = Some(value.label()),
            "icon" | "img" => image = true,
            // Upstream ignores keys it does not know, and the rest only size or place
            // icons and images.
            _ => {}
        }
    }
    if image {
        return Err(Failure::Unsupported);
    }
    Ok((data, text.get(end + 1..).ok_or(Failure::Unsupported)?))
}

/// A value in shape data, written as one of YAML's flow scalars.
#[derive(Clone, Copy)]
enum Scalar<'a> {
    /// Written bare.
    Plain(&'a str),
    /// Between double quotes, where `\"` stands for a quote and `\\` for a backslash.
    // Upstream's YAML parser also decodes the other escapes (`\n`, `\t`, `\u…`, …); they
    // stay as written here.
    DoubleQuoted(&'a str),
    /// Between single quotes, where `''` stands for one quote.
    SingleQuoted(&'a str),
}

impl<'a> Scalar<'a> {
    /// The text the value stands for.
    fn text(self) -> Cow<'a, str> {
        match self {
            Self::Plain(text) => Cow::Borrowed(text),
            Self::DoubleQuoted(text) if !text.contains('\\') => Cow::Borrowed(text),
            Self::DoubleQuoted(text) => {
                let mut unescaped = String::with_capacity(text.len());
                let mut chars = text.chars().peekable();
                while let Some(c) = chars.next() {
                    match chars.peek() {
                        Some(&next @ ('"' | '\\')) if c == '\\' => {
                            unescaped.push(next);
                            chars.next();
                        }
                        _ => unescaped.push(c),
                    }
                }
                Cow::Owned(unescaped)
            }
            Self::SingleQuoted(text) if !text.contains("''") => Cow::Borrowed(text),
            Self::SingleQuoted(text) => Cow::Owned(text.replace("''", "'")),
        }
    }

    /// The label the value gives: a quoted value is a string, which may be a markdown
    /// string, as a quoted label in brackets is. A line break in a double-quoted value
    /// breaks the label, as upstream's `shapeDataStr` lexer rule replaces each line break
    /// and the blanks after it with `<br/>` before YAML reads the value; YAML folds one
    /// in a single-quoted value to a blank.
    fn label(self) -> Label {
        match self {
            Self::Plain(text) => Label::parse(text),
            Self::DoubleQuoted(_) => Label::string(&breaks_as_tags(&self.text())),
            Self::SingleQuoted(_) => Label::string(&self.text()),
        }
    }
}

/// `text` with each line break and the blanks after it replaced by `<br/>`.
fn breaks_as_tags(text: &str) -> Cow<'_, str> {
    let Some((first, mut rest)) = text.split_once('\n') else {
        return Cow::Borrowed(text);
    };
    let mut joined = format!("{first}<br/>");
    rest = rest.trim_start();
    while let Some((line, after)) = rest.split_once('\n') {
        joined.push_str(line);
        joined.push_str("<br/>");
        rest = after.trim_start();
    }
    joined.push_str(rest);
    Cow::Owned(joined)
}

/// One `key: value` pair of shape data, as [`scan_shape_data`] finds it.
struct DataPair<'a> {
    key: &'a str,
    value: Scalar<'a>,
    /// The text between the value and the next separator: blank unless the value is
    /// quoted and something follows its closing quote.
    after_value: &'a str,
}

/// Scans the shape data `text`, which follows the `@{`, and returns its `key: value`
/// pairs with the byte index of the closing `}`; `None` when nothing closes it.
/// Upstream's `flowDb` reads the braces as a YAML flow mapping; this reads the flat form
/// its documentation shows: pairs separated by commas or line breaks, each value bare or
/// quoted, a quoted one holding any commas, braces and line breaks. A quote starts a
/// quoted value only at the value's start, as in YAML, so `it's` stays bare. A `#` at the
/// start of a key or after a blank starts a comment running to the end of its line, a
/// `}` in it included. A `:` separates a key from its value only before a blank, a line
/// break, a `,` or a `}`, so `shape:rect` is one key without a value.
fn scan_shape_data(text: &str) -> Option<(Vec<DataPair<'_>>, usize)> {
    let mut pairs = Vec::new();
    let mut rest = text;
    loop {
        rest = rest.trim_start_matches(|c: char| c.is_whitespace() || c == ',');
        if rest.starts_with('}') {
            return Some((pairs, text.len() - rest.len()));
        }
        if rest.is_empty() {
            return None;
        }
        if rest.starts_with('#') {
            rest = rest.get(rest.find('\n')?..)?;
            continue;
        }
        let (key, after_key) = rest.split_at(plain_end(rest, true));
        // A key without a value sets nothing.
        let Some(after_colon) = after_key.strip_prefix(':') else {
            rest = after_comment(after_key)?;
            continue;
        };
        let (value, after) = scalar(after_colon.trim_start_matches([' ', '\t']))?;
        let after = after_comment(after)?;
        let (after_value, next) = after.split_at(after.find([',', '\n', '}'])?);
        pairs.push(DataPair { key: key.trim(), value, after_value });
        rest = next;
    }
}

/// `text` from the line break ending the comment it starts with after blanks, or
/// `text` itself when it starts with no comment; `None` when no line break ends it.
fn after_comment(text: &str) -> Option<&str> {
    let after_blanks = text.trim_start_matches([' ', '\t']);
    if after_blanks.len() == text.len() || !after_blanks.starts_with('#') {
        return Some(text);
    }
    after_blanks.get(after_blanks.find('\n')?..)
}

/// The byte index where the bare scalar at the start of `text` ends: at a comma, a line
/// break, a `}`, which YAML does not allow in one inside a flow mapping, or a blank
/// before a `#`, which starts a comment; for a `key`, also at a `:` before a blank, a
/// line break, a `,`, a `}` or the end of `text`.
fn plain_end(text: &str, key: bool) -> usize {
    let mut chars = text.char_indices().peekable();
    while let Some((at, c)) = chars.next() {
        let next = chars.peek().map(|&(_, next)| next);
        let ends = match c {
            ',' | '\n' | '}' => true,
            ' ' | '\t' => next == Some('#'),
            ':' => key && next.is_none_or(|next| next.is_whitespace() || matches!(next, ',' | '}')),
            _ => false,
        };
        if ends {
            return at;
        }
    }
    text.len()
}

/// Splits the scalar at the start of `text` off it; `None` when its closing quote is
/// missing. A bare scalar ends as [`plain_end`] says.
fn scalar(text: &str) -> Option<(Scalar<'_>, &str)> {
    if let Some(quoted) = text.strip_prefix('"') {
        let mut escaped = false;
        let end = quoted.char_indices().find_map(|(at, c)| {
            let closes = c == '"' && !escaped;
            escaped = c == '\\' && !escaped;
            closes.then_some(at)
        })?;
        return Some((Scalar::DoubleQuoted(quoted.get(..end)?), quoted.get(end + 1..)?));
    }
    if let Some(quoted) = text.strip_prefix('\'') {
        let mut from = 0;
        loop {
            let end = from + quoted.get(from..)?.find('\'')?;
            let after = quoted.get(end + 1..)?;
            match after.strip_prefix('\'') {
                Some(_) => from = end + 2,
                None => return Some((Scalar::SingleQuoted(quoted.get(..end)?), after)),
            }
        }
    }
    let (value, after) = text.split_at(plain_end(text, false));
    Some((Scalar::Plain(value.trim_end()), after))
}

/// The shape a `shape` value names. A name outside [`SHAPE_NAMES`] is a syntax error, as
/// upstream throws `No such shape`, which also covers names that are not lowercase.
// Upstream's `shapes` map, which `isValidShape` checks (`shape in shapes`, `shapes.ts`),
// holds the shapes of `undocumentedShapes` under their own names and each documented
// shape under its short name, `aliases` and `internalAliases`; `flowDb` rejects a name
// holding an uppercase letter or `_` before asking it. What that leaves beyond the
// documented names, `doublecircle` and the lowercase undocumented shapes, is accepted
// here.
fn shape_named(line: usize, name: &str) -> Result<Shape, Failure> {
    SHAPE_NAMES
        .into_iter()
        .find_map(|(known, shape)| (known == name).then_some(shape))
        .ok_or_else(|| syntax_error(line, format!("no such shape \"{name}\"")))
}

/// The parts of a link token that a drawing shows.
struct Link<'a> {
    stroke: Stroke,
    tail: Option<Marker>,
    head: Option<Marker>,
    /// The text of the `A -- text --> B` form: a leading double-quoted string, if any,
    /// quotes included, and the plain text after it, kept as written so that
    /// [`Label::parse_after_string`] can tell the string from that text.
    label: Option<&'a str>,
    length: usize,
}

/// Splits a link off the start of `text`. These are the `LINK` tokens of Mermaid's
/// `flow.jison`, `[xo<]?--+[-xo>]`, `[xo<]?==+[=xo>]`, `[xo<]?-\.+-[xo>]?` and `~~~+`,
/// and their text forms `-- text -->`, `== text ==>` and `-. text .->`.
fn link(text: &str) -> Option<(Link<'_>, &str)> {
    if let Some(after_base) = text.strip_prefix("~~") {
        let rest = after_base.trim_start_matches('~');
        // Each tilde past the third lengthens the link, as each extra dash does.
        let length = after_base.len() - rest.len();
        let link = Link {
            stroke: Stroke::Invisible,
            tail: None,
            head: None,
            label: None,
            length: length.min(MAX_LINK_LENGTH),
        };
        return (length > 0).then_some((link, rest));
    }
    let (tail, stroke, after_base) = link_start(text)?;
    let after_dots = after_base.trim_start_matches('.');
    let dots = after_base.len() - after_dots.len();
    let (label, closing) = match stroke {
        Stroke::Dotted if after_dots.starts_with('-') => (None, after_dots),
        Stroke::Dotted if opens_text(stroke, after_base) => {
            let (label, closing) =
                after_base.split_at_checked(find_after_quotes(after_base, ".-")?)?;
            if label.ends_with('.') {
                return None;
            }
            (Some(label.trim()), closing.get(1..)?)
        }
        Stroke::Solid | Stroke::Thick if opens_text(stroke, after_base) => {
            let doubled = if stroke == Stroke::Solid { "--" } else { "==" };
            let (label, closing) =
                after_base.split_at_checked(find_after_quotes(after_base, doubled)?)?;
            (Some(label.trim()), closing.get(2..)?)
        }
        Stroke::Dotted | Stroke::Invisible => return None,
        Stroke::Solid | Stroke::Thick => (None, after_base),
    };
    let (head, length, rest) = link_end(stroke, closing)?;
    let length = (length + dots).min(MAX_LINK_LENGTH);
    Some((Link { stroke, tail, head, label, length }, rest))
}

/// Splits the start of a link other than `~~~` off `text`: its tail marker (`<`, `o`,
/// `x`, or none), its stroke, read from `-.`, `--` or `==`, and the text after those two
/// characters.
fn link_start(text: &str) -> Option<(Option<Marker>, Stroke, &str)> {
    let tail = match text.chars().next() {
        Some('<') => Some(Marker::Arrow),
        Some('o') => Some(Marker::Circle),
        Some('x') => Some(Marker::Cross),
        _ => None,
    };
    let body = if tail.is_some() { text.get(1..)? } else { text };
    let (stroke, after_base) = if let Some(after_base) = body.strip_prefix("-.") {
        (Stroke::Dotted, after_base)
    } else if let Some(after_base) = body.strip_prefix("--") {
        (Stroke::Solid, after_base)
    } else {
        (Stroke::Thick, body.strip_prefix("==")?)
    };
    Some((tail, stroke, after_base))
}

/// Whether a link start of `stroke` followed by `after_base` opens the text of a
/// `-- text -->` link rather than going on as a link such as `-->`, `-.->` or `===`.
fn opens_text(stroke: Stroke, after_base: &str) -> bool {
    match stroke {
        Stroke::Dotted => !after_base.starts_with(['.', '-']),
        Stroke::Solid | Stroke::Thick => {
            !after_base.starts_with(|c| c == line_char(stroke) || head_marker(c).is_some())
        }
        Stroke::Invisible => false,
    }
}

/// Splits an edge id and the `@` after it off the start of `text`, for Mermaid's
/// `LINK_ID` token `[^\s\"]+\@(?=[^\{\"])`, which stands before a link. The id ends at
/// the first `@`: the token's greedy match may take the last one, which would swallow an
/// `@` inside a following label, and that `@` stays label text here.
fn edge_id(text: &str) -> Option<(&str, &str)> {
    let token = text.split(|c: char| c.is_whitespace() || c == '"').next()?;
    let at = token.find('@')?;
    let (id, after_at) = (token.get(..at)?, text.get(at + 1..)?);
    let next = after_at.chars().next()?;
    (!id.is_empty() && next != '{' && next != '"').then_some((id, after_at))
}

/// The byte index of the first `close` in `text`, searching after the double-quoted
/// string that `text` starts with, if any, so that the string may contain `close`.
fn find_after_quotes(text: &str, close: &str) -> Option<usize> {
    let skip = match text.trim_start().strip_prefix('"') {
        Some(quoted) => text.len() - quoted.len() + quoted.find('"')? + 1,
        None => 0,
    };
    Some(skip + text.get(skip..)?.find(close)?)
}

/// The character repeated in a link of `stroke`, after its first two characters.
fn line_char(stroke: Stroke) -> char {
    match stroke {
        Stroke::Solid | Stroke::Dotted | Stroke::Invisible => '-',
        Stroke::Thick => '=',
    }
}

/// Splits the end of a link — the part after `--`, `==` or `-.` and any extra dots —
/// off `text`, returning its head marker (`>`, `o`, `x`, or none) and the link's length
/// counted from its dashes or equals signs. A dotted link ends in `-` plus an optional
/// marker; a solid or thick one in line characters and a marker, or in at least one
/// line character without a marker.
fn link_end(stroke: Stroke, text: &str) -> Option<(Option<Marker>, usize, &str)> {
    if stroke == Stroke::Dotted {
        let after_line = text.strip_prefix('-')?;
        return Some(match after_line.chars().next().and_then(head_marker) {
            Some(head) => (Some(head), 1, after_line.get(1..)?),
            None => (None, 1, after_line),
        });
    }
    let after_line = text.trim_start_matches(line_char(stroke));
    let extra = text.len() - after_line.len();
    match after_line.chars().next().and_then(head_marker) {
        Some(head) => Some((Some(head), 1 + extra, after_line.get(1..)?)),
        None if extra > 0 => Some((None, extra, after_line)),
        None => None,
    }
}

/// The marker a link character at the target end stands for.
fn head_marker(c: char) -> Option<Marker> {
    match c {
        '>' => Some(Marker::Arrow),
        'o' => Some(Marker::Circle),
        'x' => Some(Marker::Cross),
        _ => None,
    }
}

fn syntax_error(line: usize, message: impl Into<String>) -> Failure {
    Failure::Syntax(SyntaxError { line: Some(line), message: message.into() })
}

/// The word after `keyword` and the rest of `statement` after that word, both trimmed
/// and possibly empty, when `statement` is `keyword` alone or `keyword` and a blank;
/// `None` otherwise.
fn keyword_statement<'a>(statement: &'a str, keyword: &str) -> Option<(&'a str, &'a str)> {
    let rest = statement.strip_prefix(keyword)?;
    if !(rest.is_empty() || rest.starts_with(|c: char| c.is_ascii_whitespace())) {
        return None;
    }
    let rest = rest.trim_start();
    let (word, after) = rest.split_once(|c: char| c.is_ascii_whitespace()).unwrap_or((rest, ""));
    Some((word, after.trim()))
}

/// The property `list` of a `style` or `classDef` statement without the `;` ending it,
/// when a `:`, then non-blanks, then a `#` come before that `;`. A `;` right after a hex
/// color ends no statement (see [`split_statements`]), so it stays in the list; upstream's
/// `encodeEntities` (`packages/mermaid/src/utils.ts`) drops it from such statements with
/// `/style.*:\S*#.*;/` and `/classDef.*:\S*#.*;/` before parsing.
fn without_color_semicolon(list: &str) -> &str {
    let Some(before) = list.strip_suffix(';') else { return list };
    let color_after_colon = before.match_indices(':').any(|(at, _)| {
        before
            .get(at + 1..)
            .is_some_and(|after| after.chars().take_while(|c| !c.is_whitespace()).any(|c| c == '#'))
    });
    if color_after_colon { before } else { list }
}

/// The link index `position` of a `linkStyle` statement on `line`, after `links` links:
/// digits only, as upstream's `NUM` token reads it, and below `links`, as upstream's
/// `updateLink` and `updateLinkInterpolate` fail on a link not defined yet.
fn link_index(line: usize, position: &str, links: usize) -> Result<usize, Failure> {
    let digits = !position.is_empty() && position.bytes().all(|byte| byte.is_ascii_digit());
    let index: usize = digits.then(|| position.parse().ok()).flatten().ok_or_else(|| {
        syntax_error(line, format!("linkStyle index \"{position}\" is not a number"))
    })?;
    if index >= links {
        let range = match links.checked_sub(1) {
            Some(last) => format!("0 to {last}"),
            None => "no links".to_owned(),
        };
        let message = format!("linkStyle index {index} is out of range ({range})");
        return Err(syntax_error(line, &message));
    }
    Ok(index)
}

/// Splits the longest id prefix off `text`.
fn split_id(text: &str) -> (&str, &str) {
    let end = text
        .char_indices()
        .find(|&(index, c)| {
            !is_id_char(c, text.get(index + c.len_utf8()..).and_then(|after| after.chars().next()))
        })
        .map_or(text.len(), |(index, _)| index);
    text.split_at(end)
}

/// The characters upstream's lexer never reads as part of an unquoted label's text, with
/// the way a syntax error names them.
struct LabelTokens {
    /// The characters themselves.
    chars: &'static [char],
    /// The characters as a syntax error names them.
    names: &'static str,
}

/// The tokens of a plain label: upstream's `text` state takes `TEXT` as
/// `[^\[\]\(\)\{\}\|\"]+`, a closing bracket or `|` ends the label, and `(`, `[`, `{`,
/// `|` and `"` start a new token in every state.
const LABEL_TOKENS: LabelTokens =
    LabelTokens { chars: &['(', ')', '[', ']', '{', '}', '|', '"'], names: "( ) [ ] { } | or \"" };

/// The tokens of a lean or trapezoid label (`[/ /]`, `[\ \]`, `[/ \]`, `[\ /]`). Upstream's
/// `trapText` state takes `TEXT` as `\/(?!\])|\\(?!\])|[^\\\[\]\(\)\{\}\/]+`, and that
/// rule is listed before `<*>"|"`, so `|` is text there, as are the `/` and `\` that
/// close no label. `"` is text there too, as that `TEXT` does not leave it out: the
/// `<*>["]` rule, listed first, starts a string only where a token starts, right after
/// the opening bracket or after a `/` or `\`, which are tokens of their own;
/// [`check_trap_label`] rejects the latter.
const TRAP_LABEL_TOKENS: LabelTokens =
    LabelTokens { chars: &['(', ')', '[', ']', '{', '}'], names: "( ) [ ] { or }" };

/// Checks that the unquoted text of `label`, as [`bracket_label`] split it off on `line`
/// — all of it, or what follows a leading double-quoted string — holds none of the
/// characters of `tokens`, which the error names as `tokens` does: [`LABEL_TOKENS`], or
/// [`TRAP_LABEL_TOKENS`] for a lean or trapezoid label.
fn check_label(line: usize, label: &str, tokens: &LabelTokens) -> Result<(), Failure> {
    let unquoted = leading_string(label).map_or(label, |(_, rest)| rest);
    if !unquoted.contains(tokens.chars) {
        return Ok(());
    }
    Err(syntax_error(
        line,
        format!("unquoted label contains {}; wrap the label in double quotes", tokens.names),
    ))
}

/// Checks that the unquoted text of a subgraph `title` written without brackets on `line`
/// — all of it, or what follows a leading double-quoted string — is made of upstream's
/// `textNoTags` tokens (`NUM`, `NODE_STRING`, `SPACE`, `MINUS`, `AMP`, `UNICODE_TEXT`,
/// `COLON`, `MULT`, `BRKT`, keywords and `START_LINK`), which hold no
/// `( ) [ ] { } | < > , @ ~`, no `--`, `==` or `-.`, and a `"` only inside a word.
/// `START_LINK` is a `textNoTags` token, but it moves upstream's lexer into link text,
/// whose tokens no title may hold.
fn check_unbracketed_title(line: usize, title: &str) -> Result<(), Failure> {
    const TOKEN_CHARS: [char; 12] = ['(', ')', '[', ']', '{', '}', '|', '<', '>', ',', '@', '~'];
    let unquoted = leading_string(title).map_or(title, |(_, rest)| rest);
    if unquoted.contains(TOKEN_CHARS)
        || unquoted.char_indices().any(|(at, _)| unquoted.get(at..).and_then(link_start).is_some())
    {
        return Err(syntax_error(
            line,
            "unquoted subgraph title contains ( ) [ ] { } | < > , @ ~ or a link; wrap the title in double quotes",
        ));
    }
    // `textNoTags` may start with a string but takes no other.
    if unquoted.match_indices('"').any(|(at, _)| starts_string(unquoted, at)) {
        return Err(syntax_error(
            line,
            "unquoted subgraph title contains \" at the start of a word; wrap the title in double quotes",
        ));
    }
    Ok(())
}

/// Checks a lean or trapezoid `label` on `line` as [`check_label`] does with
/// [`TRAP_LABEL_TOKENS`], and that its unquoted text has no `"` right after a `/` or
/// `\`: upstream's `trapText` lexer reads each `/` and `\` as a token of its own, so the
/// `"` after one starts a token, which the `<*>["]` rule, listed first, reads as the
/// start of a string the grammar does not accept there.
fn check_trap_label(line: usize, label: &str) -> Result<(), Failure> {
    check_label(line, label, &TRAP_LABEL_TOKENS)?;
    let unquoted = leading_string(label).map_or(label, |(_, rest)| rest);
    if !unquoted.contains("/\"") && !unquoted.contains("\\\"") {
        return Ok(());
    }
    Err(syntax_error(
        line,
        "unquoted label contains \" after / or \\; wrap the label in double quotes",
    ))
}

/// Checks that the text of a `-- text -->` link on `line`, if any, holds no `"` outside
/// a double-quoted string it starts with. Upstream's grammar reads it as
/// `edgeText: edgeTextToken | edgeText edgeTextToken | STR | MD_STR`, where a string may
/// only come first, and its `<*>["]` rule is listed before the `<edgeText>` rules, so a
/// `"` anywhere else starts a string the grammar does not accept there. The
/// `( ) [ ] { } |` that a bracket label may not hold are text here, as the
/// `<edgeText>[^-]|\-(?!\-)+` rule is listed before `<*>"("` and its kin.
fn check_link_text(line: usize, text: Option<&str>) -> Result<(), Failure> {
    let Some(text) = text else { return Ok(()) };
    let unquoted = leading_string(text).map_or(text, |(_, rest)| rest);
    if !unquoted.contains('"') {
        return Ok(());
    }
    Err(syntax_error(line, "unquoted link text contains \"; wrap the text in double quotes"))
}

/// Splits `text`, which follows an opening bracket, into the label and the text after
/// the `close` bracket. A label may start with a string in double quotes, which may
/// contain brackets, and go on in plain text up to `close`; it keeps its quotes, which
/// tell [`Label::parse_after_string`] a quoted string, such as a markdown string, from
/// plain text.
fn bracket_label<'a>(text: &'a str, close: &str) -> Option<(&'a str, &'a str)> {
    match text.strip_prefix('"') {
        Some(quoted) => {
            let (inner, after_quote) = quoted.split_once('"')?;
            let (rest, after_close) = after_quote.split_once(close)?;
            let label = text.get(..inner.len() + 2 + rest.len())?;
            Some((label, after_close))
        }
        None => text.split_once(close),
    }
}

/// Whether the `"` at byte index `at` of `text` starts a string: upstream's `<*>["]` rule
/// reads one only where a token starts, and inside a word the `NODE_STRING` token, which
/// holds `"`, has already taken it. Right after the opening bracket of a node label (see
/// [`BRACKETS`]) a token starts even when the bracket ends in a `NODE_STRING` character,
/// as the `/` of `[/`, the `\` of `[\` and the `-` of `(-` do: the lexer has read the
/// bracket as a token of its own and left its `INITIAL` state.
///
/// This is the rule of upstream's `INITIAL` lexer state: inside the text of a link,
/// [`split_statements`] tracks the state itself.
fn starts_string(text: &str, at: usize) -> bool {
    let Some(before) = text.get(..at) else { return true };
    BRACKETS.iter().any(|(open, _)| before.ends_with(open))
        || !before.chars().next_back().is_some_and(|before| is_id_char(before, Some('"')))
}

/// The characters of the `NODE_STRING` token of Mermaid's `flow.jison`, plus non-ASCII
/// text: a hyphen belongs to an id only when it cannot start a link (`-->`, `---`,
/// `-.`), and an equals sign only when it cannot start a thick link (`==`).
fn is_id_char(c: char, next: Option<char>) -> bool {
    match c {
        '-' => next.is_some_and(|next| !matches!(next, '>' | '-' | '.')),
        '=' => next != Some('='),
        _ => c.is_ascii_alphanumeric() || !c.is_ascii() || "!\"#$%&'*+.`?\\_/".contains(c),
    }
}
