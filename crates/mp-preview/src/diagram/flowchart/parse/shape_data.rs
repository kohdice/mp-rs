//! The `@{ … }` shape data of a node: its shape, label and other keys.

use std::borrow::Cow;

use crate::diagram::Failure;
use crate::diagram::flowchart::chart::Shape;
use crate::diagram::flowchart::label::Label;

use super::front_matter::{
    BLANKS, END_EXPECTED, indent_of, is_blank_or_comment, mapping_colon, unclosed_quote,
    uncommented,
};
use super::syntax_error;

/// What a node's `@{ … }` shape data sets.
pub(super) struct ShapeData {
    pub(super) shape: Option<Shape>,
    /// Replaces the label, also one given in brackets.
    pub(super) label: Option<Label>,
    /// Whether the `view` value is `collapsed`, when one is given; it matters only on a
    /// subgraph's id.
    pub(super) collapsed: Option<bool>,
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

/// The byte index of the `}` closing the shape data `text`, which follows the `@{`: the
/// first one outside double quotes, as upstream's `shapeData` lexer state reads
/// `[^}^"]+`, `"}"` and `["]`, which opens the `shapeDataStr` state that the next `"`
/// closes. A `'` opens no string there, so a `}` inside single quotes ends the data.
pub(super) fn shape_data_end(text: &str) -> Option<usize> {
    outside_strings(text).find(|&(_, c)| c == '}').map(|(at, _)| at)
}

/// The byte index of the `}` closing the shape data `text` (see [`shape_data_end`]),
/// or the syntax error on `line` when the data is unclosed or holds a `^` outside double
/// quotes, which upstream's `shapeData` lexer state has no rule for.
fn checked_shape_data_end(line: usize, text: &str) -> Result<usize, Failure> {
    let end = shape_data_end(text).ok_or_else(|| syntax_error(line, "unclosed shape data"))?;
    let inside = text.get(..end).unwrap_or_default();
    if outside_strings(inside).any(|(_, c)| c == '^') {
        return Err(syntax_error(line, "shape data holds \"^\""));
    }
    Ok(end)
}

/// The byte index of the `}` closing the data `text` of a link, which follows the `@{`
/// (see [`shape_data_end`]), or the syntax error on `line` when the data is unclosed,
/// holds a `^` outside double quotes or is not YAML [`shape_data`] would read. Upstream's
/// `addVertex` reads the data before it tells a link's id from a node's, and then
/// ignores a link's `shape` and `label`, so the keys are read and dropped.
pub(super) fn edge_data_end(line: usize, text: &str) -> Result<usize, Failure> {
    let end = checked_shape_data_end(line, text)?;
    data_pairs(line, text.get(..end).unwrap_or_default())?;
    Ok(end)
}

/// The `key: value` pairs of the data `inside` `@{` and `}`. Upstream's `addVertex` wraps
/// data on one line in `{`, `}` to read it as a YAML flow mapping, and reads data that
/// holds a line break as it stands, a block mapping; a line break inside double quotes
/// does not count, as the `shapeDataStr` lexer state has replaced it with `<br/>`.
fn data_pairs(line: usize, inside: &str) -> Result<Vec<DataPair<'_>>, Failure> {
    if outside_strings(inside).any(|(_, c)| c == '\n') {
        block_pairs(line, inside)
    } else {
        flow_pairs(line, inside)
    }
}

/// The characters of `text`, with their byte indexes, that stand outside double quotes
/// as upstream's `shapeData` lexer state reads them (see [`shape_data_end`]), the quotes
/// themselves excluded.
fn outside_strings(text: &str) -> impl Iterator<Item = (usize, char)> + '_ {
    let mut in_string = false;
    text.char_indices().filter(move |&(_, c)| {
        let outside = !in_string && c != '"';
        in_string ^= c == '"';
        outside
    })
}

