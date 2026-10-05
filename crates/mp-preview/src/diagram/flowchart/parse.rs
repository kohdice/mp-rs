//! Flowchart source text to nodes and edges, following upstream Mermaid's grammar
//! (`packages/mermaid/src/diagrams/flowchart/parser/flow.jison` in
//! <https://github.com/mermaid-js/mermaid>).

use std::collections::{HashMap, HashSet};

use crate::diagram::{Failure, SyntaxError};
use crate::style::Line;

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
    Box { label: String, shape: Shape },
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
    /// `A[(label)]`.
    Cylinder,
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
    pub label: Option<String>,
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
    pub title: String,
    /// Indices into [`Flowchart::nodes`] of the nodes referenced inside the block or
    /// inside the subgraphs nested in it, in order of first reference.
    pub members: Vec<usize>,
    /// Index into [`Flowchart::subgraphs`] of the subgraph this one is nested in.
    pub parent: Option<usize>,
    /// The direction of the block's `direction` statement, if it has one; of several,
    /// the last wins.
    pub direction: Option<Direction>,
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

/// Brackets of upstream node shapes that are not drawn yet: the ellipse. A node using
/// one falls back to the code block, unless the closer is missing, which is a syntax
/// error as for any other bracket. They are checked before `BRACKETS`, whose
/// single-character openers they start with.
const UNSUPPORTED_SHAPE_OPENERS: [(&str, &str); 1] = [("(-", "-)")];

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
        // `curve`) have no text drawing, and upstream ignores the rest. The multi-line
        // form, closed on a later line, is not read.
        let (id, after_id) = split_id(statement);
        if self.edge_ids.contains(id)
            && let Some(data) = after_id.strip_prefix("@{")
        {
            let end = outside_quotes(data, '}').next().ok_or(Failure::Unsupported)?;
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
    /// title. A subgraph opened inside another is nested in it.
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
            None => (unquoted(text), unquoted(text)),
        };
        self.subgraph_ids.push(id);
        let parent = self.open.last().map(|parent| parent.index);
        self.open.push(OpenSubgraph {
            line,
            index: self.chart.subgraphs.len(),
            members: HashSet::new(),
        });
        self.chart.subgraphs.push(Subgraph {
            title: spaced(title),
            members: Vec::new(),
            parent,
            direction: None,
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
                body: Body::Box { label: id.to_owned(), shape: Shape::Rectangle },
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
        if let Some((after_open, close)) = UNSUPPORTED_SHAPE_OPENERS
            .into_iter()
            .find_map(|(open, close)| Some((rest.strip_prefix(open)?, close)))
        {
            if bracket_label(after_open, close).is_none() {
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
                if let Some(Body::Box { label: node_label, shape: node_shape }) =
                    self.chart.nodes.get_mut(index).map(|node| &mut node.body)
                {
                    *node_label = spaced(label);
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
                        *label = spaced(data_label);
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
struct ShapeData<'a> {
    shape: Option<Shape>,
    /// Replaces the label, also one given in brackets.
    label: Option<&'a str>,
}

/// Upstream's names and aliases for the shapes drawn here, the `shape` values of
/// `@{ … }`, from the table "Complete List of New Shapes" in
/// <https://mermaid.js.org/syntax/flowchart.html>.
const SHAPE_NAMES: [(&str, Shape); 44] = [
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
];

/// The rest of the names and aliases in the same table, for shapes not drawn yet.
const UNDRAWN_SHAPE_NAMES: [&str; 96] = [
    "bang",
    "browser",
    "bucket",
    "notch-rect",
    "card",
    "notched-rectangle",
    "cloud",
    "hourglass",
    "collate",
    "bolt",
    "com-link",
    "lightning-bolt",
    "brace",
    "brace-l",
    "comment",
    "brace-r",
    "braces",
    "console",
    "datastore",
    "data-store",
    "delay",
    "half-rounded-rectangle",
    "h-cyl",
    "das",
    "horizontal-cylinder",
    "lin-cyl",
    "disk",
    "lined-cylinder",
    "curv-trap",
    "curved-trapezoid",
    "display",
    "div-rect",
    "div-proc",
    "divided-process",
    "divided-rectangle",
    "doc",
    "document",
    "tri",
    "extract",
    "triangle",
    "folder",
    "directory",
    "fork",
    "join",
    "win-pane",
    "internal-storage",
    "window-pane",
    "f-circ",
    "filled-circle",
    "junction",
    "lin-doc",
    "lined-document",
    "lin-rect",
    "lin-proc",
    "lined-process",
    "lined-rectangle",
    "shaded-process",
    "notch-pent",
    "loop-limit",
    "notched-pentagon",
    "flip-tri",
    "flipped-triangle",
    "manual-file",
    "sl-rect",
    "manual-input",
    "sloped-rectangle",
    "docs",
    "documents",
    "st-doc",
    "stacked-document",
    "st-rect",
    "processes",
    "procs",
    "stacked-rectangle",
    "flag",
    "paper-tape",
    "person",
    "sm-circ",
    "small-circle",
    "start",
    "fr-circ",
    "framed-circle",
    "stop",
    "bow-rect",
    "bow-tie-rectangle",
    "stored-data",
    "cross-circ",
    "crossed-circle",
    "summary",
    "tag-doc",
    "tagged-document",
    "tag-rect",
    "tag-proc",
    "tagged-process",
    "tagged-rectangle",
    "text",
];

/// Reads the `key: value` pairs of `@{ … }` shape data from `text`, which follows the
/// `@{`, and returns them with the text after the closing `}`. Upstream reads the braces
/// as YAML; the pairs are split at commas outside double quotes, and a value is taken
/// literally without the double quotes around it.
fn shape_data(line: usize, text: &str) -> Result<(ShapeData<'_>, &str), Failure> {
    // The multi-line form, closed on a later line, is not read.
    let end = outside_quotes(text, '}').next().ok_or(Failure::Unsupported)?;
    let (body, after_body) = text.split_at(end);
    let mut data = ShapeData { shape: None, label: None };
    let mut start = 0;
    for comma in outside_quotes(body, ',').chain([body.len()]) {
        let pair = body.get(start..comma).ok_or(Failure::Unsupported)?;
        start = comma + 1;
        let Some((key, value)) = pair.split_once(':') else { continue };
        let value = unquoted(value.trim());
        match key.trim() {
            "shape" => data.shape = Some(shape_named(line, value)?),
            "label" => data.label = Some(value),
            // An image has no text drawing yet.
            "icon" | "img" => return Err(Failure::Unsupported),
            // Upstream ignores keys it does not know, and the rest only size or place
            // icons and images.
            _ => {}
        }
    }
    Ok((data, after_body.get(1..).ok_or(Failure::Unsupported)?))
}

/// The shape a `shape` value names. Another name in upstream's list is not drawn yet
/// and falls back; a name outside the list is a syntax error, as upstream throws `No
/// such shape`, which also covers names that are not lowercase.
fn shape_named(line: usize, name: &str) -> Result<Shape, Failure> {
    if let Some((_, shape)) = SHAPE_NAMES.into_iter().find(|&(known, _)| known == name) {
        return Ok(shape);
    }
    if UNDRAWN_SHAPE_NAMES.contains(&name) {
        return Err(Failure::Unsupported);
    }
    Err(syntax_error(line, &format!("no such shape \"{name}\"")))
}

/// The byte index of each `c` in `text` that is outside double quotes.
fn outside_quotes(text: &str, c: char) -> impl Iterator<Item = usize> + '_ {
    let mut in_quotes = false;
    text.char_indices().filter_map(move |(index, found)| {
        if found == '"' {
            in_quotes = !in_quotes;
        }
        (found == c && !in_quotes).then_some(index)
    })
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
