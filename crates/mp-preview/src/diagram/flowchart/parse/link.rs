//! Link tokens: their strokes, markers, lengths, text and edge ids.

use super::{Marker, Stroke};

/// The most layers a link spans, however many line characters it has: upstream's
/// `addSingleLink` lowers a longer link's `length` to 10.
pub(super) const MAX_LINK_LENGTH: usize = 10;

/// The parts of a link token that a drawing shows.
pub(super) struct Link<'a> {
    pub(super) stroke: Stroke,
    pub(super) tail: Option<Marker>,
    pub(super) head: Option<Marker>,
    /// The text of the `A -- text --> B` form: a leading double-quoted string, if any,
    /// quotes included, and the plain text after it, kept as written so that
    /// [`Label::parse_after_string`](crate::diagram::flowchart::label::Label::parse_after_string) can tell the string from that text.
    pub(super) label: Option<&'a str>,
    pub(super) length: usize,
}

/// Splits a link off the start of `text`. These are the `LINK` tokens of Mermaid's
/// `flow.jison`, `[xo<]?--+[-xo>]`, `[xo<]?==+[=xo>]`, `[xo<]?-\.+-[xo>]?` and `~~~+`,
/// and their text forms `-- text -->`, `== text ==>` and `-. text .->`.
pub(super) fn link(text: &str) -> Option<(Link<'_>, &str)> {
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
pub(super) fn link_start(text: &str) -> Option<(Option<Marker>, Stroke, &str)> {
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
pub(super) fn opens_text(stroke: Stroke, after_base: &str) -> bool {
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
pub(super) fn edge_id(text: &str) -> Option<(&str, &str)> {
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
