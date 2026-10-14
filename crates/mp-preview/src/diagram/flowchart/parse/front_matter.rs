//! The frontmatter block a diagram source may start with, and the title it gives.

use std::borrow::Cow;

use crate::diagram::Failure;
use crate::diagram::flowchart::label::shown_text;

use super::shape_data::scalar;
use super::syntax_error;

/// A diagram's source split at its frontmatter.
pub(in crate::diagram::flowchart) struct FrontMatter<'a> {
    /// The `title` the frontmatter gives, which upstream draws above the diagram.
    pub title: Option<String>,
    /// The source after the frontmatter.
    pub body: &'a str,
    /// The 1-based line of `source` that `body` starts on.
    pub first_line: usize,
}

/// Splits off the frontmatter `source` starts with and reads its top-level `title`,
/// mirroring `extractFrontMatter` in `packages/mermaid/src/diagram-api/frontmatter.ts`.
/// The values of other keys are not read: nothing in upstream's `config` has a
/// counterpart in box-drawing text.
///
/// Errors, with js-yaml's message where js-yaml throws:
///
/// - "unclosed front matter" when the block never closes;
/// - `duplicated mapping key` for a top-level key written twice, or a `title` written
///   twice in a block that is one flow mapping;
/// - `end of the stream or a document separator is expected` for a line holding no key
///   before the first key, a sequence entry after a key, or anything after a quoted
///   scalar or flow collection the block starts with;
/// - `can not read a block mapping entry; a multiline key may not be an implicit key`
///   and `can not read an implicit mapping pair; a colon is missed` for another line
///   holding no key after a key;
/// - `tab characters must not be used in indentation` for a line indented with a tab;
/// - `bad indentation of a mapping entry` or `… of a sequence entry` for a line of the
///   title's value, or of a sequence the block starts with, that does not fit where it
///   stands;
/// - `missed comma between flow collection entries` for text left after an item's value
///   in a flow collection;
/// - `unexpected end of the stream within …` for a quoted scalar or flow collection
///   that nothing closes.
///
/// `source` uses `\n` line endings: the Markdown layer (`sanitize_block_body`)
/// normalises CR before any diagram source reaches the parser.
pub(in crate::diagram::flowchart) fn front_matter(
    source: &str,
) -> Result<FrontMatter<'_>, Failure> {
    let none = FrontMatter { title: None, body: source, first_line: 1 };
    let mut lines = source.split_inclusive('\n').zip(1..);
    let Some((first, opening)) = lines.next() else { return Ok(none) };
    let fence = first.trim_start_matches(|c: char| c.is_whitespace() && c != '\n');
    if !is_fence(fence) {
        return Ok(none);
    }
    let indent = first.get(..first.len() - fence.len()).unwrap_or_default();
    let mut offset = first.len();
    let mut block = Vec::new();
    for (line, number) in lines {
        offset += line.len();
        // The opening fence may be indented. Upstream's `frontMatterRegex`
        // (`^([^\S\n\r]*)-{3}` … `\1-{3}`) needs the closing fence to repeat that indent,
        // and the indent is removed from each line between, a line without it being read
        // as written.
        let stripped = line.strip_prefix(indent);
        if stripped.is_some_and(is_fence) {
            let body = source.get(offset..).unwrap_or_default();
            let title = title(&block.join("\n"), opening + 1)?;
            return Ok(FrontMatter { title, body, first_line: number + 1 });
        }
        block.push(stripped.unwrap_or(line).trim_end_matches('\n'));
    }
    Err(syntax_error(opening, "unclosed front matter"))
}

/// Whether `line` is a frontmatter fence: `---` and nothing after it but blanks.
fn is_fence(line: &str) -> bool {
    line.strip_prefix("---").is_some_and(|after| after.trim().is_empty())
}

/// YAML's blanks: spaces and tabs only, as js-yaml reads a no-break space as content.
const BLANKS: [char; 2] = [' ', '\t'];

/// The characters that separate the items of a flow collection: [`BLANKS`] and line
/// breaks, which a flow collection may span.
const FLOW_BLANKS: [char; 3] = [' ', '\t', '\n'];

/// The message js-yaml's `readDocument` throws for text left after the document's root
/// node.
const END_EXPECTED: &str = "end of the stream or a document separator is expected";

/// Reads the title the frontmatter `block` gives. `block` holds the block's lines
/// without its indent, and its first line is line `first_line` of the source.
///
/// The title is the value of the top-level `title` key, read by [`Yaml::node`]. It is
/// written as JavaScript's `String` writes it and drawn on one row (see [`one_row`]).
/// Upstream keeps it only `if (parsed.title)`. `None` when no key is `title` or its value
/// is falsy. A block that is one value and no mapping gives a title only as a flow
/// mapping (see [`Yaml::start`]).
///
/// Its errors, listed at [`front_matter`], come from [`Yaml::start`], from this loop
/// (`duplicated mapping key` and the tab), from [`Yaml::not_a_key`], from
/// [`Yaml::node`] and from [`Yaml::closed_value`].
fn title(block: &str, first_line: usize) -> Result<Option<String>, Failure> {
    let yaml = Yaml::new(block, first_line);
    let (first_key, key, after_key) = match yaml.start()? {
        Start::Mapping { at, key, after_key } => (at, key, after_key),
        Start::Value(title) => return Ok(title),
    };
    let mut keys: Vec<Cow<'_, str>> = Vec::new();
    let mut title = None;
    let mut found = Some((key, after_key));
    let mut at = first_key;
    while let Some(&line) = yaml.lines.get(at) {
        let (key, after_key) = match found.take() {
            Some(found) => found,
            None if !is_top_level(line) => {
                at += 1;
                continue;
            }
            None if line.starts_with('\t') => {
                return Err(yaml.error(at, "tab characters must not be used in indentation"));
            }
            None => top_level_key(line).ok_or_else(|| yaml.not_a_key(at))?,
        };
        if keys.contains(&key) {
            return Err(yaml.error(at, "duplicated mapping key"));
        }
        let mut end = yaml.nested_end(at, 0);
        at = if key == "title" {
            let (node, next) = yaml.node(at, after_key, end, 0, Place::KeyLine)?;
            title = node.title();
            next
        } else {
            // A block sequence may stand at its key's own indentation, as js-yaml's
            // `composeNode` reads one there, when nothing follows the key on its line.
            if is_blank_or_comment(after_key) && is_sequence_entry(yaml.line(end)) {
                end = yaml.compact_sequence_end(end, 0);
            }
            // Another key's value is passed over, but a quoted scalar or flow collection
            // in it runs to its close across lines, so the keys resume after that.
            yaml.passed_over_end(at, after_key, end, 0)?
        };
        keys.push(key);
    }
    Ok(title)
}

