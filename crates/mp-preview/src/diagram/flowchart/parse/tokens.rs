//! Lexical checks against the tokens of upstream's lexer: ids, bracketed labels and
//! the characters label text may not hold.

use crate::diagram::Failure;
use crate::diagram::flowchart::label::leading_string;

use super::link::link_start;
use super::{Shape, syntax_error};

/// The opening brackets of a node label, each with the closing brackets it may end with
/// and the shape each pair gives. An opener comes before the shorter openers it starts
/// with.
pub(super) const BRACKETS: [(&str, &[(&str, Shape)]); 13] = [
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

/// Splits the longest id prefix off `text`; the id is empty when `text` does not start
/// with an id character.
pub(super) fn split_id(text: &str) -> (&str, &str) {
    let end = text
        .char_indices()
        .find(|&(index, c)| {
            !is_id_char(c, text.get(index + c.len_utf8()..).and_then(|after| after.chars().next()))
        })
        .map_or(text.len(), |(index, _)| index);
    text.split_at(end)
}

/// The characters upstream's lexer never reads as part of an unquoted label's text, with
/// the way a syntax error names them.
pub(super) struct LabelTokens {
    chars: &'static [char],
    names: &'static str,
}

/// The brackets, `|` and `"`, which a plain label's text may not hold, as `TEXT` in
/// upstream's `text` lexer state is `[^\[\]\(\)\{\}\|\"]+`.
pub(super) const LABEL_TOKENS: LabelTokens =
    LabelTokens { chars: &['(', ')', '[', ']', '{', '}', '|', '"'], names: "( ) [ ] { } | or \"" };

/// The brackets, which the text of a lean or trapezoid label (`[/ /]`, `[\ \]`, `[/ \]`,
/// `[\ /]`) may not hold, as `TEXT` in upstream's `trapText` lexer state is
/// `\/(?!\])|\\(?!\])|[^\\\[\]\(\)\{\}\/]+`.
const TRAP_LABEL_TOKENS: LabelTokens =
    LabelTokens { chars: &['(', ')', '[', ']', '{', '}'], names: "( ) [ ] { or }" };

/// Checks that the unquoted text of `label`, as [`bracket_label`] split it off on `line`
/// — all of it, or what follows a leading double-quoted string — holds none of the
/// characters of `tokens`, which the error names as `tokens` does: [`LABEL_TOKENS`], or
/// [`TRAP_LABEL_TOKENS`] for a lean or trapezoid label.
pub(super) fn check_label(line: usize, label: &str, tokens: &LabelTokens) -> Result<(), Failure> {
    let unquoted = leading_string(label).map_or(label, |(_, rest)| rest);
    if !unquoted.contains(tokens.chars) {
        return Ok(());
    }
    Err(syntax_error(
        line,
        format!("unquoted label contains {}; wrap the label in double quotes", tokens.names),
    ))
}

/// Checks that the unquoted text of a subgraph `title` written without brackets on `line`
/// — all of it, or what follows a leading double-quoted string — is made of upstream's
/// `textNoTags` tokens (`NUM`, `NODE_STRING`, `SPACE`, `MINUS`, `AMP`, `UNICODE_TEXT`,
/// `COLON`, `MULT`, `BRKT`, keywords and `START_LINK`), which hold no
/// `( ) [ ] { } | < > , @ ~`, no `--`, `==` or `-.`, and a `"` only inside a word.
/// `START_LINK` is a `textNoTags` token, but it moves upstream's lexer into link text,
/// whose tokens no title may hold.
pub(super) fn check_unbracketed_title(line: usize, title: &str) -> Result<(), Failure> {
    const TOKEN_CHARS: [char; 12] = ['(', ')', '[', ']', '{', '}', '|', '<', '>', ',', '@', '~'];
    let unquoted = leading_string(title).map_or(title, |(_, rest)| rest);
    if unquoted.contains(TOKEN_CHARS)
        || unquoted.char_indices().any(|(at, _)| unquoted.get(at..).and_then(link_start).is_some())
    {
        return Err(syntax_error(
            line,
            "unquoted subgraph title contains ( ) [ ] { } | < > , @ ~ or a link; wrap the title in double quotes",
        ));
    }
    // `textNoTags` may start with a string but takes no other.
    if unquoted.match_indices('"').any(|(at, _)| starts_string(unquoted, at)) {
        return Err(syntax_error(
            line,
            "unquoted subgraph title contains \" at the start of a word; wrap the title in double quotes",
        ));
    }
    Ok(())
}

/// Checks a lean or trapezoid `label` on `line` as [`check_label`] does with
/// [`TRAP_LABEL_TOKENS`], and rejects a `"` right after a `/` or `\` in its unquoted text.
/// Elsewhere a `"` is text, but upstream's `trapText` state reads a `/` or `\` as a token
/// of its own, so the `"` after one starts a token, which its `<*>["]` rule reads as a
/// string the grammar does not accept there.
pub(super) fn check_trap_label(line: usize, label: &str) -> Result<(), Failure> {
    check_label(line, label, &TRAP_LABEL_TOKENS)?;
    let unquoted = leading_string(label).map_or(label, |(_, rest)| rest);
    if !unquoted.contains("/\"") && !unquoted.contains("\\\"") {
        return Ok(());
    }
    Err(syntax_error(
        line,
        "unquoted label contains \" after / or \\; wrap the label in double quotes",
    ))
}

/// Checks that the text of a `-- text -->` link on `line`, if any, holds no `"` outside
/// a double-quoted string it starts with, as `flow.jison`'s `edgeText` rule accepts a
/// string only at its start. Brackets and `|` are text here.
pub(super) fn check_link_text(line: usize, text: Option<&str>) -> Result<(), Failure> {
    let Some(text) = text else { return Ok(()) };
    let unquoted = leading_string(text).map_or(text, |(_, rest)| rest);
    if !unquoted.contains('"') {
        return Ok(());
    }
    Err(syntax_error(line, "unquoted link text contains \"; wrap the text in double quotes"))
}

/// Splits `text`, which follows an opening bracket, into the label and the text after
/// the `close` bracket. A label may start with a string in double quotes, which may
/// contain brackets, and go on in plain text up to `close`; it keeps its quotes, which
/// tell [`Label::parse_after_string`](crate::diagram::flowchart::label::Label::parse_after_string) a quoted string, such as a markdown string, from
/// plain text.
pub(super) fn bracket_label<'a>(text: &'a str, close: &str) -> Option<(&'a str, &'a str)> {
    match text.strip_prefix('"') {
        Some(quoted) => {
            let (inner, after_quote) = quoted.split_once('"')?;
            let (rest, after_close) = after_quote.split_once(close)?;
            let label = text.get(..inner.len() + 2 + rest.len())?;
            Some((label, after_close))
        }
        None => text.split_once(close),
    }
}

/// Whether the `"` at byte index `at` of `text` starts a string: upstream's `<*>["]` rule
/// reads one only where a token starts, and inside a word the `NODE_STRING` token, which
/// holds `"`, has already taken it. Right after the opening bracket of a node label (see
/// [`BRACKETS`]) a token starts even when the bracket ends in a `NODE_STRING` character,
/// as the `/` of `[/`, the `\` of `[\` and the `-` of `(-` do: the lexer has read the
/// bracket as a token of its own and left its `INITIAL` state.
///
/// This is the rule of upstream's `INITIAL` lexer state: inside the text of a link,
/// [`split_statements`](super::statements::split_statements) tracks the state itself.
pub(super) fn starts_string(text: &str, at: usize) -> bool {
    let Some(before) = text.get(..at) else { return true };
    BRACKETS.iter().any(|(open, _)| before.ends_with(open))
        || !before.chars().next_back().is_some_and(|before| is_id_char(before, Some('"')))
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
