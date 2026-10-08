//! Building the chart from its statements: nodes, links, subgraphs and styles.

use std::collections::{HashMap, HashSet};

use crate::diagram::Failure;
use crate::diagram::flowchart::label::{Label, unquoted};
use crate::diagram::flowchart::styling::Styling;

use super::link::{Link, edge_id, link};
use super::shape_data::{shape_data, shape_data_end};
use super::statements::keyword_statement;
use super::tokens::{
    BRACKETS, LABEL_TOKENS, bracket_label, check_label, check_link_text, check_trap_label,
    check_unbracketed_title, split_id,
};
use super::{
    Body, Edge, End, Flowchart, Node, Shape, Subgraph, compact_index, direction_word, syntax_error,
};

/// Mermaid's default `maxEdges`, a top-level configuration key like `maxTextSize`.
const MAX_EDGES: usize = 500;

/// Statements that only affect interaction, which a text drawing cannot show.
const IGNORED_KEYWORDS: [&str; 1] = ["click"];

/// The classes upstream's `getData` compiles for every node before the node's own,
/// so that `classDef default` and `classDef node` style every node, its own classes
/// overriding them.
const NODE_CLASSES: [&str; 2] = ["default", "node"];

pub(super) struct Builder<'a> {
    pub(super) chart: Flowchart,
    pub(super) index_of: HashMap<&'a str, usize>,
    /// The subgraphs whose `end` has not been read yet, innermost last.
    pub(super) open: Vec<OpenSubgraph>,
    /// The id of every subgraph, indexed like [`Flowchart::subgraphs`], which Mermaid
    /// lets a link point to.
    pub(super) subgraph_ids: Vec<&'a str>,
    /// The ids given to links (`A e1@--> B`), which an `e1@{ … }` statement refers to.
    pub(super) edge_ids: HashSet<&'a str>,
    /// The ids of closed subgraphs whose last `view` in `@{ … }` data, given after their
    /// `end`, is `collapsed`.
    pub(super) collapsed: HashSet<&'a str>,
    /// The ids of the subgraphs whose `end` has been read, which upstream's
    /// `addSubGraph` registers only then.
    pub(super) closed_subgraphs: HashSet<&'a str>,
    /// What the `style` statements naming each id set, the later ones over the earlier.
    pub(super) styles: HashMap<&'a str, Styling>,
    /// What the `classDef` statements naming each class set, the later ones over the
    /// earlier.
    pub(super) class_defs: HashMap<&'a str, Styling>,
    /// The classes attached to each id, in the order they were attached.
    pub(super) classes: HashMap<&'a str, Vec<&'a str>>,
    /// The id of each link, indexed like [`Flowchart::edges`].
    pub(super) edge_names: Vec<Option<&'a str>>,
    /// What the last `linkStyle` statement naming each link's index sets.
    pub(super) link_styles: HashMap<usize, Styling>,
    /// What the last `linkStyle default` statement sets.
    pub(super) default_link_style: Styling,
}

/// A subgraph whose `end` has not been read yet.
pub(super) struct OpenSubgraph {
    /// The line its `subgraph` statement is on, for error reports.
    pub(super) line: usize,
    /// Its index into [`Flowchart::subgraphs`].
    index: usize,
    /// The nodes already among its members.
    members: HashSet<usize>,
}

impl<'a> Builder<'a> {
    /// Applies one statement, whose first word picks its form: `subgraph`, `end`, `style`,
    /// `classDef`, `linkStyle`, `class`, `click` (ignored), `direction`, link data
    /// (`e1@{ … }`), or else a group of nodes optionally followed by links to further
    /// groups, where a link joins every node of the group before it to every node of the
    /// group after it. `line` is the statement's 1-based line number in the source, for
    /// error reports.
    pub(super) fn statement(&mut self, line: usize, statement: &'a str) -> Result<(), Failure> {
        // A closed `accDescr {` block never reaches here
        // (`statements::accessibility_end` passes it over); an unclosed one is an error,
        // as upstream's `acc_descr_multiline` lexer state reads up to a `}` and fails at
        // the end of the text.
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
            // Upstream reads an unbracketed title as `textNoTags`, not as a bracket label:
            // see `check_unbracketed_title` for what it rejects. A leading string and the
            // text after it are joined, so `subgraph "a" b` is titled `a b`.
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
    /// that do not nest, or nested in itself through such lists, is
    /// [`Failure::Unsupported`], as is an index outside the chart's subgraphs or nodes. Each
    /// subgraph whose id was last given `view: collapsed` is marked
    /// [`Subgraph::collapsed`].
    pub(super) fn resolve_subgraph_ids(&mut self) -> Result<(), Failure> {
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
    /// Classes are looked up only after every statement is read, so that a `classDef`
    /// may follow the `class` statement using it, as upstream compiles them when the
    /// diagram is drawn.
    pub(super) fn apply_styles(&mut self) {
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

/// The property `list` of a `style` or `classDef` statement without the `;` ending it,
/// when a `:`, then non-blanks, then a `#` come before that `;`. A `;` right after a hex
/// color ends no statement (see [`split_statements`](super::statements::split_statements)), so it stays in the list; upstream's
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