/// Whether `line` is a top-level line holding more than a comment: unindented, or
/// indented with a tab, which js-yaml rejects.
fn is_top_level(line: &str) -> bool {
    !is_blank_or_comment(line) && !line.starts_with(' ')
}

/// The key of `line` and the text after its colon, when `line`, an unindented line
/// that is no comment, holds a key of the top-level mapping. A sequence entry (`- `)
/// holds none, while `-a: 1` holds the key `-a`. The colon is the one [`mapping_colon`]
/// finds.
fn top_level_key(line: &str) -> Option<(Cow<'_, str>, &str)> {
    if is_sequence_entry(line) {
        return None;
    }
    let colon = mapping_colon(line)?;
    let written = line.get(..colon)?.trim_end_matches(BLANKS);
    (!written.is_empty()).then(|| (key_text(written), line.get(colon + 1..).unwrap_or_default()))
}

/// The key a mapping entry writes as `written`: a quoted key reads as its text, as
/// js-yaml reads `"title": x` as the key `title`.
fn key_text(written: &str) -> Cow<'_, str> {
    quoted_text(written).unwrap_or(Cow::Borrowed(written))
}

/// How a frontmatter block starts, as [`Yaml::start`] reads it.
enum Start<'a> {
    /// The block is a mapping whose first key is `key`, on line `at`, with `after_key`
    /// after its colon.
    Mapping { at: usize, key: Cow<'a, str>, after_key: &'a str },
    /// The block is one value and no block mapping, which gives this title.
    Value(Option<String>),
}

/// A YAML node of the frontmatter, read as far as a title needs it.
enum Node {
    /// A bare scalar js-yaml reads as null (see [`is_null`]).
    Null,
    /// Any other bare scalar, as written.
    Plain(String),
    /// A quoted or block scalar, always a string.
    Text(String),
    Sequence(Vec<Node>),
    Mapping,
}

impl Node {
    /// What JavaScript's `String` writes for the value js-yaml loads the node as, except
    /// that a bare scalar js-yaml reads as a number or a boolean is written as it stands.
    /// A null item of a sequence is written as nothing, as `Array.prototype.join` writes
    /// it.
    fn js_string(&self) -> String {
        // A known deviation: scalars stay as written, where js-yaml's `JSON_SCHEMA` reads
        // `1.0`, `0x10` and `True` as the number 1, the number 16 and the boolean true,
        // which upstream draws as `1`, `16` and `true`. Only the falsy check reads numbers
        // and booleans.
        match self {
            Self::Null => String::new(),
            Self::Plain(text) | Self::Text(text) => text.clone(),
            Self::Sequence(items) => {
                items.iter().map(Self::js_string).collect::<Vec<_>>().join(",")
            }
            Self::Mapping => "[object Object]".to_owned(),
        }
    }

    /// The title the node gives as a `title` value. `None` when JavaScript reads the
    /// value as falsy, or when it writes as nothing but blanks.
    fn title(&self) -> Option<String> {
        if matches!(self, Self::Null)
            || matches!(self, Self::Plain(text) if is_falsy_plain_scalar(text))
        {
            return None;
        }
        let row = one_row(&self.js_string());
        (!row.is_empty()).then(|| shown_text(&row))
    }
}

/// Where a node stands, which decides what may start there and the message js-yaml
/// throws for a line that does not fit.
#[derive(Clone, Copy, PartialEq, Eq)]
enum Place {
    /// On the line of a mapping key, after its colon, where no block collection starts.
    KeyLine,
    /// On a line of its own under a mapping key.
    UnderKey,
    /// After a sequence entry's `-`, where a block collection may start on the same line.
    Entry,
}

impl Place {
    /// The message js-yaml throws for a line that does not fit after a node standing
    /// here: `readBlockMapping` and `readBlockSequence` in `lib/loader.js` throw it when
    /// text is left on the node's last line or a deeper line follows.
    fn bad_indentation(self) -> &'static str {
        match self {
            Self::KeyLine | Self::UnderKey => "bad indentation of a mapping entry",
            Self::Entry => "bad indentation of a sequence entry",
        }
    }
}

/// The message js-yaml's `readFlowCollection` throws at the stream's end for a flow
/// collection that nothing closes.
const UNCLOSED_COLLECTION: &str = "unexpected end of the stream within a flow collection";

/// The message js-yaml's `readDoubleQuotedScalar` or `readSingleQuotedScalar` throws at
/// the stream's end for a scalar opened by `quote` that nothing closes. A quote inside
/// a flow collection reports the quote, as its reader throws first.
fn unclosed_quote(quote: char) -> &'static str {
    if quote == '"' {
        "unexpected end of the stream within a double quoted scalar"
    } else {
        "unexpected end of the stream within a single quoted scalar"
    }
}

/// The lines of a frontmatter block, as [`title`] reads them.
struct Yaml<'a> {
    block: &'a str,
    lines: Vec<&'a str>,
    /// The byte index in `block` where each line starts.
    starts: Vec<usize>,
    /// The line of the source that the block's first line is.
    first_line: usize,
}

/// A quoted scalar or flow collection read up to its close.
struct Closed<'a> {
    value: Node,
    /// The rest of the line it closes on.
    rest: &'a str,
    /// The index of the line it closes on.
    line: usize,
}

