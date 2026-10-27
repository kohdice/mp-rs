//! Building the chart from its statements: nodes, links, subgraphs and styles.

use std::collections::{HashMap, HashSet};

use crate::diagram::Failure;
use crate::diagram::flowchart::chart::{
    Body, Direction, Edge, End, Flowchart, Node, Shape, Subgraph, compact_index,
};
use crate::diagram::flowchart::label::{Label, leading_string, unquoted};
use crate::diagram::flowchart::styling::Styling;

use super::link::{Link, edge_id, link};
use super::shape_data::{edge_data_end, shape_data};
use super::statements::keyword_statement;
use super::tokens::{
    BRACKETS, LABEL_TOKENS, bracket_label, check_label, check_link_text, check_trap_label,
    check_unbracketed_title, id_keyword, leading_keyword, split_id,
};
use super::{DIRECTION_WORDS, direction_statement, syntax_error};

/// Mermaid's default `maxEdges`, a top-level configuration key like `maxTextSize`.
const MAX_EDGES: usize = 500;

/// The link targets upstream's `LINK_TARGET` token reads.
const LINK_TARGETS: [&str; 4] = ["_self", "_blank", "_parent", "_top"];

/// The classes upstream's `getData` compiles for every node before the node's own,
/// so that `classDef default` and `classDef node` style every node, its own classes
/// overriding them.
const NODE_CLASSES: [&str; 2] = ["default", "node"];

pub(super) struct Builder<'a> {
    chart: Flowchart,
    index_of: HashMap<&'a str, usize>,
    /// Every `subgraph … end` block, in the order the blocks open.
    blocks: Vec<Block<'a>>,
    /// Indices into `blocks` of the blocks whose `end` has not been read yet, innermost
    /// last.
    open: Vec<usize>,
    /// The ids a registered subgraph has already taken into its list, which no later
    /// block may list again.
    claimed: HashSet<SubgraphId<'a>>,
    /// The ids given to links (`A e1@--> B`), which an `e1@{ … }` statement refers to.
    edge_ids: HashSet<&'a str>,
    /// The ids of closed subgraphs whose last `view` in `@{ … }` data, given after their
    /// `end`, is `collapsed`.
    collapsed: HashSet<&'a str>,
    /// The id of each subgraph whose `end` has been read, which upstream's `addSubGraph`
    /// registers only then, and the index into `blocks` of the first block that ended
    /// with the id.
    closed_subgraphs: HashMap<SubgraphId<'a>, usize>,
    styles: Styles<'a>,
    /// The id of each link, indexed like [`Flowchart::edges`].
    edge_names: Vec<Option<&'a str>>,
}

/// What the `style`, `classDef`, `class` and `linkStyle` statements set, which
/// [`with_styles`] gives the nodes, links and subgraphs once every statement is read.
#[derive(Default)]
struct Styles<'a> {
    /// What the `style` statements naming each id set, the later ones over the earlier.
    of_ids: HashMap<&'a str, Styling>,
    /// What the `classDef` statements naming each class set, the later ones over the
    /// earlier.
    class_defs: HashMap<&'a str, Styling>,
    /// The classes attached to each id, in the order they were attached.
    classes: HashMap<&'a str, Vec<&'a str>>,
    /// What the last `linkStyle` statement naming each link's index sets.
    of_links: HashMap<usize, Styling>,
    /// What the last `linkStyle default` statement sets.
    default_link: Styling,
}

/// A `subgraph … end` block.
struct Block<'a> {
    /// The line its `subgraph` statement is on, for error reports.
    line: usize,
    id: SubgraphId<'a>,
    title: Label,
    /// The direction of its last `direction` statement, if any.
    direction: Option<Direction>,
    /// Whether that statement spells `TD` (see [`Subgraph::spelled_td`]).
    spelled_td: bool,
    /// The ids it lists, in order: those of the nodes its own statements reference and
    /// those of the subgraphs whose blocks end inside it, as upstream's grammar hands a
    /// block's `document` to `addSubGraph`. Once the block is registered, only the ids
    /// left to it, and those a later block with the same id adds.
    listed: Vec<SubgraphId<'a>>,
}

/// An id a block lists or a subgraph has: a name, which a node or a subgraph may have,
/// or, for a block whose id upstream generates, the block's index into
/// [`Builder::blocks`], which nothing in the source can refer to.
#[derive(Clone, Copy, PartialEq, Eq, Hash)]
enum SubgraphId<'a> {
    Named(&'a str),
    Anonymous(usize),
}

impl<'a> SubgraphId<'a> {
    /// The name, unless the id is anonymous.
    fn name(self) -> Option<&'a str> {
        match self {
            Self::Named(name) => Some(name),
            Self::Anonymous(_) => None,
        }
    }
}

