//! The `@{ … }` shape data of a node: its shape, label and other keys.

use std::borrow::Cow;

use crate::diagram::Failure;
use crate::diagram::flowchart::label::Label;

use super::{Shape, syntax_error};

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

/// The byte index of the `}` closing the shape data `text`, which follows the `@{`.
pub(super) fn shape_data_end(text: &str) -> Option<usize> {
    scan_shape_data(text).map(|(_, end)| end)
}

/// Reads the `key: value` pairs of `@{ … }` shape data from `text`, which follows the
/// `@{`, and returns them with the text after the closing `}`. Text between a quoted
/// value and the next separator is a syntax error, as YAML rejects two pairs without a
/// separator between them. An empty `shape` or `label` sets nothing, as upstream's
/// `addVertex` in `flowDb.ts` skips a falsy one. On the id of a `subgraph` whose `end`
/// has been read only `view` is read, as `addVertex` merges the data into the
/// subgraph's and returns before it looks at a shape, an icon or an image.
pub(super) fn shape_data(
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
pub(super) enum Scalar<'a> {
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
    pub(super) fn text(self) -> Cow<'a, str> {
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