impl<'a> Yaml<'a> {
    fn new(block: &'a str, first_line: usize) -> Self {
        let lines: Vec<&str> = block.split('\n').collect();
        let starts = lines
            .iter()
            .scan(0, |start, line| {
                let this = *start;
                *start += line.len() + 1;
                Some(this)
            })
            .collect();
        Self { block, lines, starts, first_line }
    }

    fn line(&self, index: usize) -> &'a str {
        self.lines.get(index).copied().unwrap_or_default()
    }

    /// The index of the line holding byte index `offset` of the block.
    fn line_of(&self, offset: usize) -> usize {
        self.starts.partition_point(|&start| start <= offset).saturating_sub(1)
    }

    /// The syntax error `message` on the line of index `index`, which may be one past the
    /// last line: js-yaml adds a line break to a block that does not end with one, so a
    /// reader that runs to the end stops on the line after the last.
    fn error(&self, index: usize, message: &str) -> Failure {
        syntax_error(self.first_line + index, message)
    }

    /// The syntax error `message` on the line after the last, where a reader that runs to
    /// the end of the block stops (see [`Yaml::error`]).
    fn error_at_end(&self, message: &str) -> Failure {
        self.error(self.lines.len(), message)
    }

    /// The syntax error for `error`, from a flow collection whose text starts at byte
    /// `start` of the block: an unclosed one on the line after the last, where js-yaml
    /// stops, and a missed comma on the line of the text left in its item.
    fn flow_error(&self, start: usize, error: FlowError) -> Failure {
        match error {
            FlowError::Unclosed(message) => self.error_at_end(message),
            FlowError::MissedComma { breaks } => {
                self.error(self.line_of(start) + breaks, MISSED_COMMA)
            }
        }
    }

    /// The index of the first line from `from` that holds more than blanks or a comment,
    /// is indented no deeper than `indent`, and does not `continue` the block before it.
    fn block_end(&self, from: usize, indent: usize, continues: impl Fn(&str) -> bool) -> usize {
        (from..self.lines.len())
            .find(|&index| {
                let line = self.line(index);
                !is_blank_or_comment(line) && indent_of(line) <= indent && !continues(line)
            })
            .unwrap_or(self.lines.len())
    }

    /// The index of the first line after line `at` indented no deeper than `indent` (see
    /// [`Yaml::block_end`]). The lines before it are nested under what line `at` holds.
    fn nested_end(&self, at: usize, indent: usize) -> usize {
        self.block_end(at + 1, indent, |_| false)
    }

    /// The index of the first line from `from` that ends a compact block sequence, one at
    /// its key's own indentation `column`: the first line indented no deeper than
    /// `column` that is not an entry (see [`Yaml::block_end`]).
    fn compact_sequence_end(&self, from: usize, column: usize) -> usize {
        self.block_end(from, column, |line| is_sequence_entry(line.trim_start_matches(' ')))
    }

    /// Reads how the block starts: its first top-level key, or else the one value it is.
    ///
    /// A block opening with a sequence entry is a block sequence, read by
    /// [`Yaml::sequence`] up to the first unindented line that is no entry. A block
    /// opening with a quoted scalar or flow collection is that value, read up to its close
    /// (see [`Yaml::closed_value`]). Any other line opens a bare scalar, which js-yaml's
    /// `readPlainScalar` reads on over the lines after it whatever their indentation, up
    /// to a `:` before a blank or the end, or a comment. Upstream keeps nothing from a
    /// scalar or a sequence, and the `title` of a flow mapping (see
    /// [`Yaml::flow_mapping_title`]).
    ///
    /// Syntax errors: a line indented with a tab, as `readBlockMapping` throws `tab
    /// characters must not be used in indentation`; what [`Yaml::sequence`] rejects in a
    /// sequence entry, such as `bad indentation of a sequence entry` (no line after its
    /// last entry is left for the message under a key, as the sequence ends at the first
    /// unindented line that is no entry); and anything after the value the block opens
    /// with, where `readDocument` throws `end of the stream or a document separator is
    /// expected`: on the line a quoted scalar or flow collection closes on, on the line
    /// whose `:` stops a bare scalar, or on a later line holding more than blanks or a
    /// comment.
    fn start(&self) -> Result<Start<'a>, Failure> {
        let Some(at) = (0..self.lines.len()).find(|&index| is_top_level(self.line(index))) else {
            return Ok(Start::Value(None));
        };
        let line = self.line(at);
        if line.starts_with('\t') {
            return Err(self.error(at, "tab characters must not be used in indentation"));
        }
        if let Some((key, after_key)) = top_level_key(line) {
            return Ok(Start::Mapping { at, key, after_key });
        }
        if is_sequence_entry(line) {
            let end = self.compact_sequence_end(at, 0);
            let (_, next) = self.sequence(at, 0, end, Place::UnderKey, 0)?;
            self.end_expected_from(next)?;
            return Ok(Start::Value(None));
        }
        if let Some(closed) = self.closed_value(at, line)? {
            if !uncommented(trim_blanks(closed.rest)).0.is_empty() {
                return Err(self.error(closed.line, END_EXPECTED));
            }
            self.end_expected_from(closed.line + 1)?;
            let title = if line.starts_with('{') { self.flow_mapping_title(at)? } else { None };
            return Ok(Start::Value(title));
        }
        let stop = if uncommented(line).1 {
            at
        } else {
            let stop = (at + 1..self.lines.len()).find(|&index| {
                let (text, commented) = uncommented(trim_blanks(self.line(index)));
                commented || plain_colon(text).is_some()
            });
            let Some(stop) = stop else { return Ok(Start::Value(None)) };
            if plain_colon(uncommented(trim_blanks(self.line(stop))).0).is_some() {
                return Err(self.error(stop, END_EXPECTED));
            }
            stop
        };
        self.end_expected_from(stop + 1)?;
        Ok(Start::Value(None))
    }

    /// Errors with `end of the stream or a document separator is expected` at the first
    /// line from `from` that holds more than blanks or a comment, as js-yaml's
    /// `readDocument` throws for text left after the document's root node.
    fn end_expected_from(&self, from: usize) -> Result<(), Failure> {
        match (from..self.lines.len()).find(|&index| !is_blank_or_comment(self.line(index))) {
            Some(index) => Err(self.error(index, END_EXPECTED)),
            None => Ok(()),
        }
    }

    /// The title the flow mapping opening line `at` gives: the value of its `title` key,
    /// read as an item of a flow collection is (see [`flow_value`]). A `title` written
    /// twice is a syntax error, as js-yaml throws `duplicated mapping key`, on the line
    /// its item starts on.
    fn flow_mapping_title(&self, at: usize) -> Result<Option<String>, Failure> {
        let start = self.starts.get(at).copied().unwrap_or_default();
        let from = self.block.get(start..).unwrap_or_default();
        let (items, _) = flow_items(from).map_err(|e| self.flow_error(start, e))?;
        let mut title = None;
        for item in &items {
            let text = item.text.trim_matches(FLOW_BLANKS);
            let (key, value) = match mapping_colon(text) {
                Some(colon) => (
                    key_text(text.get(..colon).unwrap_or_default().trim_end_matches(FLOW_BLANKS)),
                    text.get(colon + 1..).unwrap_or_default(),
                ),
                None => (key_text(text), ""),
            };
            if key != "title" {
                continue;
            }
            let item_start = start + item.at;
            if title.is_some() {
                return Err(self.error(self.line_of(item_start), "duplicated mapping key"));
            }
            let before = text.get(..text.len() - value.trim_start_matches(FLOW_BLANKS).len());
            let node = flow_value(value)
                .map_err(|e| self.flow_error(item_start, e.after(before.unwrap_or_default())))?;
            title = Some(node.title());
        }
        Ok(title.flatten())
    }

    /// The syntax error for line `at`, an unindented line after a top-level key that
    /// holds neither a key nor a comment, with the message and line js-yaml throws:
    ///
    /// - A sequence entry ends the top-level mapping, and `readDocument` throws `end of
    ///   the stream or a document separator is expected` on its line.
    /// - A quoted scalar or flow collection is read up to its close. One that nothing
    ///   closes throws as [`Yaml::closed_value`] says. One that closes on a later line is
    ///   a key spanning lines, for which `readBlockMapping` throws `can not read a block
    ///   mapping entry; a multiline key may not be an implicit key` on that line. One that
    ///   closes on its own line is a key that no colon follows, for which it throws `can
    ///   not read an implicit mapping pair; a colon is missed`.
    /// - A bare scalar ending in a comment is also a key that no colon follows.
    /// - Any other bare scalar is read on as a key, as top-level keys are not indented:
    ///   past blank lines and over any other line, up to a line that holds a comment or a
    ///   mapping entry or starts a document marker (`---`, `...`), or else to the line
    ///   after the last. The multiline key error is thrown there.
    fn not_a_key(&self, at: usize) -> Failure {
        const MULTILINE_KEY: &str =
            "can not read a block mapping entry; a multiline key may not be an implicit key";
        const NO_COLON: &str = "can not read an implicit mapping pair; a colon is missed";
        let line = self.line(at);
        if is_sequence_entry(line) {
            return self.error(at, END_EXPECTED);
        }
        match self.closed_value(at, line) {
            Err(unclosed) => return unclosed,
            Ok(Some(closed)) if closed.line == at => return self.error(at, NO_COLON),
            Ok(Some(closed)) => return self.error(closed.line, MULTILINE_KEY),
            Ok(None) => {}
        }
        if uncommented(line).1 {
            return self.error(at, NO_COLON);
        }
        let stop = (at + 1..self.lines.len())
            .find(|&index| {
                let line = self.line(index);
                let (text, commented) = uncommented(trim_blanks(line));
                commented
                    || plain_colon(text).is_some()
                    || line.starts_with("---")
                    || line.starts_with("...")
            })
            .unwrap_or(self.lines.len());
        self.error(stop, MULTILINE_KEY)
    }

    /// Reads the node that starts with `text`, a suffix of line `at`, standing at
    /// `place`. Lines `at + 1..end` continue it and are nested deeper than `parent`.
    /// Returns the node and the index of the first line after it.
    ///
    /// When `text` holds nothing but a comment, the node starts on the next line that
    /// holds more, as YAML passes over comments. Under a key, a block sequence at the
    /// key's own indentation also starts the value, as js-yaml's `composeNode` reads one
    /// there. Otherwise the node is null.
    ///
    /// A quoted scalar or flow collection continues to its close whatever the
    /// indentation, so it may close past `end`. The lines nested under the line it closes
    /// on then end the node instead.
    ///
    /// Syntax errors: on a key's line, a mapping or sequence entry, as js-yaml throws `bad
    /// indentation of a mapping entry` for the text it leaves there; after a quoted scalar
    /// or flow collection, anything but a comment on the line it closes on, or a line
    /// holding more before the node's end; what [`Yaml::sequence`],
    /// [`Yaml::block_scalar`] and [`Yaml::plain`] reject; and what [`Yaml::closed_value`]
    /// and [`Yaml::passed_over_end`] return for a quoted scalar or flow collection.
    fn node(
        &self,
        at: usize,
        text: &'a str,
        end: usize,
        parent: usize,
        place: Place,
    ) -> Result<(Node, usize), Failure> {
        let text = trim_blanks(text);
        let (content, commented) = uncommented(text);
        if content.is_empty() {
            let own_line = if place == Place::KeyLine { Place::UnderKey } else { place };
            if let Some(start) = (at + 1..end).find(|&index| !is_blank_or_comment(self.line(index)))
            {
                return self.node(start, self.line(start), end, parent, own_line);
            }
            let next = self.line(end);
            if place == Place::KeyLine
                && indent_of(next) == parent
                && next.get(parent..).is_some_and(is_sequence_entry)
            {
                let sequence_end = self.compact_sequence_end(end, parent);
                return self.sequence(end, parent, sequence_end, Place::UnderKey, parent);
            }
            return Ok((Node::Null, end));
        }
        if is_sequence_entry(text) {
            if place == Place::KeyLine {
                return Err(self.error(at, place.bad_indentation()));
            }
            let column = self.line(at).len() - text.len();
            return self.sequence(at, column, end, place, parent);
        }
        if let Some(value) = entry_value(text) {
            if place == Place::KeyLine {
                return Err(self.error(at, place.bad_indentation()));
            }
            return Ok((Node::Mapping, self.passed_over_end(at, value, end, parent)?));
        }
        if is_block_scalar_header(text) {
            return Ok((Node::Text(self.block_scalar(at, text, end, parent, place)?), end));
        }
        if let Some(closed) = self.closed_value(at, text)? {
            let end = end.max(self.nested_end(closed.line, parent));
            if !uncommented(trim_blanks(closed.rest)).0.is_empty() {
                return Err(self.error(closed.line, place.bad_indentation()));
            }
            self.check_lines(closed.line + 1, end, place)?;
            return Ok((closed.value, (closed.line + 1).max(end)));
        }
        Ok((self.plain(at, content, commented, end, place)?, end))
    }

    /// Reads the block sequence standing at `place` whose first entry's `-` is at
    /// `column` of line `at`, nested under a node at `parent`. Its entries are on lines
    /// before `end` whose `-` is at the same column. Returns the sequence and the index of
    /// the first line after it.
    ///
    /// When an entry closes past `end`, the end is found again from there: as
    /// [`Yaml::compact_sequence_end`] says for a sequence at its parent's own indentation,
    /// which only a compact sequence under a key is, and as [`Yaml::nested_end`] says for
    /// any other.
    ///
    /// Syntax error: a line holding more than blanks or a comment after its last entry,
    /// before `end`, with the message for `place`, as js-yaml throws one for a line that
    /// does not fit after the sequence.
    fn sequence(
        &self,
        at: usize,
        column: usize,
        mut end: usize,
        place: Place,
        parent: usize,
    ) -> Result<(Node, usize), Failure> {
        let mut items = Vec::new();
        let mut entry = Some(at);
        let mut after = end;
        while let Some(start) = entry {
            let entry_end = self.nested_end(start, column).min(end);
            let after_dash = self.line(start).get(column + 1..).unwrap_or_default();
            let (item, next) = self.node(start, after_dash, entry_end, column, Place::Entry)?;
            items.push(item);
            after = entry_end.max(next);
            if after > end {
                end = if column == parent {
                    self.compact_sequence_end(after, column)
                } else {
                    self.nested_end(after.saturating_sub(1), parent)
                };
            }
            let line = self.line(after);
            entry =
                (after < end && indent_of(line) == column && is_sequence_entry(trim_blanks(line)))
                    .then_some(after);
        }
        self.check_lines(after, end, place)?;
        Ok((Node::Sequence(items), after.max(end)))
    }

    /// Reads the text of the block scalar standing at `place` whose header `header` ends
    /// line `at`. Lines `at + 1..end` hold it, each without the content's indentation.
    ///
    /// The header's indentation indicator gives that indentation, added to `parent`'s.
    /// Without one, the first line holding more than blanks gives it (YAML 1.2
    /// §8.1.1.1). Only a comment line indented less ends the content, and is dropped; any
    /// other line indented less is the syntax error below.
    ///
    /// Folding and chomping are not applied: [`one_row`] later draws every run of blanks
    /// and line breaks as one space.
    ///
    /// Syntax error: a line indented less that is not a comment, or one after such a
    /// comment, with the message for `place`, as js-yaml throws one there.
    fn block_scalar(
        &self,
        at: usize,
        header: &str,
        end: usize,
        parent: usize,
        place: Place,
    ) -> Result<String, Failure> {
        let indicator = header
            .chars()
            .take_while(|&c| !BLANKS.contains(&c))
            .find_map(|c| c.to_digit(10).and_then(|digit| usize::try_from(digit).ok()));
        let content_indent = indicator.map(|digit| parent + digit).or_else(|| {
            (at + 1..end)
                .map(|index| self.line(index))
                .find(|line| !trim_blanks(line).is_empty())
                .map(indent_of)
                .filter(|&indent| indent > parent)
        });
        let mut content = Vec::new();
        let mut ended = content_indent.is_none();
        for index in at + 1..end {
            let line = self.line(index);
            if trim_blanks(line).is_empty() {
                continue;
            }
            let indent = content_indent.filter(|&indent| !ended && indent_of(line) >= indent);
            match indent {
                Some(indent) => content.push(line.get(indent..).unwrap_or_default()),
                None if trim_blanks(line).starts_with('#') => ended = true,
                None => return Err(self.error(index, place.bad_indentation())),
            }
        }
        Ok(content.join("\n"))
    }

    /// Reads the quoted scalar or flow collection that `text`, a suffix of line `at`,
    /// starts with, up to its close across any lines, whatever their indentation, as
    /// js-yaml only warns of a deficient one. `None` when `text` starts with no quote or
    /// bracket.
    ///
    /// Syntax errors: a quote or bracket that nothing closes, with js-yaml's message (see
    /// [`unclosed_quote`] and [`UNCLOSED_COLLECTION`]) on the line after the last, where
    /// js-yaml stops; and text left after an item's value in a flow collection (see
    /// [`flow_value`] and [`Yaml::flow_error`]).
    fn closed_value(&self, at: usize, text: &str) -> Result<Option<Closed<'a>>, Failure> {
        let start =
            self.starts.get(at).copied().unwrap_or_default() + self.line(at).len() - text.len();
        let from = self.block.get(start..).unwrap_or_default();
        let (value, after) = match text.chars().next() {
            Some(quote @ ('"' | '\'')) => {
                let (quoted, after) =
                    scalar(from).ok_or_else(|| self.error_at_end(unclosed_quote(quote)))?;
                (Node::Text(quoted.text().into_owned()), after)
            }
            Some('[' | '{') => {
                let (node, length) = flow_node(from).map_err(|e| self.flow_error(start, e))?;
                (node, from.get(length..).unwrap_or_default())
            }
            _ => return Ok(None),
        };
        let close = self.block.len() - after.len();
        Ok(Some(Closed {
            value,
            rest: after.split('\n').next().unwrap_or_default(),
            line: self.line_of(close.saturating_sub(1)),
        }))
    }

    /// Reads the bare scalar standing at `place` whose first line's text is `content`.
    /// `content` is line `at` before its comment, and `commented` tells whether one ends
    /// it. Lines `at + 1..end` continue it, up to a comment.
    ///
    /// A continuation line that starts with `- ` continues the scalar, as in YAML.
    ///
    /// Syntax errors, with the message for `place`, as js-yaml throws one there: a
    /// continuation line that holds a mapping entry, and a line holding more than blanks
    /// or a comment after the comment that ends the scalar.
    fn plain(
        &self,
        at: usize,
        content: &str,
        commented: bool,
        end: usize,
        place: Place,
    ) -> Result<Node, Failure> {
        let mut parts = vec![content];
        let mut last = at;
        let mut ended = commented;
        while !ended && last + 1 < end {
            last += 1;
            let (text, comment) = uncommented(trim_blanks(self.line(last)));
            if mapping_colon(text).is_some() {
                return Err(self.error(last, place.bad_indentation()));
            }
            if !text.is_empty() {
                parts.push(text);
            }
            ended = comment;
        }
        self.check_lines(last + 1, end, place)?;
        let text = parts.join(" ");
        Ok(if is_null(&text) { Node::Null } else { Node::Plain(text) })
    }

    /// Errors with the message for `place` at the first line from `from` before `end`
    /// that holds more than blanks or a comment: nothing may follow the node before it.
    fn check_lines(&self, from: usize, end: usize, place: Place) -> Result<(), Failure> {
        match (from..end).find(|&index| !is_blank_or_comment(self.line(index))) {
            Some(index) => Err(self.error(index, place.bad_indentation())),
            None => Ok(()),
        }
    }

    /// Passes over a value that is not read and returns the index of the first line
    /// after it. The value starts on line `index` with `start`, which may be blank or a
    /// comment, and lines before `end` continue it, nested under a node at `parent`.
    ///
    /// Each quoted scalar or flow collection that starts the value, or the value of an
    /// entry nested in it, runs to its close (see [`Yaml::closed_value`]). The lines
    /// nested under `parent` after the line it closes on then continue the value, so it
    /// may end past `end`. A block scalar's lines are passed over unread.
    ///
    /// Syntax error: a quote or bracket that nothing closes, and text left in a flow
    /// collection's item, as [`Yaml::closed_value`] throws.
    // A line continuing a bare scalar that starts with `- ` and a quote is taken for a
    // sequence entry holding a quoted scalar: telling the two apart needs reading the
    // value.
    fn passed_over_end(
        &self,
        mut index: usize,
        start: &'a str,
        mut end: usize,
        parent: usize,
    ) -> Result<usize, Failure> {
        let mut start = Some(start);
        loop {
            if let Some(text) = start.map(trim_blanks) {
                if is_block_scalar_header(text) {
                    index = self.nested_end(index, indent_of(self.line(index))) - 1;
                } else if let Some(line) = self.closed_value(index, text)?.map(|c| c.line) {
                    end = end.max(self.nested_end(line, parent));
                    index = line;
                }
            }
            index += 1;
            if index >= end {
                return Ok(end);
            }
            start = entry_value(self.line(index));
        }
    }
}

