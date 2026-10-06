//! Flowchart source text to nodes and edges, following upstream Mermaid's grammar
//! (`packages/mermaid/src/diagrams/flowchart/parser/flow.jison` in
//! <https://github.com/mermaid-js/mermaid>).

use std::borrow::Cow;
use std::collections::{HashMap, HashSet};

use crate::diagram::{Failure, SyntaxError};
use crate::style::Line;

use super::label::{Label, is_entity_name_char, shown_text, unquoted};

#[derive(Debug)]
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
    /// every link end on one of its borders has a cell of its own; set by
    /// [`grow_boxes`](super::layout::grow_boxes).
    pub spread: usize,
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
    /// [`embed_units`](super::unit::embed_units).
    Drawing(Vec<Line>),
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
    /// The fewest layers the link spans: one, plus one for each extra `-`, `=` or `.`.
    pub length: usize,
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
    /// every link end on one of its borders has a cell of its own; set by
    /// [`grow_frames`](super::layout::grow_frames).
    pub spread: usize,
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

/// Statements that only affect colors or interaction, which a text drawing cannot show.
const IGNORED_KEYWORDS: [&str; 5] = ["style", "classDef", "class", "linkStyle", "click"];

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
/// `title`. The block opens with a first non-blank line `---`, which may be indented, and
/// closes with the next line that is the same indent followed by `---`, as
/// `frontMatterRegex` in `diagram-api/regexes.ts` (`([^\S\n\r]*)-{3}` … `\1-{3}`) reads
/// it; that indent is removed from each line between before its keys are read, and a
/// line without it is skipped. Every key but `title` is skipped: upstream applies
/// `config` (theme, curve, HTML labels, …) to the SVG it draws, and none of it has a
/// counterpart in box-drawing text.
pub(super) fn front_matter(source: &str) -> Result<FrontMatter<'_>, Failure> {
    let none = FrontMatter { title: None, body: source, first_line: 1 };
    let mut lines = source.split_inclusive('\n').zip(1..);
    let mut offset = 0;
    let (opening, indent) = loop {
        let Some((line, number)) = lines.next() else { return Ok(none) };
        offset += line.len();
        if !line.trim().is_empty() {
            let fence =
                line.trim_start_matches(|c: char| c.is_whitespace() && c != '\n' && c != '\r');
            if !is_fence(fence) {
                return Ok(none);
            }
            break (number, line.get(..line.len() - fence.len()).unwrap_or_default());
        }
    };
    let mut title = None;
    for (line, number) in lines {
        offset += line.len();
        // A line without the indent is neither the closing fence, which `\1-{3}` needs
        // to repeat it, nor a top-level key.
        let Some(line) = line.strip_prefix(indent) else { continue };
        if is_fence(line) {
            let body = source.get(offset..).unwrap_or_default();
            return Ok(FrontMatter { title, body, first_line: number + 1 });
        }
        if let Some(value) = title_value(line) {
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
    let mut statements = split_statements(source, first_line)
        .into_iter()
        .map(|(line, text)| (line, text.trim()))
        .filter(|(_, text)| !text.is_empty());
    let (header_line, header) = statements.next().ok_or(Failure::Unsupported)?;
    let direction = direction(header_line, header)?;
    let mut builder = Builder {
        chart: Flowchart { direction, nodes: Vec::new(), edges: Vec::new(), subgraphs: Vec::new() },
        index_of: HashMap::new(),
        open: Vec::new(),
        subgraph_ids: Vec::new(),
        edge_ids: HashSet::new(),
    };
    for (line, text) in statements {
        builder.statement(line, text)?;
    }
    if let Some(open) = builder.open.last() {
        return Err(syntax_error(open.line, "subgraph is not closed with \"end\""));
    }
    builder.resolve_subgraph_ids()?;
    let mut chart = builder.chart;
    // Mermaid draws an empty subgraph as a frame showing only its title, placed in the
    // flow like a node: a hidden member gives it a place and its frame the least size.
    // Inner subgraphs come first, so that a subgraph holding only an empty one is not
    // empty itself.
    for subgraph in chart.innermost_first() {
        if chart.subgraphs.get(subgraph).is_some_and(|subgraph| subgraph.members.is_empty()) {
            let hidden = chart.nodes.len();
            chart.nodes.push(Node { body: Body::Hidden, spread: 0 });
            for holder in std::iter::once(subgraph).chain(chart.enclosing(subgraph)) {
                if let Some(holder) = chart.subgraphs.get_mut(holder) {
                    holder.members.push(hidden);
                }
            }
        }
    }
    Ok(chart)
}

impl Flowchart {
    /// The subgraphs `subgraph` is nested in, from its parent outwards.
    pub(super) fn enclosing(&self, subgraph: usize) -> Vec<usize> {
        let mut enclosing = Vec::new();
        let mut current = subgraph;
        // Parsing leaves no cycle of parents; the bound keeps a walk finite regardless.
        while let Some(parent) = self.subgraphs.get(current).and_then(|current| current.parent)
            && enclosing.len() < self.subgraphs.len()
        {
            enclosing.push(parent);
            current = parent;
        }
        enclosing
    }

    /// Every subgraph's index, those nested deeper first.
    pub(super) fn innermost_first(&self) -> Vec<usize> {
        let mut order: Vec<usize> = (0..self.subgraphs.len()).collect();
        order.sort_by_key(|&subgraph| std::cmp::Reverse(self.enclosing(subgraph).len()));
        order
    }
}

/// Reads the direction of a `flowchart` or `graph` header; `TB`, `v` and an omitted
/// direction mean top-down, and `>`, `<` and `^` mean `LR`, `RL` and `BT`, as Mermaid
/// does. Other diagram kinds are unsupported. Like Mermaid, the direction must end the
/// statement: anything after it on the same line needs a `;` first.
fn direction(line: usize, header: &str) -> Result<Direction, Failure> {
    let mut words = header.split_ascii_whitespace();
    if !matches!(words.next(), Some("flowchart" | "graph")) {
        return Err(Failure::Unsupported);
    }
    let direction = match words.next() {
        Some("LR" | ">") => Direction::LeftToRight,
        None | Some("TD" | "TB" | "v") => Direction::TopDown,
        Some("BT" | "^") => Direction::BottomUp,
        Some("RL" | "<") => Direction::RightToLeft,
        Some(other) => return Err(syntax_error(line, &format!("unknown direction \"{other}\""))),
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

/// Splits `source`, whose first line is line `first_line`, into statements, each with the
/// 1-based line it starts on, for error reports: at each line break and `;`, and drops
/// each `%%` comment up to the end of its line and the text [`passed_over`] names,
/// ignoring all of them inside double quotes, so that a quoted string may span lines as
/// `flow.jison`'s `string` lexer state lets it, inside `@{ … }` shape data up to its
/// closing brace (see [`shape_data_end`]), which upstream's `shapeData` lexer state reads
/// across lines, and inside the text of a `-- text -->` link on its line. The `;` closing
/// an entity code such as `#quot;` splits nothing: upstream's `encodeEntities` in
/// `packages/mermaid/src/utils.ts` replaces every `#\w+;` in the source before parsing
/// it.
fn split_statements(source: &str, first_line: usize) -> Vec<(usize, &str)> {
    let mut statements: Vec<(usize, &str)> = Vec::new();
    let mut in_quotes = false;
    let mut line = first_line;
    // The line and byte index the current statement starts at.
    let (mut start_line, mut start) = (first_line, 0);
    // Where a `%%` comment cut the current statement short.
    let mut comment: Option<usize> = None;
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
            '"' => in_quotes = !in_quotes,
            ';' => ends_statement = !in_quotes && !entity_name.is_some_and(|length| length > 0),
            '%' | 'a' if !in_quotes => {
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
                    None if c == '%' && rest.starts_with("%%") => comment = Some(index),
                    None => {}
                }
            }
            '-' | '=' if !in_quotes => {
                // Upstream reads the text of a `-- text -->` link in exclusive lexer
                // states (`edgeText` and its thick and dotted kin), where `@{`, `;` and
                // `%%` are text, so the link is passed over whole, up to its end on the
                // line.
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
                    // upstream's lexer takes the run as one `LINK` token.
                    _ => while chars.next_if(|&(_, next)| next == c).is_some() {},
                }
            }
            '@' if !in_quotes && chars.peek().is_some_and(|&(_, next)| next == '{') => {
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
            let end = comment.take().unwrap_or(index);
            let text = source.get(start..end).unwrap_or_default();
            after_header |= !text.trim().is_empty();
            statements.push((start_line, text));
            start = index + c.len_utf8();
            start_line = line + usize::from(c == '\n');
        }
        blank = ends_statement || restarts || (blank && c.is_whitespace());
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

/// The byte length of the text at the start of `rest` that [`split_statements`] passes
/// over whole, ending the statement before it, where `after_header` tells whether the
/// header has been read and `blank` whether `rest` starts a statement:
///
/// - A directive, `%%{` and a keyword (`init: …`), on one line or several, up to and
///   including its `}%%`: upstream applies its configuration to the SVG, which a text
///   drawing has no counterpart for. Upstream's `directiveRegex` (`%{2}{\s*(?:(\w+)\s*:|
///   (\w+))`, `packages/mermaid/src/diagram-api/regexes.ts`) needs the keyword, so `%%{`
///   without one is a comment. An unclosed directive runs to the end of the source.
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
            self.open
                .pop()
                .ok_or_else(|| syntax_error(line, "\"end\" without an open subgraph"))?;
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
                // Mermaid, and so is an id where a link should be. An id followed by `@`
                // may be the edge id of a link form not read here, so it falls back
                // rather than being reported as a syntax error.
                if rest.strip_prefix('<').unwrap_or(rest).starts_with(['-', '=']) {
                    return Err(syntax_error(line, "unclosed link"));
                }
                let (id, after_id) = split_id(rest);
                if !id.is_empty() && !after_id.starts_with('@') {
                    return Err(syntax_error(line, "expected a link"));
                }
                return Err(Failure::Unsupported);
            };
            if let Some(id) = id {
                self.edge_ids.insert(id);
            }
            let (label, after_link) = match after_link.trim_start().strip_prefix('|') {
                // Mermaid's grammar accepts a `|text|` label after `~~~`, but a link
                // that is not drawn has no line to carry the text, so the block falls
                // back rather than dropping the text.
                Some(_) if !stroke.is_visible() => return Err(Failure::Unsupported),
                Some(after_pipe) => {
                    let (label, after_label) = bracket_label(after_pipe, "|")
                        .ok_or_else(|| syntax_error(line, "unclosed edge label"))?;
                    (Some(label.trim()), after_label.trim_start())
                }
                None => (label, after_link.trim_start()),
            };
            if after_link.is_empty() {
                return Err(syntax_error(line, "edge has no target"));
            }
            let (targets, after_targets) = self.group(line, after_link)?;
            for &from in &sources {
                for &to in &targets {
                    if self.chart.edges.len() == MAX_EDGES {
                        return Err(syntax_error(
                            line,
                            &format!("too many edges (limit {MAX_EDGES})"),
                        ));
                    }
                    self.chart.edges.push(Edge {
                        from: End::Node(from),
                        to: End::Node(to),
                        stroke,
                        tail,
                        head,
                        label: label.map(Label::parse),
                        length,
                    });
                }
            }
            (sources, rest) = (targets, after_targets);
        }
        Ok(())
    }

    /// Starts the subgraph declared by `text`, the part of a `subgraph` statement after
    /// the keyword: an id, which may be a quoted string, followed by a bracketed title,
    /// or else the rest of the statement as the id, without the quotes around it, and as
    /// the title, quotes and all, for [`Label::parse`] to read. A subgraph opened inside
    /// another is nested in it.
    fn open_subgraph(&mut self, line: usize, text: &'a str) -> Result<(), Failure> {
        let text = text.trim();
        let quoted = text.strip_prefix('"').and_then(|rest| rest.split_once('"'));
        let (id, after_id) = quoted.unwrap_or_else(|| split_id(text));
        let (id, title) = match after_id.trim_start().strip_prefix('[') {
            Some(after_open) => {
                let (title, _) = bracket_label(after_open, "]")
                    .ok_or_else(|| syntax_error(line, "unclosed subgraph title"))?;
                (id, title)
            }
            None => (unquoted(text), text),
        };
        self.subgraph_ids.push(id);
        let parent = self.open.last().map(|parent| parent.index);
        self.open.push(OpenSubgraph {
            line,
            index: self.chart.subgraphs.len(),
            members: HashSet::new(),
        });
        self.chart.subgraphs.push(Subgraph {
            title: Label::parse(title).joined(),
            members: Vec::new(),
            parent,
            direction: None,
            spread: 0,
        });
        Ok(())
    }

    /// Turns every reference to an id that is also a subgraph's, declared before or
    /// after it, into a reference to the subgraph, as Mermaid reads it: a link end
    /// becomes [`End::Subgraph`], a subgraph listed inside another's block is nested in
    /// it, and the node declared for the id is dropped. A subgraph listed in two blocks
    /// that do not nest, or nested in itself through such lists, is not drawn.
    fn resolve_subgraph_ids(&mut self) -> Result<(), Failure> {
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
            let Some(&holder) =
                holders.iter().max_by_key(|&&holder| self.chart.enclosing(holder).len())
            else {
                continue;
            };
            let enclosing = self.chart.enclosing(holder);
            if holders.iter().any(|other| *other != holder && !enclosing.contains(other)) {
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
            .any(|subgraph| self.chart.enclosing(subgraph).contains(&subgraph))
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
        let mut renumbered = Vec::with_capacity(named.len());
        let mut kept = 0;
        for names in &named {
            renumbered.push(kept);
            kept += usize::from(names.is_none());
        }
        let resolve = |end: End| match end {
            End::Node(node) => match named.get(node).copied().flatten() {
                Some(subgraph) => Some(End::Subgraph(subgraph)),
                None => Some(End::Node(*renumbered.get(node)?)),
            },
            End::Subgraph(_) => Some(end),
        };
        for edge in &mut self.chart.edges {
            edge.from = resolve(edge.from).ok_or(Failure::Unsupported)?;
            edge.to = resolve(edge.to).ok_or(Failure::Unsupported)?;
        }
        for member in self.chart.subgraphs.iter_mut().flat_map(|subgraph| &mut subgraph.members) {
            *member = *renumbered.get(*member).ok_or(Failure::Unsupported)?;
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
        let index = *self.index_of.entry(id).or_insert_with(|| {
            self.chart.nodes.push(Node {
                body: Body::Box { label: Label::plain(id), shape: Shape::Rectangle },
                spread: 0,
            });
            self.chart.nodes.len() - 1
        });
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
            .find_map(|(open, closers)| Some((rest.strip_prefix(open)?, closers)));
        let rest = match bracketed {
            Some((after_open, closers)) => {
                let (label, shape, after_label) = closers
                    .iter()
                    .filter_map(|&(close, shape)| {
                        let (label, after_label) = bracket_label(after_open, close)?;
                        Some((label, shape, after_label))
                    })
                    // The closer found first ends the label.
                    .min_by_key(|(label, ..)| label.len())
                    .ok_or_else(|| syntax_error(line, "unclosed node label"))?;
                if let Some(Body::Box { label: node_label, shape: node_shape }) =
                    self.chart.nodes.get_mut(index).map(|node| &mut node.body)
                {
                    *node_label = Label::parse(label);
                    *node_shape = shape;
                }
                after_label
            }
            None => rest,
        };
        let rest = rest.strip_prefix(":::").map_or(rest, |class| split_id(class).1);
        let rest = match rest.strip_prefix("@{") {
            Some(after_open) => {
                let (data, after_data) = shape_data(line, after_open)?;
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
}

/// Upstream's short names and aliases of every shape, the `shape` values of `@{ … }`,
/// from `shapesDefs` in `packages/mermaid/src/rendering-util/rendering-elements/shapes.ts`
/// in <https://github.com/mermaid-js/mermaid>, also listed in the table "Complete List of
/// New Shapes" in <https://mermaid.js.org/syntax/flowchart.html>.
const SHAPE_NAMES: [(&str, Shape); 141] = [
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
];

/// The byte index of the `}` closing the shape data `text`, which follows the `@{`.
fn shape_data_end(text: &str) -> Option<usize> {
    scan_shape_data(text).map(|(_, end)| end)
}

/// Reads the `key: value` pairs of `@{ … }` shape data from `text`, which follows the
/// `@{`, and returns them with the text after the closing `}`. Text between a quoted
/// value and the next separator is a syntax error, as YAML rejects two pairs without a
/// separator between them. An empty `shape` or `label` sets nothing, as upstream's
/// `addVertex` in `flowDb.ts` skips a falsy one.
fn shape_data(line: usize, text: &str) -> Result<(ShapeData, &str), Failure> {
    let (pairs, end) =
        scan_shape_data(text).ok_or_else(|| syntax_error(line, "unclosed shape data"))?;
    if pairs.iter().any(|pair| !pair.after_value.trim().is_empty()) {
        return Err(syntax_error(line, "invalid shape data"));
    }
    let mut data = ShapeData { shape: None, label: None };
    let mut image = false;
    for DataPair { key, value, .. } in pairs {
        match key {
            "shape" | "label" if value.text().is_empty() => {}
            "shape" => data.shape = Some(shape_named(line, &value.text())?),
            "label" => data.label = Some(value.label()),
            // An image has no text drawing; every other pair is checked first, so that
            // an error beside it is reported whatever the order of the keys.
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
// Upstream registers each shape's short name, `aliases` and `internalAliases` as keys of
// the `shapes` map that `isValidShape` checks (`shape in shapes`, `shapes.ts`), and
// `flowDb` rejects only a name holding an uppercase letter or `_` before asking it. That
// leaves `doublecircle` the one internal alias upstream accepts, so it is accepted here.
fn shape_named(line: usize, name: &str) -> Result<Shape, Failure> {
    SHAPE_NAMES
        .into_iter()
        .find_map(|(known, shape)| (known == name).then_some(shape))
        .ok_or_else(|| syntax_error(line, &format!("no such shape \"{name}\"")))
}

/// The parts of a link token that a drawing shows.
struct Link<'a> {
    stroke: Stroke,
    tail: Option<Marker>,
    head: Option<Marker>,
    /// The text of the `A -- text --> B` form, with any double quotes around it.
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
        let link = Link { stroke: Stroke::Invisible, tail: None, head: None, label: None, length };
        return (length > 0).then_some((link, rest));
    }
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
    let after_dots = after_base.trim_start_matches('.');
    let dots = after_base.len() - after_dots.len();
    let (label, closing) = match stroke {
        Stroke::Dotted if after_dots.starts_with('-') => (None, after_dots),
        Stroke::Dotted if dots == 0 => {
            let (label, closing) =
                after_base.split_at_checked(find_after_quotes(after_base, ".-")?)?;
            if label.ends_with('.') {
                return None;
            }
            (Some(label.trim()), closing.get(1..)?)
        }
        Stroke::Solid | Stroke::Thick
            if !after_base.starts_with(|c| c == line_char(stroke) || head_marker(c).is_some()) =>
        {
            let doubled = if stroke == Stroke::Solid { "--" } else { "==" };
            let (label, closing) =
                after_base.split_at_checked(find_after_quotes(after_base, doubled)?)?;
            (Some(label.trim()), closing.get(2..)?)
        }
        Stroke::Dotted | Stroke::Invisible => return None,
        Stroke::Solid | Stroke::Thick => (None, after_base),
    };
    let (head, length, rest) = link_end(stroke, closing)?;
    Some((Link { stroke, tail, head, label, length: length + dots }, rest))
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

fn syntax_error(line: usize, message: &str) -> Failure {
    Failure::Syntax(SyntaxError { line: Some(line), message: message.to_owned() })
}

/// Splits the longest id prefix off `text`.
fn split_id(text: &str) -> (&str, &str) {
    let end = text
        .char_indices()
        .find(|&(index, c)| !is_id_char(c, text[index + c.len_utf8()..].chars().next()))
        .map_or(text.len(), |(index, _)| index);
    text.split_at(end)
}

/// Splits `text`, which follows an opening bracket, into the label and the text after
/// the `close` bracket. A label in double quotes may contain brackets; it keeps its
/// quotes, which tell [`Label::parse`] a quoted string, such as a markdown string, from
/// plain text.
fn bracket_label<'a>(text: &'a str, close: &str) -> Option<(&'a str, &'a str)> {
    match text.strip_prefix('"') {
        Some(quoted) => {
            let (inner, after_quote) = quoted.split_once('"')?;
            let label = text.get(..inner.len() + 2)?;
            Some((label, after_quote.strip_prefix(close)?))
        }
        None => text.split_once(close),
    }
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