/// Reads the `key: value` pairs of `@{ … }` shape data from `text`, which follows the
/// `@{`, and returns them with the text after the closing `}` (see [`shape_data_end`]).
/// A `^` outside double quotes is a syntax error, as no rule of the `shapeData` lexer
/// state reads one. An empty `shape` or `label` sets nothing, as upstream's `addVertex`
/// in `flowDb.ts` skips a falsy one. On the id of a `subgraph` whose `end` has been read
/// only `view` is read, as `addVertex` merges the data into the subgraph's and returns
/// before it looks at a shape, an icon or an image.
pub(super) fn shape_data(
    line: usize,
    text: &str,
    closed_subgraph: bool,
) -> Result<(ShapeData, &str), Failure> {
    let end = checked_shape_data_end(line, text)?;
    let pairs = data_pairs(line, text.get(..end).unwrap_or_default())?;
    let mut data = ShapeData { shape: None, label: None, collapsed: None };
    let mut image = false;
    for DataPair { key, value } in pairs {
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
pub(super) enum Scalar<'a> {
    /// Written bare, possibly over several lines (see [`folded_breaks`]).
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
    pub(super) fn text(self) -> Cow<'a, str> {
        match self {
            Self::Plain(text) => folded_breaks(text),
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
    /// and the blanks after it with `<br/>` before YAML reads the value; YAML folds the
    /// line breaks of a single-quoted value (see [`folded_breaks`]).
    fn label(self) -> Label {
        match self {
            Self::Plain(_) => Label::parse(&self.text()),
            Self::DoubleQuoted(_) => Label::string(&breaks_as_tags(&self.text())),
            Self::SingleQuoted(_) => Label::string(&folded_breaks(&self.text())),
        }
    }
}

/// `text` folded as YAML folds a flow scalar spanning lines: the blanks around each line
/// break are dropped, a line break followed by a non-blank line becomes one space, and
/// each blank line between two lines becomes one line break.
fn folded_breaks(text: &str) -> Cow<'_, str> {
    let Some((first, rest)) = text.split_once('\n') else {
        return Cow::Borrowed(text);
    };
    let mut folded = first.to_owned();
    let mut blank_lines = 0;
    let mut lines = rest.split('\n').peekable();
    while let Some(line) = lines.next() {
        let line = line.trim_start();
        if line.is_empty() && lines.peek().is_some() {
            blank_lines += 1;
            continue;
        }
        folded.truncate(folded.trim_end().len());
        if blank_lines == 0 {
            folded.push(' ');
        }
        folded.extend(std::iter::repeat_n('\n', blank_lines));
        folded.push_str(line);
        blank_lines = 0;
    }
    Cow::Owned(folded)
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

/// One `key: value` pair of shape data, as [`flow_pairs`] or [`block_pairs`] finds it.
struct DataPair<'a> {
    key: &'a str,
    value: Scalar<'a>,
}

/// Reads the shape data `data` on one line, the line breaks inside double-quoted values
/// aside, as the YAML flow mapping upstream wraps it in (see [`shape_data`]); this reads
/// the flat form its documentation shows: pairs separated by commas, each value bare or
/// quoted, a quoted one holding any commas and line breaks. A quote starts a quoted
/// value only at the value's start, as in YAML, so `it's` stays bare. A `#` at the start
/// of a key or after a blank starts a comment running to the end of `data`. A `:`
/// separates a key from its value only before a blank, a `,` or the end of `data`, so
/// `shape:rect` is one key without a value. Text between a quoted value and the next
/// separator is a syntax error, as YAML rejects two pairs without a separator between
/// them, and so is a quoted value that nothing closes, with js-yaml's message.
fn flow_pairs(line: usize, data: &str) -> Result<Vec<DataPair<'_>>, Failure> {
    let mut pairs = Vec::new();
    let mut rest = data;
    loop {
        rest = rest.trim_start_matches(|c: char| c.is_whitespace() || c == ',');
        if rest.is_empty() {
            return Ok(pairs);
        }
        if rest.starts_with('#') {
            rest = rest.get(rest.find('\n').unwrap_or(rest.len())..).unwrap_or_default();
            continue;
        }
        let (key, after_key) = rest.split_at(plain_end(rest, true));
        // A key without a value sets nothing.
        let Some(after_colon) = after_key.strip_prefix(':') else {
            rest = after_comment(after_key);
            continue;
        };
        let value_text = after_colon.trim_start_matches(BLANKS);
        let (value, after) = quoted_or_plain(line, value_text)?;
        let after = after_comment(after);
        let (after_value, next) = after.split_at(after.find([',', '\n']).unwrap_or(after.len()));
        if !after_value.trim().is_empty() {
            return Err(syntax_error(line, "invalid shape data"));
        }
        pairs.push(DataPair { key: key.trim(), value });
        rest = next;
    }
}