/// `text` without the [`BLANKS`] it starts with.
fn trim_blanks(text: &str) -> &str {
    text.trim_start_matches(BLANKS)
}

/// The text of the quoted scalar `text` is, when it starts with a quote that closes.
fn quoted_text(text: &str) -> Option<Cow<'_, str>> {
    text.starts_with(['"', '\'']).then(|| scalar(text)).flatten().map(|(quoted, _)| quoted.text())
}

/// The text the value of an entry on `line` starts with, after any sequence entries'
/// `-` and a mapping entry's key; `None` when `line` holds no entry.
fn entry_value(line: &str) -> Option<&str> {
    let mut text = trim_blanks(line);
    let mut entry = false;
    while is_sequence_entry(text) {
        text = trim_blanks(text.get(1..).unwrap_or_default());
        entry = true;
    }
    if let Some(colon) = mapping_colon(text) {
        return Some(trim_blanks(text.get(colon + 1..).unwrap_or_default()));
    }
    entry.then_some(text)
}

/// The message js-yaml's `readFlowCollection` throws for an item that holds text after
/// its value.
const MISSED_COMMA: &str = "missed comma between flow collection entries";

/// Why a flow collection cannot be read, with what js-yaml's `readFlowCollection`
/// throws for it.
enum FlowError {
    /// The collection, or a quoted item in it, never closes: the message of
    /// [`UNCLOSED_COLLECTION`] or [`unclosed_quote`], thrown at the stream's end.
    Unclosed(&'static str),
    /// An item holds text after its value, as in `["a" b]`, `[[a] b]` or `{a: b: c}`.
    /// `breaks` counts the line breaks from the start of the text read to that text,
    /// whose line js-yaml throws on: a count holds where comments are left out.
    MissedComma { breaks: usize },
}

impl FlowError {
    /// The error for text that `before` comes before: a missed comma moves down by the
    /// line breaks in `before`.
    fn after(self, before: &str) -> Self {
        match self {
            Self::MissedComma { breaks } => {
                Self::MissedComma { breaks: breaks + before.matches('\n').count() }
            }
            unclosed @ Self::Unclosed(_) => unclosed,
        }
    }
}

/// The node of the flow collection `text` starts with, and its byte length up to and
/// including the closing bracket. Each item is read by [`flow_value`].
///
/// Errors: what [`flow_items`] throws for a collection nothing closes, and what
/// [`flow_value`] throws for an item, counted from the start of `text`.
fn flow_node(text: &str) -> Result<(Node, usize), FlowError> {
    let (items, length) = flow_items(text)?;
    let mut nodes = Vec::with_capacity(items.len());
    for item in &items {
        let before = text.get(..item.at).unwrap_or_default();
        nodes.push(flow_value(&item.text).map_err(|error| error.after(before))?);
    }
    let node = if text.starts_with('{') { Node::Mapping } else { Node::Sequence(nodes) };
    Ok((node, length))
}

/// The node of `item`, an item of a flow collection or the value of a flow mapping's
/// pair: a flow collection, a single-pair mapping when it holds a mapping entry, or else
/// a scalar.
///
/// Error: [`FlowError::MissedComma`], counted from the item's first character other
/// than a blank or line break, for text after a nested collection or a quoted scalar,
/// or after the value of a pair, where a bare scalar stops at a `:` before a blank as
/// js-yaml's `readPlainScalar` does, and for what a nested collection's items hold.
fn flow_value(item: &str) -> Result<Node, FlowError> {
    let item = item.trim_matches(FLOW_BLANKS);
    // The error for text left at byte `at` of `item`.
    let missed_comma = |at: usize| FlowError::MissedComma {
        breaks: item.get(..at).unwrap_or_default().matches('\n').count(),
    };
    // The byte index in `item` of the first character of `rest`, a suffix of `item`,
    // other than a blank or line break.
    let start_of = |rest: &str| item.len() - rest.trim_start_matches(FLOW_BLANKS).len();
    if item.starts_with(['[', '{']) {
        let (node, length) = flow_node(item)?;
        let rest = item.get(length..).unwrap_or_default();
        if !rest.trim_matches(FLOW_BLANKS).is_empty() {
            return Err(missed_comma(start_of(rest)));
        }
        return Ok(node);
    }
    if let Some(colon) = mapping_colon(item) {
        let raw = item.get(colon + 1..).unwrap_or_default();
        let value = raw.trim_matches(FLOW_BLANKS);
        let value_at = start_of(raw);
        if let Some(inner) = mapping_colon(value) {
            return Err(missed_comma(value_at + inner));
        }
        let before = item.get(..value_at).unwrap_or_default();
        flow_value(value).map_err(|error| error.after(before))?;
        return Ok(Node::Mapping);
    }
    if item.starts_with(['"', '\''])
        && let Some((quoted, rest)) = scalar(item)
    {
        if !rest.trim_matches(FLOW_BLANKS).is_empty() {
            return Err(missed_comma(start_of(rest)));
        }
        return Ok(Node::Text(quoted.text().into_owned()));
    }
    Ok(if is_null(item) { Node::Null } else { Node::Plain(item.to_owned()) })
}

/// An item of a flow collection, as [`flow_items`] splits it off.
struct FlowItem {
    /// The item's text, without comments.
    text: String,
    /// The byte index in the collection's text of the item's first character other than
    /// a blank or line break.
    at: usize,
}

/// The items of the flow collection `text` starts with, split at its top-level commas,
/// without its comments, and its byte length up to and including the closing bracket.
///
/// A `,`, bracket or brace inside a quoted item or a nested collection splits nothing,
/// and an empty last item, as in `[a, ]` or `[]`, is no item. A `#` outside a quoted
/// item starts a comment running to the line's end when a blank, a line break, `[`, `{`
/// or `,` comes right before it, as js-yaml's `readFlowCollection` passes over comments
/// after an opening bracket and between items.
///
/// Error: when nothing closes the collection, [`FlowError::Unclosed`] with the message
/// js-yaml throws for what it ends inside: a quoted item (see [`unclosed_quote`]) or
/// the collection ([`UNCLOSED_COLLECTION`]).
fn flow_items(text: &str) -> Result<(Vec<FlowItem>, usize), FlowError> {
    let mut items = Vec::new();
    let mut item = FlowItem { text: String::new(), at: 0 };
    let mut depth = 0_usize;
    let mut quote: Option<char> = None;
    // Whether only blanks have come since the item or a `:` started: a quote starts a
    // quoted scalar only there, and is a character of a bare one anywhere else.
    let mut at_start = true;
    let mut prev = ' ';
    let mut chars = text.char_indices().peekable();
    while let Some((at, c)) = chars.next() {
        if let Some(open) = quote {
            item.text.push(c);
            let escaped = match c {
                '\\' if open == '"' => true,
                '\'' if open == '\'' => chars.peek().is_some_and(|&(_, next)| next == '\''),
                _ => false,
            };
            if escaped {
                if let Some((_, next)) = chars.next() {
                    item.text.push(next);
                }
            } else if c == open {
                quote = None;
            }
            prev = c;
            continue;
        }
        if c == '#' && (FLOW_BLANKS.contains(&prev) || matches!(prev, '[' | '{' | ',')) {
            while chars.peek().is_some_and(|&(_, next)| next != '\n') {
                chars.next();
            }
            continue;
        }
        let separates = match c {
            '"' | '\'' if at_start => {
                quote = Some(c);
                false
            }
            '[' | '{' => {
                depth += 1;
                depth == 1
            }
            ']' | '}' => {
                depth = depth.checked_sub(1).ok_or(FlowError::Unclosed(UNCLOSED_COLLECTION))?;
                if depth == 0 {
                    if !item.text.trim_matches(FLOW_BLANKS).is_empty() {
                        items.push(item);
                    }
                    return Ok((items, at + 1));
                }
                false
            }
            ',' if depth == 1 => {
                let next = FlowItem { text: String::new(), at: 0 };
                items.push(std::mem::replace(&mut item, next));
                true
            }
            _ => false,
        };
        if !separates {
            if item.text.trim_matches(FLOW_BLANKS).is_empty() && !FLOW_BLANKS.contains(&c) {
                item.at = at;
            }
            item.text.push(c);
        }
        at_start = matches!(c, '[' | '{' | ',' | ':') || (at_start && FLOW_BLANKS.contains(&c));
        prev = c;
    }
    Err(FlowError::Unclosed(quote.map_or(UNCLOSED_COLLECTION, unclosed_quote)))
}

/// Whether `value` is the header of a YAML block scalar (YAML 1.2 §8.1.1): `|` or `>`,
/// its chomping and indentation indicators, and nothing else but blanks or a comment
/// after a blank.
fn is_block_scalar_header(value: &str) -> bool {
    let Some(indicators) = value.strip_prefix(['|', '>']) else { return false };
    let after = indicators.trim_start_matches(|c: char| matches!(c, '+' | '-' | '1'..='9'));
    let rest = trim_blanks(after);
    rest.is_empty() || (rest.len() < after.len() && rest.starts_with('#'))
}

/// `text` before its comment, a `#` at its start or after a blank (YAML 1.2 §6.6), without
/// the blanks at its end, and whether a comment ends it. A `#` right after other text, as
/// in `Hello#tag`, is part of the text.
fn uncommented(text: &str) -> (&str, bool) {
    let comment = text
        .match_indices('#')
        .find(|&(at, _)| at == 0 || text.get(..at).is_some_and(|b| b.ends_with(BLANKS)));
    let end = comment.map_or(text.len(), |(at, _)| at);
    (text.get(..end).unwrap_or_default().trim_end_matches(BLANKS), comment.is_some())
}

/// Whether `line` holds nothing but blanks or a comment.
fn is_blank_or_comment(line: &str) -> bool {
    let text = trim_blanks(line);
    text.is_empty() || text.starts_with('#')
}

/// The number of spaces `line` starts with: YAML indents with spaces only.
fn indent_of(line: &str) -> usize {
    line.len() - line.trim_start_matches(' ').len()
}

/// Whether the character at byte index `at` of `text` comes before a blank or the end.
fn before_blank(text: &str, at: usize) -> bool {
    text.get(at + 1..).is_some_and(|after| after.is_empty() || after.starts_with(BLANKS))
}

/// Whether `text` starts a block sequence entry: `-` before a blank or the end.
fn is_sequence_entry(text: &str) -> bool {
    text.starts_with('-') && before_blank(text, 0)
}

/// The byte index of the `:` that makes `text`, the content of a line, a block mapping
/// entry: after a quoted key's closing quote and any blanks, or else the first `:` before
/// a blank or the end outside a comment, so that `title:x` is one plain scalar. It counts
/// a quoted key, so callers ask it before reading a quoted scalar. `None` for a flow
/// collection: a `:` inside `[…]` or `{…}` belongs to the collection and makes no block
/// mapping entry.
fn mapping_colon(text: &str) -> Option<usize> {
    if text.starts_with(['[', '{']) {
        return None;
    }
    if text.starts_with(['"', '\'']) {
        let (_, after) = scalar(text)?;
        let rest = trim_blanks(after);
        return rest.starts_with(':').then(|| text.len() - rest.len());
    }
    plain_colon(uncommented(text).0)
}

/// The byte index of the first `:` in `content`, text holding no comment, that comes
/// before a blank or the end: where js-yaml's `readPlainScalar` stops a bare scalar.
fn plain_colon(content: &str) -> Option<usize> {
    content.match_indices(':').find(|&(at, _)| before_blank(content, at)).map(|(at, _)| at)
}

/// `text` on one row: each run of ASCII blanks and line breaks drawn as one space and
/// none at either end, as a browser draws the SVG `text` element upstream draws the
/// title in, under CSS's `white-space: normal`.
fn one_row(text: &str) -> String {
    text.split_ascii_whitespace().collect::<Vec<_>>().join(" ")
}

/// Whether the bare YAML scalar `value` reads as null under js-yaml's `JSON_SCHEMA`:
/// empty, `~`, `null`, `Null` or `NULL`.
fn is_null(value: &str) -> bool {
    matches!(value, "" | "~" | "null" | "Null" | "NULL")
}

/// Whether the bare YAML scalar `value`, which is not null, reads as a falsy JavaScript
/// value under js-yaml's `JSON_SCHEMA`, which upstream's `extractFrontMatter` loads the
/// frontmatter with before keeping the title only `if (parsed.title)`: `false` (bool), or
/// an int or float equal to 0 or NaN.
///
/// js-yaml's int type (`lib/type/int.js`) reads an optional sign and decimal digits, or
/// `0b`, `0o` or `0x` (lowercase only) and digits of that base; it reads no `_`. Its float
/// type (`lib/type/float.js`) reads `YAML_FLOAT_PATTERN`: decimal digits with an optional
/// sign, fraction and exponent, a fraction without digits or sign before its `.`, `.inf`
/// with an optional sign, and `.nan`, which is NaN. `.inf` is truthy.
fn is_falsy_plain_scalar(value: &str) -> bool {
    if matches!(value, "false" | "False" | "FALSE" | ".nan" | ".NaN" | ".NAN") {
        return true;
    }
    let unsigned = value.strip_prefix(['-', '+']).unwrap_or(value);
    // A prefixed int is 0 when all its digits are `0`, which are digits of every base.
    for prefix in ["0b", "0o", "0x"] {
        if let Some(digits) = unsigned.strip_prefix(prefix) {
            return !digits.is_empty() && digits.chars().all(|c| c == '0');
        }
    }
    is_decimal_number(value) && value.parse::<f64>().is_ok_and(|number| number == 0.0)
}

/// Whether `value` matches the decimal forms of js-yaml's `YAML_FLOAT_PATTERN`, which
/// hold its decimal ints: `[-+]?[0-9]+(\.[0-9]*)?([eE][-+]?[0-9]+)?` or
/// `\.[0-9]+([eE][-+]?[0-9]+)?`.
fn is_decimal_number(value: &str) -> bool {
    let digits = |text: &str| !text.is_empty() && text.bytes().all(|byte| byte.is_ascii_digit());
    let (mantissa, exponent) = match value.split_once(['e', 'E']) {
        Some((mantissa, exponent)) => (mantissa, Some(exponent)),
        None => (value, None),
    };
    let exponent_fits = exponent
        .is_none_or(|exponent| digits(exponent.strip_prefix(['-', '+']).unwrap_or(exponent)));
    let mantissa_fits = match mantissa.strip_prefix('.') {
        Some(fraction) => digits(fraction),
        None => {
            let unsigned = mantissa.strip_prefix(['-', '+']).unwrap_or(mantissa);
            match unsigned.split_once('.') {
                Some((whole, fraction)) => {
                    digits(whole) && (fraction.is_empty() || digits(fraction))
                }
                None => digits(unsigned),
            }
        }
    };
    mantissa_fits && exponent_fits
}