impl<'a> Builder<'a> {
    /// A builder for a chart running in `direction`, spelled `BR` when `spelled_br` (see
    /// [`Flowchart::spelled_br`]), that has read no statement yet.
    pub(super) fn new((direction, spelled_br): (Direction, bool)) -> Self {
        Self {
            chart: Flowchart {
                direction,
                spelled_br,
                nodes: Vec::new(),
                edges: Vec::new(),
                subgraphs: Vec::new(),
            },
            index_of: HashMap::new(),
            blocks: Vec::new(),
            open: Vec::new(),
            claimed: HashSet::new(),
            edge_ids: HashSet::new(),
            collapsed: HashSet::new(),
            closed_subgraphs: HashMap::new(),
            styles: Styles::default(),
            edge_names: Vec::new(),
        }
    }

    /// The chart the statements read so far make, or a syntax error when a subgraph is
    /// still open. Each node, link and subgraph gets its styles, references to subgraph
    /// ids become references to the subgraphs, and every empty subgraph gets one hidden
    /// member.
    pub(super) fn finish(self) -> Result<Flowchart, Failure> {
        if let Some(open) = self.open.last().and_then(|&open| self.blocks.get(open)) {
            return Err(syntax_error(open.line, "subgraph is not closed with \"end\""));
        }
        let registered = registered_blocks(&self.blocks, &self.closed_subgraphs);
        let ids: Vec<SubgraphId<'a>> = registered.iter().map(|block| block.id).collect();
        let lists: Vec<&[SubgraphId<'a>]> =
            registered.iter().map(|block| block.listed.as_slice()).collect();
        let chart = Flowchart {
            subgraphs: registered.iter().map(|block| block.subgraph()).collect(),
            ..self.chart
        };
        let chart = with_styles(chart, &self.styles, &self.index_of, &self.edge_names, &ids);
        let chart = resolve_subgraph_ids(chart, &self.index_of, &ids, &lists, &self.collapsed)?;
        Ok(with_hidden_members(chart))
    }

    /// Applies one statement, whose first word picks its form: `subgraph`, `end`, `style`,
    /// `classDef`, `linkStyle`, `class`, `click` (checked, then dropped), `direction`, link data
    /// (`e1@{ … }`), or else a group of nodes optionally followed by links to further
    /// groups, where a link joins every node of the group before it to every node of the
    /// group after it. `line` is the statement's 1-based line number in the source, for
    /// error reports, `direction_words` the [`super::direction_words`] of its line, counted
    /// from the statement's start, and `followed_by` the character after the statement in
    /// the source, if any. Returns whether the statement is a direction statement, which
    /// takes the rest of its line, so that any statement starting before that line's end
    /// is part of it.
    pub(super) fn statement(
        &mut self,
        line: usize,
        statement: &'a str,
        direction_words: [Option<usize>; DIRECTION_WORDS.len()],
        followed_by: Option<char>,
    ) -> Result<bool, Failure> {
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
        check_direction_after_keyword(line, statement, direction_words, 0)?;
        // Upstream's `"end"\b\s*` token takes the blanks after it, and its subgraph rule
        // needs no separator after `end`, so a statement may follow on the same line.
        let (whole, mut statement) = (statement.len(), statement);
        while leading_keyword(statement) == Some("end") {
            let closed = self
                .open
                .pop()
                .ok_or_else(|| syntax_error(line, "\"end\" without an open subgraph"))?;
            self.register(closed);
            statement = statement.get("end".len()..).unwrap_or_default().trim_start();
            if statement.is_empty() {
                return Ok(false);
            }
            check_direction_after_keyword(
                line,
                statement,
                direction_words,
                whole - statement.len(),
            )?;
        }
        if let Some(rest) = statement.strip_prefix("subgraph")
            && (rest.is_empty() || rest.starts_with(|c: char| c.is_ascii_whitespace()))
        {
            return self.open_subgraph(line, rest).map(|()| false);
        }
        // Upstream's grammar needs every part of a styling statement, so a statement
        // missing one is a syntax error.
        if let Some((id, list)) = keyword_statement(statement, "style") {
            if id.is_empty() || split_id(id).0 != id {
                return Err(syntax_error(line, "style has no node id"));
            }
            check_id(line, id)?;
            if list.is_empty() {
                return Err(syntax_error(line, "style has no property list"));
            }
            // Upstream's `styleStatement` passes the whole id, commas included, to
            // `addVertex`, which declares a node; a node whose id names a subgraph is
            // turned into that subgraph by `resolve_subgraph_ids`, as upstream's
            // `getData` merges the two.
            let styling = Styling::parse(property_list(line, without_color_semicolon(list))?);
            self.declare(id);
            let styles = self.styles.of_ids.entry(id).or_default();
            *styles = styles.then(styling);
            return Ok(false);
        }
        if let Some((names, list)) = keyword_statement(statement, "classDef") {
            if names.is_empty() {
                return Err(syntax_error(line, "classDef has no class name"));
            }
            check_class_name(line, names)?;
            if list.is_empty() {
                return Err(syntax_error(line, "classDef has no property list"));
            }
            let styling = Styling::parse(property_list(line, without_color_semicolon(list))?);
            for name in names.split(',') {
                let class = self.styles.class_defs.entry(name).or_default();
                *class = class.then(styling);
            }
            return Ok(false);
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
                Some((_, "")) => return Ok(false),
                Some((_, after_curve)) => after_curve,
                None => list,
            };
            if list.is_empty() {
                return Err(syntax_error(line, "linkStyle has no property list"));
            }
            // Each statement replaces the style it names, as upstream's `updateLink`
            // assigns the list rather than appending to it.
            let styling = Styling::parse(property_list(line, list)?);
            let Some(indexes) = indexes else {
                self.styles.default_link = styling;
                return Ok(false);
            };
            for index in indexes {
                self.styles.of_links.insert(index, styling);
            }
            return Ok(false);
        }
        // Upstream's `setClass` splits the ids at commas but not the class name.
        if let Some((ids, class)) = keyword_statement(statement, "class") {
            if ids.is_empty()
                || split_id(ids).0 != ids
                || class.is_empty()
                || class.contains(char::is_whitespace)
            {
                return Err(syntax_error(line, "class needs one node id list and one class name"));
            }
            check_id(line, ids)?;
            check_class_name(line, class)?;
            for id in ids.split(',') {
                self.attach_class(id, class);
            }
            return Ok(false);
        }
        // A `click` statement binds an interaction, which a text drawing cannot show, so
        // it is checked and then dropped.
        if let Some(click) = statement.strip_prefix("click")
            && (click.is_empty() || click.starts_with(|c: char| c.is_ascii_whitespace()))
        {
            if !is_click(click) {
                return Err(syntax_error(line, "click needs a callback, a link or a string"));
            }
            return Ok(false);
        }
        // A `direction` statement sets the direction a subgraph's members are laid out
        // in, the last such statement winning; Mermaid reads it only there and ignores it
        // at the top level. A statement led by a keyword never gets here with a direction
        // word on its line: upstream's keyword rules come before its direction rules, and
        // after the keyword the rest of the line lexes as a direction token, which the
        // grammar does not accept there (see `check_direction_after_keyword`).
        if let Some((word, direction)) =
            direction_statement(direction_words, whole - statement.len())
        {
            if let Some(open) = self.open.last().and_then(|&open| self.blocks.get_mut(open)) {
                open.direction = Some(direction);
                open.spelled_td = word == "TD";
            }
            return Ok(true);
        }
        // Data for a link declared earlier with an id: its keys (`animate`, `animation`,
        // `curve`) have no text drawing, and upstream ignores the rest. Upstream animates
        // the edge or draws it along the named curve; a terminal drawing is static and
        // its lines run along rows and columns, so the keys are read and dropped.
        let (id, after_id) = split_id(statement);
        if self.edge_ids.contains(id)
            && let Some(data) = after_id.strip_prefix("@{")
        {
            let end = edge_data_end(line, data)?;
            if data.get(end + 1..).is_some_and(|after| after.trim().is_empty()) {
                return Ok(false);
            }
        }
        let (mut sources, mut rest) = self.group(line, statement, followed_by)?;
        while !rest.is_empty() {
            // An id is an edge id only when a link starts after its `@` and any blanks, so
            // that an `@` inside a link's label stays label text.
            let with_id = edge_id(rest)
                .and_then(|(id, after_id)| Some((Some(id), link(after_id)?)))
                .or_else(|| Some((None, link(rest)?)));
            let Some((id, (Link { stroke, tail, head, label, length, mismatched }, after_link))) =
                with_id
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
            if mismatched {
                return Err(syntax_error(line, "link start and end markers do not match"));
            }
            // An id that an earlier link already took names no link here, as upstream's
            // `addSingleLink` gives a repeated id a generated one instead.
            let mut id = id.filter(|&id| self.edge_ids.insert(id));
            let (label, after_link) = match after_link.trim_start().strip_prefix('|') {
                // Mermaid's grammar accepts a `|text|` label after `~~~`, but a link
                // that is not drawn has no line to carry the text, so the block falls
                // back rather than dropping the text.
                Some(_) if !stroke.is_visible() => return Err(Failure::Unsupported),
                // Upstream's `START_LINK edgeText LINK` takes no `arrowText` after it.
                Some(_) if label.is_some() => {
                    return Err(syntax_error(line, "edge label after link text"));
                }
                Some(after_pipe) => {
                    let (label, after_label) = bracket_label(after_pipe, "|")
                        .ok_or_else(|| syntax_error(line, "unclosed edge label"))?;
                    // Upstream's `arrowText` needs a `text` token between the pipes, and an
                    // empty string yields none, but a blank does (`SPACE`).
                    if matches!(label, "" | "\"\"") {
                        return Err(syntax_error(line, "empty edge label"));
                    }
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
            let (targets, after_targets) = self.group(line, after_link, followed_by)?;
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
        Ok(false)
    }

    /// Starts the subgraph declared by `text`, the part of a `subgraph` statement after
    /// the keyword: an id, which may hold blanks and start with a quoted string, whose
    /// quotes are dropped when it is nothing else, followed by a bracketed title, or
    /// else the rest of the statement as the id, without the quotes around it, and as the
    /// title. Either title is read by [`Label::parse_after_string`], as it may go on in
    /// plain text after a leading string.
    fn open_subgraph(&mut self, line: usize, text: &'a str) -> Result<(), Failure> {
        let text = text.trim();
        // Upstream's grammar accepts a bare `subgraph`, but its `addSubGraph` then reads
        // the text of an id that is not there, so the diagram fails to render.
        if text.is_empty() {
            return Err(syntax_error(line, "subgraph has no id or title"));
        }
        // Upstream reads the id before a bracketed title as `textNoTags`, the same text
        // as an unbracketed title, blanks included; `addSubGraph` keeps such an id named,
        // as it drops only an id that equals the title.
        let after_string = leading_string(text).map_or(text, |(_, rest)| rest);
        let (id, title) = match after_string.find('[') {
            Some(open) => {
                let (id, after_id) = text.split_at(text.len() - after_string.len() + open);
                let id = id.trim_end();
                // `subgraph SPACE textNoTags SQS text SQE` needs a token in both texts, and
                // upstream's string lexer makes no `STR` token of `""`.
                if id.is_empty() {
                    return Err(syntax_error(line, "subgraph has no id or title"));
                }
                check_unbracketed_title(line, id)?;
                let (title, after_title) =
                    bracket_label(after_id.get(1..).unwrap_or_default(), "]")
                        .ok_or_else(|| syntax_error(line, "unclosed subgraph title"))?;
                if title.is_empty() || title == "\"\"" {
                    return Err(syntax_error(line, "empty subgraph title"));
                }
                check_label(line, title, &LABEL_TOKENS)?;
                // The grammar's `separator` follows the `SQE`.
                if !after_title.trim().is_empty() {
                    return Err(syntax_error(
                        line,
                        "expected a new line or \";\" after the subgraph title",
                    ));
                }
                (SubgraphId::Named(unquoted(id)), Label::parse_after_string(title))
            }
            // Upstream reads an unbracketed title as `textNoTags`, not as a bracket label:
            // see `check_unbracketed_title` for what it rejects. A leading string and the
            // text after it are joined, so `subgraph "a" b` is titled `a b`. When the
            // title holds a blank, upstream's `addSubGraph` drops the id and generates
            // `subGraph` plus a counter, which no statement can name: such a block never
            // merges with another, and nothing links to, styles or nests it by id.
            None => {
                check_unbracketed_title(line, text)?;
                let id = unquoted(text);
                let id = if id.contains(char::is_whitespace) {
                    SubgraphId::Anonymous(self.blocks.len())
                } else {
                    SubgraphId::Named(id)
                };
                (id, Label::parse_after_string(text))
            }
        };
        self.open.push(self.blocks.len());
        self.blocks.push(Block {
            line,
            id,
            title: title.joined(),
            direction: None,
            spelled_td: false,
            listed: Vec::new(),
        });
        Ok(())
    }

    /// Registers the subgraph of block `closed`, whose `end` has just been read, as
    /// upstream's `addSubGraph` does: the ids a registered subgraph already lists, and
    /// the block's own id, are dropped from the block's list, so that the first block
    /// to end listing an id keeps it; and a block whose id is already registered adds
    /// what is left of its list to that subgraph, which keeps its title, direction and
    /// place. The id then joins the list of the block enclosing this one.
    fn register(&mut self, closed: usize) {
        let Some(block) = self.blocks.get_mut(closed) else { return };
        let id = block.id;
        let listed = std::mem::take(&mut block.listed);
        let kept: Vec<SubgraphId<'a>> = listed
            .into_iter()
            .filter(|&listed| listed != id && self.claimed.insert(listed))
            .collect();
        let first = *self.closed_subgraphs.entry(id).or_insert(closed);
        if let Some(first) = self.blocks.get_mut(first) {
            first.listed.extend(kept);
        }
        if let Some(open) = self.open.last().and_then(|&open| self.blocks.get_mut(open)) {
            open.listed.push(id);
        }
    }

    /// Reads node references joined by `&` at the start of `text`, and returns their
    /// indices with the text after them. Upstream's `node spaceList AMP spaceList
    /// styledVertex` needs a blank on both sides of each `&`. `followed_by` is as for
    /// [`node`](Self::node).
    fn group(
        &mut self,
        line: usize,
        text: &'a str,
        followed_by: Option<char>,
    ) -> Result<(Vec<usize>, &'a str), Failure> {
        let (first, mut rest) = self.node(line, text, followed_by)?;
        let mut nodes = vec![first];
        while let Some(after_ampersand) = rest
            .strip_prefix(char::is_whitespace)
            .and_then(|after_blank| after_blank.trim_start().strip_prefix('&'))
            .filter(|after_ampersand| after_ampersand.starts_with(char::is_whitespace))
        {
            let (node, after_node) = self.node(line, after_ampersand.trim_start(), followed_by)?;
            nodes.push(node);
            rest = after_node;
        }
        let rest = rest.trim_start();
        if rest.starts_with('&') {
            return Err(syntax_error(line, "& needs a blank on both sides"));
        }
        Ok((nodes, rest))
    }

    /// The index of the node `id` names, declared as a box showing the id when new.
    fn declare(&mut self, id: &'a str) -> usize {
        *self.index_of.entry(id).or_insert_with(|| {
            self.chart.nodes.push(Node {
                body: Body::Box { label: Label::plain(id), shape: Shape::Rectangle },
                styling: Styling::default(),
                array_index: array_index(id),
            });
            self.chart.nodes.len() - 1
        })
    }

    /// Attaches `class` to `id` when a node, a link or a subgraph whose `end` has been
    /// read has the id, as upstream's `setClass` does: an id declared only later gets
    /// nothing.
    fn attach_class(&mut self, id: &'a str, class: &'a str) {
        let known = self.index_of.contains_key(id)
            || self.edge_ids.contains(id)
            || self.closed_subgraphs.contains_key(&SubgraphId::Named(id));
        if known {
            self.styles.classes.entry(id).or_default().push(class);
        }
    }

    /// Reads one node reference at the start of `text`, declaring the node on first
    /// use, and returns its index with the text after it. `followed_by` is the character
    /// in the source after the statement `text` ends, if any, which tells whether
    /// whitespace follows an id ending the statement.
    fn node(
        &mut self,
        line: usize,
        text: &'a str,
        followed_by: Option<char>,
    ) -> Result<(usize, &'a str), Failure> {
        let (id, rest) = split_id(text);
        if id.is_empty() {
            return Err(syntax_error(line, "expected a node id"));
        }
        check_id(line, id)?;
        // `"click"[\s]+`, `"href"[\s]` and `"call"[\s]+` need whitespace after the word, a
        // line break included, so each is a node id only before anything else.
        if matches!(id, "click" | "href" | "call")
            && rest.chars().next().or(followed_by).is_some_and(char::is_whitespace)
        {
            return Err(syntax_error(line, format!("\"{id}\" cannot be a node id")));
        }
        let index = self.declare(id);
        if let Some(open) = self.open.last().and_then(|&open| self.blocks.get_mut(open)) {
            open.listed.push(SubgraphId::Named(id));
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
                // Upstream's `vertex` rules need a `text` of one token at least, and its
                // string lexer makes no `STR` token of `""`.
                if label.is_empty() || label == "\"\"" {
                    return Err(syntax_error(line, "empty node label"));
                }
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
                let closed_subgraph = self.closed_subgraphs.contains_key(&SubgraphId::Named(id));
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
        // Untrimmed: `group` needs the blank before an `&` to tell `A & B` from `A &B`.
        Ok((index, rest))
    }
}

impl Block<'_> {
    /// The subgraph the block declares, with neither members nor parent yet, which
    /// [`resolve_subgraph_ids`] gives it, and without styles, which [`with_styles`] gives
    /// it.
    fn subgraph(&self) -> Subgraph {
        Subgraph {
            title: self.title.clone(),
            members: Vec::new(),
            parent: None,
            direction: self.direction,
            spelled_td: self.spelled_td,
            collapsed: false,
            styling: Styling::default(),
        }
    }
}

impl Styles<'_> {
    /// What the classes `base` and then the classes attached to `id` set, in order, and
    /// then the `style` statements naming it.
    fn styling_of(&self, id: &str, base: &[&str]) -> Styling {
        self.class_styling(id, base).then(self.of_ids.get(id).copied().unwrap_or_default())
    }

    /// What the classes `base` and then the classes attached to `id` set, in order; a
    /// class no `classDef` defines sets nothing.
    fn class_styling(&self, id: &str, base: &[&str]) -> Styling {
        let classes = base.iter().chain(self.classes.get(id).into_iter().flatten());
        classes
            .filter_map(|class| self.class_defs.get(class))
            .fold(Styling::default(), |styling, &later| styling.then(later))
    }
}

/// The first block to end with each registered id (see `closed_subgraphs`), in the
/// order the blocks open: one for each subgraph, indexed like [`Flowchart::subgraphs`].
fn registered_blocks<'b, 'a>(
    blocks: &'b [Block<'a>],
    closed_subgraphs: &HashMap<SubgraphId<'a>, usize>,
) -> Vec<&'b Block<'a>> {
    let mut first_blocks: Vec<usize> = closed_subgraphs.values().copied().collect();
    first_blocks.sort_unstable();
    first_blocks.iter().filter_map(|&block| blocks.get(block)).collect()
}

/// `chart` with each node, link and subgraph given what its classes and its `style` or
/// `linkStyle` statements set (see [`Node::styling`], [`Edge::styling`] and
/// [`Subgraph::styling`]). `index_of` maps each id to the index of the node declared for
/// it, `edge_names` holds the id of each link and `subgraph_ids` that of each subgraph.
/// Classes are looked up only after every statement is read, so that a `classDef` may
/// follow the `class` statement using it, as upstream compiles them when the diagram is
/// drawn.
fn with_styles(
    mut chart: Flowchart,
    styles: &Styles<'_>,
    index_of: &HashMap<&str, usize>,
    edge_names: &[Option<&str>],
    subgraph_ids: &[SubgraphId<'_>],
) -> Flowchart {
    for (id, &index) in index_of {
        if let Some(node) = chart.nodes.get_mut(index) {
            node.styling = styles.styling_of(id, &NODE_CLASSES);
        }
    }
    // Upstream applies a link's classes as compiled styles and its `linkStyle` lists,
    // the default one first, as inline styles over them.
    for (index, edge) in chart.edges.iter_mut().enumerate() {
        let name = edge_names.get(index).copied().flatten();
        let classes = name.map(|id| styles.class_styling(id, &[])).unwrap_or_default();
        let own = styles.of_links.get(&index).copied().unwrap_or_default();
        edge.styling = classes.then(styles.default_link).then(own);
    }
    // Upstream compiles no `default` class for a subgraph, only its own classes.
    for (subgraph, id) in chart.subgraphs.iter_mut().zip(subgraph_ids) {
        subgraph.styling = id.name().map(|name| styles.styling_of(name, &[])).unwrap_or_default();
    }
    chart
}

/// `chart` with each subgraph given the parent and members its list in `lists` names,
/// indexed like [`Flowchart::subgraphs`]: a subgraph whose id in `subgraph_ids` is listed
/// is nested in the subgraph listing it, and every other listed id is a node's, which
/// `index_of` maps to the node's index. Then every reference to a subgraph's name,
/// declared before or after it, is turned into a reference to the subgraph, as Mermaid
/// reads it: a link end becomes [`End::Subgraph`], and the node declared for the name is
/// dropped. Subgraphs nested in themselves through such lists are
/// [`Failure::Unsupported`], as is an index outside the chart's subgraphs or nodes. Each
/// subgraph whose name is in `collapsed` is marked [`Subgraph::collapsed`].
fn resolve_subgraph_ids(
    mut chart: Flowchart,
    index_of: &HashMap<&str, usize>,
    subgraph_ids: &[SubgraphId<'_>],
    lists: &[&[SubgraphId<'_>]],
    collapsed: &HashSet<&str>,
) -> Result<Flowchart, Failure> {
    for (subgraph, id) in chart.subgraphs.iter_mut().zip(subgraph_ids) {
        subgraph.collapsed = id.name().is_some_and(|name| collapsed.contains(name));
    }
    let subgraph_of: HashMap<SubgraphId<'_>, usize> =
        subgraph_ids.iter().enumerate().map(|(subgraph, &id)| (id, subgraph)).collect();
    let mut named = vec![None; chart.nodes.len()];
    for (&id, &subgraph) in &subgraph_of {
        if let Some(&node) = id.name().and_then(|name| index_of.get(name))
            && let Some(slot) = named.get_mut(node)
        {
            *slot = Some(subgraph);
        }
    }
    for (holder, list) in lists.iter().enumerate() {
        for id in *list {
            if let Some(&listed) = subgraph_of.get(id) {
                let listed = chart.subgraphs.get_mut(listed).ok_or(Failure::Unsupported)?;
                listed.parent = Some(holder);
            }
        }
    }
    if (0..chart.subgraphs.len())
        .any(|subgraph| chart.enclosing(subgraph).any(|outer| outer == subgraph))
    {
        return Err(Failure::Unsupported);
    }
    // A subgraph's members are its listed nodes and the members of the subgraphs it
    // lists, which come first as they are nested deeper. No id is listed twice, so
    // no member repeats.
    for subgraph in chart.innermost_first() {
        let mut members = Vec::new();
        for id in lists.get(subgraph).copied().into_iter().flatten() {
            match subgraph_of.get(id) {
                Some(&inner) => members
                    .extend(chart.subgraphs.get(inner).ok_or(Failure::Unsupported)?.members.iter()),
                None => members.push(
                    *id.name().and_then(|name| index_of.get(name)).ok_or(Failure::Unsupported)?,
                ),
            }
        }
        if let Some(subgraph) = chart.subgraphs.get_mut(subgraph) {
            subgraph.members = members;
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
    for edge in &mut chart.edges {
        edge.from = resolve(edge.from).ok_or(Failure::Unsupported)?;
        edge.to = resolve(edge.to).ok_or(Failure::Unsupported)?;
    }
    for member in chart.subgraphs.iter_mut().flat_map(|subgraph| &mut subgraph.members) {
        *member = renumbered.get(*member).copied().flatten().ok_or(Failure::Unsupported)?;
    }
    let mut names = named.iter();
    chart.nodes.retain(|_| names.next().is_some_and(Option::is_none));
    Ok(chart)
}

/// `chart` with one hidden member added to every empty subgraph: Mermaid draws an empty
/// subgraph as a frame showing only its title, placed in the flow like a node, and the
/// member gives it a place and its frame the least size.
fn with_hidden_members(mut chart: Flowchart) -> Flowchart {
    // Inner subgraphs come first, so that a subgraph holding only an empty one is not
    // empty itself.
    for subgraph in chart.innermost_first() {
        if chart.subgraphs.get(subgraph).is_some_and(|subgraph| subgraph.members.is_empty()) {
            let hidden = chart.nodes.len();
            chart.nodes.push(Node {
                body: Body::Hidden,
                styling: Styling::default(),
                array_index: None,
            });
            let holders: Vec<usize> =
                std::iter::once(subgraph).chain(chart.enclosing(subgraph)).collect();
            for holder in holders {
                if let Some(holder) = chart.subgraphs.get_mut(holder) {
                    holder.members.push(hidden);
                }
            }
        }
    }
    chart
}

/// Checks that `id`, read on `line`, holds no keyword where upstream's lexer starts a
/// token (see [`id_keyword`]): `idStringToken` admits `DEFAULT` and `DOWN` (`v`) but no
/// other keyword.
fn check_id(line: usize, id: &str) -> Result<(), Failure> {
    match id_keyword(id) {
        Some(keyword) => Err(syntax_error(line, format!("\"{keyword}\" cannot be a node id"))),
        None => Ok(()),
    }
}

/// Checks that `name`, a class name or list of them read on `line`, holds no keyword
/// where upstream's lexer starts a token, as [`check_id`] does for node ids.
fn check_class_name(line: usize, name: &str) -> Result<(), Failure> {
    match id_keyword(name) {
        Some(keyword) => Err(syntax_error(line, format!("\"{keyword}\" cannot be a class name"))),
        None => Ok(()),
    }
}

/// Checks that no `direction` word follows a keyword leading `statement` on its line (see
/// [`statement_keyword`]): after the keyword, upstream's lexer reads the rest of the line
/// as a direction token (`.*direction\s+TB[^\n]*` and the like), which its grammar takes
/// after no keyword. `words` holds the line's direction words, counted from `offset`
/// before the statement's start (see [`direction_statement`]).
fn check_direction_after_keyword(
    line: usize,
    statement: &str,
    words: [Option<usize>; DIRECTION_WORDS.len()],
    offset: usize,
) -> Result<(), Failure> {
    match statement_keyword(statement) {
        Some(keyword) if direction_statement(words, offset + keyword.len()).is_some() => Err(
            syntax_error(line, format!("\"direction\" cannot follow \"{keyword}\" on its line")),
        ),
        _ => Ok(()),
    }
}

/// The keyword upstream's lexer reads at the start of `statement` before trying its
/// direction rules, if any: one of [`leading_keyword`]'s but `end`, whose token takes the
/// blanks after it and ends its statement, and `v`, whose rule comes after the direction
/// rules; or `click`, `href` or `call` followed by the whitespace their rules need.
fn statement_keyword(statement: &str) -> Option<&'static str> {
    leading_keyword(statement).filter(|&keyword| !matches!(keyword, "end" | "v")).or_else(|| {
        ["click", "href", "call"].into_iter().find(|keyword| {
            statement
                .strip_prefix(keyword)
                .is_some_and(|after| after.starts_with(char::is_whitespace))
        })
    })
}

/// Whether `text`, a `click` statement after the keyword and the blank ending it, is
/// one of upstream's `clickStatement` forms: a node id, one blank, and then a callback,
/// `call name(args)` or a bare name, or a link, `href "url"` or `"url"`. A tooltip
/// string may follow the callback; a tooltip string, a link target (`_self`, `_blank`,
/// `_parent` or `_top`), or both, in that order, may follow the link.
fn is_click(text: &str) -> bool {
    // `"click"[\s]+` takes every blank before the node id, but `<click>[\s\n]` takes only
    // one after it: a second would be a `SPACE` token, which no form allows there.
    let Some((_, action)) = text.trim_start().split_once(char::is_whitespace) else {
        return false;
    };
    if action.starts_with(char::is_whitespace) {
        return false;
    }
    let rest = if let Some(callback) =
        action.strip_prefix("call").filter(|after| after.starts_with(char::is_whitespace))
    {
        // Upstream's `CALLBACKNAME` lexeme `[^(]*` runs on, across lines, to the next
        // `(`, so a `call` with no argument list never parses there; this reader ends a
        // statement at the line, so it rejects the form instead.
        let Some((name, after_name)) = callback.split_once('(') else { return false };
        match after_name.split_once(')') {
            Some((_, after_args)) if !name.trim().is_empty() => after_blank(after_args, string),
            _ => return false,
        }
    } else if let Some(url) = action
        .strip_prefix("href")
        // `"href"[\s]` takes exactly one blank.
        .and_then(|after| after.strip_prefix(char::is_whitespace))
        .or(action.starts_with('"').then_some(action))
    {
        let Some(after_url) = string(url) else { return false };
        // A target left with text after it fails the check below, as upstream's
        // `LINK_TARGET`, which jison ends at a word boundary, does.
        after_blank(after_blank(after_url, string), |text| {
            LINK_TARGETS.iter().find_map(|target| text.strip_prefix(target))
        })
    } else {
        let name_end = action.find(char::is_whitespace).unwrap_or(action.len());
        after_blank(action.get(name_end..).unwrap_or_default(), string)
    };
    rest.trim().is_empty()
}

/// `id` as a canonical array index (see [`Node::array_index`]): decimal digits with no
/// leading zero but `0` itself, below `u32::MAX`.
fn array_index(id: &str) -> Option<u32> {
    let canonical =
        id.bytes().all(|byte| byte.is_ascii_digit()) && (id == "0" || !id.starts_with('0'));
    id.parse().ok().filter(|&index| canonical && index != u32::MAX)
}

/// `text` after the blanks starting it and what `part` reads after them, or `text` when
/// it starts with no blank or `part` reads nothing there; upstream's `SPACE` is a run of
/// blanks.
fn after_blank<'t>(text: &'t str, part: impl Fn(&'t str) -> Option<&'t str>) -> &'t str {
    text.strip_prefix(char::is_whitespace).map(str::trim_start).and_then(part).unwrap_or(text)
}

/// The text after a double-quoted string starting `text`, unless the string is empty,
/// as upstream's string lexer makes no `STR` token of `""`.
fn string(text: &str) -> Option<&str> {
    let (content, after) = text.strip_prefix('"')?.split_once('"')?;
    (!content.is_empty()).then_some(after)
}

/// The property `list` of a `style` or `classDef` statement without the `;` ending it,
/// when a `:`, then non-blanks, then a `#` come before that `;`. A `;` right after a hex
/// color ends no statement (see [`split_statements`](super::statements::split_statements)), so it stays in the list; upstream's
/// `encodeEntities` (`packages/mermaid/src/utils.ts`) drops it from such statements with
/// `/style.*:\S*#.*;/` and `/classDef.*:\S*#.*;/` before parsing.
fn without_color_semicolon(list: &str) -> &str {
    let Some(before) = list.strip_suffix(';') else { return list };
    let color_after_colon = before
        .chars()
        .scan(false, |after_colon, c| {
            let hash_after_colon = *after_colon && c == '#';
            *after_colon = c == ':' || (*after_colon && !c.is_whitespace());
            Some(hash_after_colon)
        })
        .any(|hash_after_colon| hash_after_colon);
    if color_after_colon { before } else { list }
}

/// The property `list` of a `style`, `classDef` or `linkStyle` statement on `line`, or a
/// syntax error quoting the first character, pair or keyword that no token of upstream's
/// `styleComponent` (`NUM`, `NODE_STRING`, `COLON`, `SPACE`, `BRKT`, `STYLE`) nor the
/// `COMMA` between declarations reads: a character outside those tokens, such as the `(`
/// of `fill:rgb(255,0,0)`, which upstream lexes as a label opener; a `"`, `&` or `*`
/// starting a word, which upstream lexes as a string, `AMP` or `MULT`; a keyword other
/// than `style` starting a word (see [`leading_keyword`]), such as the `end` of
/// `text-align:end`; and `--`, `-.`, `->` or `==` anywhere, where `NODE_STRING`'s
/// lookaheads end the word and a link token follows. A word starts the list or follows a
/// blank, `:` or `,`. A `;` reaches here only where the statement splitter kept it after
/// an entity code.
///
/// Upstream's lexer takes the first rule that matches, so a keyword matches only a whole
/// word while `"&"`, `"*"` and `["]` match at any token start; inside a word
/// `NODE_STRING` has already taken them.
fn property_list(line: usize, list: &str) -> Result<&str, Failure> {
    let held = |c: char| {
        c.is_ascii_alphanumeric()
            || "!\"#$%&'*+,-./:;=?\\_`".contains(c)
            || (c.is_whitespace() && !matches!(c, '\n' | '\r'))
    };
    let mut previous: Option<char> = None;
    let mut chars = list.char_indices().peekable();
    while let Some((at, c)) = chars.next() {
        let next = chars.peek().map(|&(_, next)| next);
        let starts_word = previous.is_none_or(|p| p.is_whitespace() || matches!(p, ':' | ','));
        let rejected = match (c, next) {
            _ if !held(c) => Some(c.to_string()),
            ('-', Some(n @ ('-' | '.' | '>'))) | ('=', Some(n @ '=')) => Some(format!("{c}{n}")),
            ('"' | '&' | '*', _) if starts_word => Some(c.to_string()),
            _ if starts_word => leading_keyword(list.get(at..).unwrap_or_default())
                .filter(|&keyword| keyword != "style")
                .map(str::to_owned),
            _ => None,
        };
        if let Some(rejected) = rejected {
            return Err(syntax_error(line, format!("property list holds \"{rejected}\"")));
        }
        previous = Some(c);
    }
    Ok(list)
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
        return Err(syntax_error(line, message));
    }
    Ok(index)
}
