//! Link tokens: their strokes, markers, lengths, text and edge ids.

use crate::diagram::flowchart::chart::{Marker, Stroke};

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
    /// Whether the start of a `-- text -->` link does not match its closing token, which
    /// upstream's `destructLink` reads as an `INVALID` link. `addSingleLink` stores that
    /// link without checking it, so a browser draws it in an undefined way; a character
    /// drawing cannot reproduce that, and the link is a syntax error here.
    pub(super) mismatched: bool,
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
            mismatched: false,
        };
        return (length > 0).then_some((link, rest));
    }
    let (tail, stroke, after_base) = link_start(text)?;
    let after_dots = after_base.trim_start_matches('.');
    let (label, dots, closing_tail, closing) = match stroke {
        Stroke::Dotted if after_dots.starts_with('-') => {
            (None, after_base.len() - after_dots.len(), tail, after_dots)
        }
        Stroke::Dotted if opens_text(stroke, after_base) => {
            let (text, closing) =
                after_base.split_at_checked(find_after_quotes(after_base, ".-")?)?;
            // The closing token, `[xo<]?\-?\.+\-` in `flow.jison`, takes the whole run of
            // dots before its `-`, a `-` before those dots and a marker before that, so
            // they end no text.
            let before_dots = text.trim_end_matches('.');
            let before_line = before_dots.strip_suffix('-').unwrap_or(before_dots);
            let (label, closing_tail) = split_closing_tail(before_line);
            (Some(label.trim()), text.len() - before_dots.len(), closing_tail, closing.get(1..)?)
        }
        Stroke::Solid | Stroke::Thick if opens_text(stroke, after_base) => {
            let doubled = if stroke == Stroke::Solid { "--" } else { "==" };
            let (text, closing) =
                after_base.split_at_checked(find_after_quotes(after_base, doubled)?)?;
            // The closing token, `[xo<]?\-\-+[-xo>]` in `flow.jison`, takes a marker
            // right before its line characters, so that marker ends no text.
            let (label, closing_tail) = split_closing_tail(text);
            (Some(label.trim()), 0, closing_tail, closing.get(2..)?)
        }
        Stroke::Dotted | Stroke::Invisible => return None,
        Stroke::Solid | Stroke::Thick => (None, 0, tail, after_base),
    };
    let (head, length, rest) = link_end(stroke, closing)?;
    // Upstream's `destructEndLink` reads a leading `<`, `o` or `x` of the closing token
    // as a second head only when it matches the end (`<-->`, `o--o`, `x--x`); otherwise
    // the character is part of the line and counts toward the length of a solid or thick
    // link. The start of a `-- text -->` link is a token of its own, which
    // `destructStartLink` reads instead; `destructLink` gives a start without a marker
    // the heads of the closing token, so `-- text o--o` is two-headed.
    let unmatched_closing_tail = closing_tail.is_some() && closing_tail != head;
    // A start with a marker must have the closing token's type in `destructLink`: the
    // same single head, which a second head on the closing token turns into a double one.
    // A thick start must also meet a thick closing token.
    let mismatched = label.is_some()
        && (tail.is_some_and(|tail| Some(tail) != head || closing_tail == head)
            || (stroke == Stroke::Thick && unmatched_closing_tail));
    let (stroke, tail, length) = if unmatched_closing_tail {
        // `destructEndLink` reads a thick stroke only from a line that starts with `=`,
        // so the unmatched marker left in front makes the link a normal one.
        let stroke = if stroke == Stroke::Thick { Stroke::Solid } else { stroke };
        let length = if stroke == Stroke::Dotted { length } else { length + 1 };
        (stroke, if label.is_some() { tail } else { None }, length)
    } else {
        (stroke, tail.or(closing_tail), length)
    };
    let length = (length + dots).min(MAX_LINK_LENGTH);
    Some((Link { stroke, tail, head, label, length, mismatched }, rest))
}

/// Splits the start of a link other than `~~~` off `text`: its tail marker (`<`, `o`,
/// `x`, or none), its stroke, read from `-.`, `--` or `==`, and the text after those two
/// characters.
pub(super) fn link_start(text: &str) -> Option<(Option<Marker>, Stroke, &str)> {
    let tail = text.chars().next().and_then(tail_marker);
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

/// Splits an edge id, the `@` after it and the blanks after that off the start of
/// `text`, for Mermaid's `LINK_ID` token `[^\s\"\[\]\(\)\{\}\|]+\@(?=[^\{\"])`, which
/// stands before a link whose `LINK` token starts with `\s*`. The match is greedy, so
/// the id runs to the last `@` of its run of characters that a character other than `{`
/// or `"` follows, and may hold `@` itself (`e1@x@-->` has the id `e1@x`).
pub(super) fn edge_id(text: &str) -> Option<(&str, &str)> {
    let run = text
        .find(|c: char| {
            c.is_whitespace() || matches!(c, '"' | '[' | ']' | '(' | ')' | '{' | '}' | '|')
        })
        .unwrap_or(text.len());
    text.get(..run)?.match_indices('@').rev().find_map(|(at, _)| {
        let (id, after_at) = (text.get(..at)?, text.get(at + 1..)?);
        let next = after_at.chars().next()?;
        (!id.is_empty() && next != '{' && next != '"').then_some((id, after_at.trim_start()))
    })
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

/// Splits a `<`, `o` or `x` that ends `text` off it: the marker that the closing token
/// of a `-- text -->` link may take before its line characters.
fn split_closing_tail(text: &str) -> (&str, Option<Marker>) {
    let mut chars = text.chars();
    match chars.next_back().and_then(tail_marker) {
        Some(marker) => (chars.as_str(), Some(marker)),
        None => (text, None),
    }
}

/// The marker a link character at the source end stands for.
fn tail_marker(c: char) -> Option<Marker> {
    match c {
        '<' => Some(Marker::Arrow),
        'o' => Some(Marker::Circle),
        'x' => Some(Marker::Cross),
        _ => None,
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
