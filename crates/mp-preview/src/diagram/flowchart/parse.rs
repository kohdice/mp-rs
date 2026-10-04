//! Flowchart source text to nodes and edges, following upstream Mermaid's grammar
//! (`packages/mermaid/src/diagrams/flowchart/parser/flow.jison` in
//! <https://github.com/mermaid-js/mermaid>).

use std::collections::{HashMap, HashSet};

use crate::diagram::{Failure, SyntaxError};

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

#[derive(Debug)]
pub(super) struct Node {
    pub label: String,
    pub shape: Shape,
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
    /// `A[(label)]`.
    Cylinder,
}

#[derive(Debug)]
pub(super) struct Edge {
    /// Index into [`Flowchart::nodes`].
    pub from: usize,
    /// Index into [`Flowchart::nodes`].
    pub to: usize,
    pub stroke: Stroke,
    /// The marker drawn where the link leaves the source, if any.
    pub tail: Option<Marker>,
    /// The marker drawn where the link meets the target, if any.
    pub head: Option<Marker>,
    pub label: Option<String>,
    /// The fewest layers the link spans: one, plus one for each extra `-`, `=` or `.`.
    pub length: usize,
}

/// A `subgraph … end` block, drawn as a titled frame.
#[derive(Debug)]
pub(super) struct Subgraph {
    pub title: String,
    /// Indices into [`Flowchart::nodes`] of the nodes referenced inside the block, in
    /// order of first reference.
    pub members: Vec<usize>,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(super) enum Stroke {
    /// `--`.
    Solid,
    /// `-.`.
    Dotted,
    /// `==`.
    Thick,
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
const BRACKETS: [(&str, &[(&str, Shape)]); 12] = [
    ("[(", &[(")]", Shape::Cylinder)]),
    ("(((", &[(")))", Shape::DoubleCircle)]),
    ("((", &[("))", Shape::Circle)]),
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

/// Openers of upstream node shapes that are not drawn yet — the ellipse and
/// `@{ shape: … }` — with the closer that must follow on the same line, if any. A node
/// using one falls back to the code block, unless that closer is missing, which is a
/// syntax error as for any other bracket; `@{` may also close on a later line. They are
/// checked before `BRACKETS`, whose single-character openers they start with.
const UNSUPPORTED_SHAPE_OPENERS: [(&str, Option<&str>); 2] = [("(-", Some("-)")), ("@{", None)];

/// Mermaid's default `flowchart.maxEdges`.
const MAX_EDGES: usize = 500;

/// Statements that only affect colors or interaction, which a text drawing cannot show.
const IGNORED_KEYWORDS: [&str; 5] = ["style", "classDef", "class", "linkStyle", "click"];

pub(super) fn parse(source: &str) -> Result<Flowchart, Failure> {
    let mut statements = source
        .lines()
        .zip(1..)
        .flat_map(|(text, line)| split_statements(text).into_iter().map(move |text| (line, text)))
        .map(|(line, text)| (line, text.trim()))
        .filter(|(_, text)| !text.is_empty());
    let (header_line, header) = statements.next().ok_or(Failure::Unsupported)?;
    let direction = direction(header_line, header)?;
    let mut builder = Builder {
        chart: Flowchart { direction, nodes: Vec::new(), edges: Vec::new(), subgraphs: Vec::new() },
        index_of: HashMap::new(),
        open: None,
        subgraph_ids: Vec::new(),
    };
    for (line, text) in statements {
        builder.statement(line, text)?;
    }
    if let Some((line, ..)) = builder.open {
        return Err(syntax_error(line, "subgraph is not closed with \"end\""));
    }
    // Mermaid draws an empty subgraph as a frame of its own, but without members there
    // is no layer to place it on.
    if builder.refers_to_a_subgraph()
        || builder.chart.subgraphs.iter().any(|subgraph| subgraph.members.is_empty())
    {
        return Err(Failure::Unsupported);
    }
    Ok(builder.chart)
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

/// Splits one source line at each `;` and drops a `%%` comment, ignoring both inside
/// double quotes.
fn split_statements(line: &str) -> Vec<&str> {
    let mut statements = Vec::new();
    let mut in_quotes = false;
    let mut start = 0;
    let mut end = line.len();
    let mut chars = line.char_indices().peekable();
    while let Some((index, c)) = chars.next() {
        match c {
            '"' => in_quotes = !in_quotes,
            ';' if !in_quotes => {
                statements.push(&line[start..index]);
                start = index + 1;
            }
            '%' if !in_quotes && chars.peek().is_some_and(|&(_, next)| next == '%') => {
                end = index;
                break;
            }
            _ => {}
        }
    }
    statements.push(&line[start..end]);
    statements
}

struct Builder<'a> {
    chart: Flowchart,
    index_of: HashMap<&'a str, usize>,
    /// The subgraph whose `end` has not been read yet, with the line it starts on and
    /// the nodes already among its members.
    open: Option<(usize, Subgraph, HashSet<usize>)>,
    /// The id of every subgraph, which Mermaid lets a link point to.
    subgraph_ids: Vec<&'a str>,
}

impl<'a> Builder<'a> {
    /// A `subgraph` or `end` line, or a group of nodes optionally followed by links to
    /// further groups; a link joins every node of the group before it to every node of
    /// the group after it.
    /// `line` is the statement's 1-based line number in the source, for error reports.
    fn statement(&mut self, line: usize, statement: &'a str) -> Result<(), Failure> {
        if let Some(rest) = statement.strip_prefix("subgraph")
            && (rest.is_empty() || rest.starts_with(|c: char| c.is_ascii_whitespace()))
        {
            return self.open_subgraph(line, rest);
        }
        if statement == "end" {
            let (_, closed, _) = self
                .open
                .take()
                .ok_or_else(|| syntax_error(line, "\"end\" without an open subgraph"))?;
            self.chart.subgraphs.push(closed);
            return Ok(());
        }
        if statement
            .split_ascii_whitespace()
            .next()
            .is_some_and(|word| IGNORED_KEYWORDS.contains(&word))
        {
            return Ok(());
        }
        // `direction` inside a subgraph sets its own layout, which is not drawn; Mermaid
        // reads it only there and ignores it at the top level.
        if statement.split_ascii_whitespace().next() == Some("direction") {
            if self.open.is_some() {
                return Err(Failure::Unsupported);
            }
            return Ok(());
        }
        let (mut sources, mut rest) = self.group(line, statement)?;
        while !rest.is_empty() {
            let Some((Link { stroke, tail, head, label, length }, after_link)) = link(rest) else {
                // Line characters that do not make a whole link are a syntax error in
                // Mermaid, and so is an id where a link should be, unless an `@` makes it
                // an edge id (`A e1@--> B`), which is not drawn.
                if rest.strip_prefix('<').unwrap_or(rest).starts_with(['-', '=']) {
                    return Err(syntax_error(line, "unclosed link"));
                }
                let (id, after_id) = split_id(rest);
                if !id.is_empty() && !after_id.starts_with('@') {
                    return Err(syntax_error(line, "expected a link"));
                }
                return Err(Failure::Unsupported);
            };
            let (label, after_link) = match after_link.trim_start().strip_prefix('|') {
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
                        from,
                        to,
                        stroke,
                        tail,
                        head,
                        label: label.map(spaced),
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
    /// or else the rest of the statement, without the quotes around it, as both id and
    /// title. A subgraph inside another is not drawn.
    fn open_subgraph(&mut self, line: usize, text: &'a str) -> Result<(), Failure> {
        if self.open.is_some() {
            return Err(Failure::Unsupported);
        }
        let text = text.trim();
        let quoted = text.strip_prefix('"').and_then(|rest| rest.split_once('"'));
        let (id, after_id) = quoted.unwrap_or_else(|| split_id(text));
        let (id, title) = match after_id.trim_start().strip_prefix('[') {
            Some(after_open) => {
                let (title, _) = bracket_label(after_open, "]")
                    .ok_or_else(|| syntax_error(line, "unclosed subgraph title"))?;
                (id, title)
            }
            None => (unquoted(text), unquoted(text)),
        };
        self.subgraph_ids.push(id);
        let subgraph = Subgraph { title: spaced(title), members: Vec::new() };
        self.open = Some((line, subgraph, HashSet::new()));
        Ok(())
    }

    /// Whether a link starts or ends at, or a subgraph lists as a member, a node whose
    /// id is also a subgraph's: Mermaid takes such a reference to mean the subgraph,
    /// drawing a link to its frame or nesting its frame inside the other, neither of
    /// which is drawn.
    fn refers_to_a_subgraph(&self) -> bool {
        let mut is_referenced = vec![false; self.chart.nodes.len()];
        let ends = self.chart.edges.iter().flat_map(|edge| [edge.from, edge.to]);
        let members =
            self.chart.subgraphs.iter().flat_map(|subgraph| subgraph.members.iter().copied());
        for node in ends.chain(members) {
            if let Some(referenced) = is_referenced.get_mut(node) {
                *referenced = true;
            }
        }
        self.subgraph_ids.iter().any(|id| {
            self.index_of.get(id).is_some_and(|&node| is_referenced.get(node) == Some(&true))
        })
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
            self.chart.nodes.push(Node { label: id.to_owned(), shape: Shape::Rectangle });
            self.chart.nodes.len() - 1
        });
        if let Some((_, open, members)) = &mut self.open
            && members.insert(index)
        {
            open.members.push(index);
        }
        if let Some((after_open, close)) = UNSUPPORTED_SHAPE_OPENERS
            .into_iter()
            .find_map(|(open, close)| Some((rest.strip_prefix(open)?, close)))
        {
            if close.is_some_and(|close| bracket_label(after_open, close).is_none()) {
                return Err(syntax_error(line, "unclosed node label"));
            }
            return Err(Failure::Unsupported);
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
                if let Some(node) = self.chart.nodes.get_mut(index) {
                    node.label = spaced(label);
                    node.shape = shape;
                }
                after_label
            }
            None => rest,
        };
        let rest = rest.strip_prefix(":::").map_or(rest, |class| split_id(class).1);
        Ok((index, rest.trim_start()))
    }
}

/// The parts of a link token that a drawing shows.
struct Link<'a> {
    stroke: Stroke,
    tail: Option<Marker>,
    head: Option<Marker>,
    /// The text of the `A -- text --> B` form.
    label: Option<&'a str>,
    length: usize,
}

/// Splits a link off the start of `text`. These are the `LINK` tokens of Mermaid's
/// `flow.jison`, `[xo<]?--+[-xo>]`, `[xo<]?==+[=xo>]` and `[xo<]?-\.+-[xo>]?`, and their
/// text forms `-- text -->`, `== text ==>` and `-. text .->`.
fn link(text: &str) -> Option<(Link<'_>, &str)> {
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
            (Some(unquoted(label.trim())), closing.get(1..)?)
        }
        Stroke::Solid | Stroke::Thick
            if !after_base.starts_with(|c| c == line_char(stroke) || head_marker(c).is_some()) =>
        {
            let doubled = if stroke == Stroke::Solid { "--" } else { "==" };
            let (label, closing) =
                after_base.split_at_checked(find_after_quotes(after_base, doubled)?)?;
            (Some(unquoted(label.trim())), closing.get(2..)?)
        }
        Stroke::Dotted => return None,
        Stroke::Solid | Stroke::Thick => (None, after_base),
    };
    let (head, length, rest) = link_end(stroke, closing)?;
    Some((Link { stroke, tail, head, label, length: length + dots }, rest))
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

/// `text` without the double quotes around it when the whole of it is one quoted
/// string, the `STR` token of Mermaid's `flow.jison`, which holds no quote itself.
fn unquoted(text: &str) -> &str {
    text.strip_prefix('"')
        .and_then(|rest| rest.strip_suffix('"'))
        .filter(|inner| !inner.contains('"'))
        .unwrap_or(text)
}

/// The character repeated in a link of `stroke`, after its first two characters.
fn line_char(stroke: Stroke) -> char {
    match stroke {
        Stroke::Solid | Stroke::Dotted => '-',
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

/// `label` with each tab replaced by a space: a tab has no display width of its own, so
/// it would leave the box narrower than the text shown in it.
fn spaced(label: &str) -> String {
    label.replace('\t', " ")
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
/// the `close` bracket. A label in double quotes may contain brackets; the quotes are
/// not part of it.
fn bracket_label<'a>(text: &'a str, close: &str) -> Option<(&'a str, &'a str)> {
    match text.strip_prefix('"') {
        Some(quoted) => {
            let (label, after_quote) = quoted.split_once('"')?;
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