/// Reads multi-line shape data `data` as the YAML block mapping upstream reads it as (see
/// [`shape_data`]), with js-yaml's messages: its first key sets the mapping's
/// indentation, and each line at that indentation holds a pair, a line without a key
/// setting nothing; a line indented less is an error. A bare value runs to the end of
/// its line or a comment, commas included, and continues on the deeper lines after it,
/// each joined to it with a blank, until a comment or a line holding a key; a quoted
/// value runs to its closing quote, across lines. Any other deeper line, and text after
/// a quoted value on its closing line, is an error.
fn block_pairs(line: usize, data: &str) -> Result<Vec<DataPair<'_>>, Failure> {
    let bad_indentation = || syntax_error(line, "bad indentation of a mapping entry");
    let mut pairs: Vec<DataPair<'_>> = Vec::new();
    let mut indent = None;
    // Where the last pair's bare value starts in `data`, while a deeper line may still
    // continue it.
    let mut open_plain = None;
    let mut at = 0;
    while let Some(rest) = data.get(at..).filter(|rest| !rest.is_empty()) {
        let row = rest.split('\n').next().unwrap_or_default();
        let next = at + row.len() + 1;
        let content = row.trim_start_matches(BLANKS);
        let content_at = at + row.len() - content.len();
        if is_blank_or_comment(content) {
            if !content.is_empty() {
                open_plain = None;
            }
            at = next;
            continue;
        }
        let column = indent_of(row);
        let mapping_indent = *indent.get_or_insert(column);
        if column > mapping_indent {
            let (Some(start), None) = (open_plain, mapping_colon(content)) else {
                return Err(bad_indentation());
            };
            let (text, commented) = uncommented(content);
            if let Some(pair) = pairs.last_mut() {
                // A value left empty on its key's line starts on the next line.
                let start =
                    if matches!(pair.value, Scalar::Plain("")) { content_at } else { start };
                pair.value =
                    Scalar::Plain(data.get(start..content_at + text.len()).unwrap_or_default());
            }
            if commented {
                open_plain = None;
            }
            at = next;
            continue;
        }
        if column < mapping_indent {
            return Err(syntax_error(line, END_EXPECTED));
        }
        open_plain = None;
        at = next;
        let Some(colon) = mapping_colon(content) else { continue };
        let key = content.get(..colon).unwrap_or_default().trim_end_matches(BLANKS);
        let value_text = content.get(colon + 1..).unwrap_or_default().trim_start_matches(BLANKS);
        let value_at = content_at + content.len() - value_text.len();
        if value_text.starts_with(['"', '\'']) {
            let (value, after) = quoted_or_plain(line, data.get(value_at..).unwrap_or_default())?;
            let rest_of_row = after.split('\n').next().unwrap_or_default();
            if !uncommented(rest_of_row).0.trim_start_matches(BLANKS).is_empty() {
                return Err(bad_indentation());
            }
            pairs.push(DataPair { key, value });
            at = data.len() - after.len() + rest_of_row.len() + 1;
        } else {
            let (text, commented) = uncommented(value_text);
            pairs.push(DataPair { key, value: Scalar::Plain(text) });
            open_plain = (!commented).then_some(value_at);
        }
    }
    Ok(pairs)
}

/// Splits the scalar at the start of `text` off it, as [`scalar`] does; a quoted one that
/// nothing closes is a syntax error with js-yaml's message.
fn quoted_or_plain(line: usize, text: &str) -> Result<(Scalar<'_>, &str), Failure> {
    scalar(text).ok_or_else(|| {
        let quote = if text.starts_with('\'') { '\'' } else { '"' };
        syntax_error(line, unclosed_quote(quote))
    })
}

/// `text` from the line break ending the comment it starts with after blanks, or
/// `text` itself when it starts with no comment; empty when no line break ends it.
fn after_comment(text: &str) -> &str {
    let after_blanks = text.trim_start_matches([' ', '\t']);
    if after_blanks.len() == text.len() || !after_blanks.starts_with('#') {
        return text;
    }
    after_blanks.get(after_blanks.find('\n').unwrap_or(after_blanks.len())..).unwrap_or_default()
}

/// The byte index where the bare scalar at the start of `text` ends: at a comma or a
/// line break, or a blank before a `#`, which starts a comment; for a `key`, also at a
/// `:` before a blank, a line break, a `,` or the end of `text`.
fn plain_end(text: &str, key: bool) -> usize {
    let mut chars = text.char_indices().peekable();
    while let Some((at, c)) = chars.next() {
        let next = chars.peek().map(|&(_, next)| next);
        let ends = match c {
            ',' | '\n' => true,
            ' ' | '\t' => next == Some('#'),
            ':' => key && next.is_none_or(|next| next.is_whitespace() || next == ','),
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
pub(super) fn scalar(text: &str) -> Option<(Scalar<'_>, &str)> {
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
