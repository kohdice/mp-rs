//! Lexical checks against the tokens of upstream's lexer: ids, bracketed labels and
//! the characters label text may not hold.

use std::cmp::Ordering;

use crate::diagram::Failure;
use crate::diagram::flowchart::chart::Shape;
use crate::diagram::flowchart::label::leading_string;

use super::link::link_start;
use super::syntax_error;

/// The opening brackets of a node label, each with the closing brackets it may end with
/// and the shape each pair gives. An opener comes before the shorter openers it starts
/// with.
pub(super) const BRACKETS: [(&str, &[(&str, Shape)]); 13] = [
    ("[(", &[(")]", Shape::Cylinder)]),
    ("(((", &[(")))", Shape::DoubleCircle)]),
    ("((", &[("))", Shape::Circle)]),
    // `flow.jison`'s `<ellipseText>[-/\)][\)]` closes an ellipse with any of the three.
    ("(-", &[("-)", Shape::Ellipse), ("/)", Shape::Ellipse), ("))", Shape::Ellipse)]),
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
/// with an id character. An id is upstream's `idString`, which joins `NODE_STRING`
/// characters (see [`is_id_char`]) with `COLON` and `COMMA` tokens. A `:` starting
/// `:::` ends the id, as the lexer reads `:::` as `STYLE_SEPARATOR` before `:`.
pub(super) fn split_id(text: &str) -> (&str, &str) {
    let end = text
        .char_indices()
        .find(|&(index, c)| {
            let after = text.get(index + c.len_utf8()..).unwrap_or_default();
            let joins = match c {
                ',' => true,
                ':' => !after.starts_with("::"),
                _ => is_id_char(c, after.chars().next()),
            };
            !joins
        })
        .map_or(text.len(), |(index, _)| index);
    text.split_at(end)
}

/// The keywords of `flow.jison`'s `INITIAL` lexer state that are read as whole words, in
/// the order of their rules. Each rule is a string pattern ending in a word character,
/// to which jison appends `\b` (`end` spells it out), and comes before `NODE_STRING`, so
/// it wins wherever a token starts with the keyword and no word character follows.
const WORD_KEYWORDS: [&str; 17] = [
    "style",
    "default",
    "linkStyle",
    "interpolate",
    "classDef",
    "class",
    "flowchart-elk",
    "swimlane-beta",
    "graph",
    "flowchart",
    "subgraph",
    "end",
    "_self",
    "_blank",
    "_parent",
    "_top",
    "v",
];

/// The keyword of [`WORD_KEYWORDS`] that upstream's lexer reads at the start of `text`,
/// if any: the first one `text` starts with that no word character (`[A-Za-z0-9_]`, what
/// `\b` sees as one) follows.
pub(super) fn leading_keyword(text: &str) -> Option<&'static str> {
    WORD_KEYWORDS.into_iter().find(|keyword| {
        text.strip_prefix(keyword).is_some_and(|after| {
            !after.starts_with(|c: char| c.is_ascii_alphanumeric() || c == '_')
        })
    })
}

/// The keyword upstream's lexer reads where a token starts inside `id`, if any, other
/// than `default` and `v`, which `idStringToken` admits: at the id's start and after each
/// `:` and `,`, which it reads as tokens of their own (`COLON`, `COMMA`), each time also
/// after the digits there, which it reads as a `NUM` before trying the keywords again.
/// The keywords are not tried inside a `NODE_STRING`, so `a1end` and `a-end` hold none.
pub(super) fn id_keyword(id: &str) -> Option<&'static str> {
    let token_starts = std::iter::once(0).chain(id.match_indices([':', ',']).map(|(at, _)| at + 1));
    token_starts.filter_map(|start| id.get(start..)).find_map(|token| {
        let after_digits = token.trim_start_matches(|c: char| c.is_ascii_digit());
        [token, after_digits].into_iter().find_map(|token| {
            leading_keyword(token).filter(|&keyword| !matches!(keyword, "default" | "v"))
        })
    })
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

/// Checks that the text of a `-- text -->` link on `line`, if any, is not blank and holds
/// no `"` outside a double-quoted string it starts with. `flow.jison`'s `edgeText` needs
/// one token at least, and `START_LINK` takes the blanks after it, so `A -- --> B` has
/// none; its `edgeText` rule accepts a string only at its start. Brackets and `|` are
/// text here.
pub(super) fn check_link_text(line: usize, text: Option<&str>) -> Result<(), Failure> {
    let Some(text) = text else { return Ok(()) };
    if text.trim().is_empty() {
        return Err(syntax_error(line, "link text is empty"));
    }
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

/// The characters of the `NODE_STRING` token of Mermaid's `flow.jison`, plus those of
/// its `UNICODE_TEXT` token (see [`UNICODE_TEXT`]): a hyphen belongs to an id only when
/// it cannot start a link (`-->`, `---`, `-.`), and an equals sign only when it cannot
/// start a thick link (`==`).
fn is_id_char(c: char, next: Option<char>) -> bool {
    match c {
        '-' => next.is_some_and(|next| !matches!(next, '>' | '-' | '.')),
        '=' => next != Some('='),
        _ if !c.is_ascii() => UNICODE_TEXT
            .binary_search_by(|&(first, last)| {
                if last < c {
                    Ordering::Less
                } else if first > c {
                    Ordering::Greater
                } else {
                    Ordering::Equal
                }
            })
            .is_ok(),
        _ => c.is_ascii_alphanumeric() || "!\"#$%&'*+.`?\\_/".contains(c),
    }
}

/// The inclusive character ranges of `flow.jison`'s `UNICODE_TEXT` token, the only
/// non-ASCII characters an id may hold, sorted and disjoint, copied from the `[\u…]`
/// alternation in `packages/mermaid/src/diagrams/flowchart/parser/flow.jison`
/// (mermaid-js/mermaid, `develop`). Nothing outside the Basic Multilingual Plane belongs:
/// JavaScript reads each `\u` escape as one UTF-16 code unit, and no range holds a
/// surrogate, so an emoji never matches.
const UNICODE_TEXT: [(char, char); 409] = [
    ('\u{00AA}', '\u{00AA}'),
    ('\u{00B5}', '\u{00B5}'),
    ('\u{00BA}', '\u{00BA}'),
    ('\u{00C0}', '\u{00D6}'),
    ('\u{00D8}', '\u{00F6}'),
    ('\u{00F8}', '\u{02C1}'),
    ('\u{02C6}', '\u{02D1}'),
    ('\u{02E0}', '\u{02E4}'),
    ('\u{02EC}', '\u{02EC}'),
    ('\u{02EE}', '\u{02EE}'),
    ('\u{0370}', '\u{0374}'),
    ('\u{0376}', '\u{0376}'),
    ('\u{0377}', '\u{0377}'),
    ('\u{037A}', '\u{037D}'),
    ('\u{0386}', '\u{0386}'),
    ('\u{0388}', '\u{038A}'),
    ('\u{038C}', '\u{038C}'),
    ('\u{038E}', '\u{03A1}'),
    ('\u{03A3}', '\u{03F5}'),
    ('\u{03F7}', '\u{0481}'),
    ('\u{048A}', '\u{0527}'),
    ('\u{0531}', '\u{0556}'),
    ('\u{0559}', '\u{0559}'),
    ('\u{0561}', '\u{0587}'),
    ('\u{05D0}', '\u{05EA}'),
    ('\u{05F0}', '\u{05F2}'),
    ('\u{0620}', '\u{064A}'),
    ('\u{066E}', '\u{066E}'),
    ('\u{066F}', '\u{066F}'),
    ('\u{0671}', '\u{06D3}'),
    ('\u{06D5}', '\u{06D5}'),
    ('\u{06E5}', '\u{06E5}'),
    ('\u{06E6}', '\u{06E6}'),
    ('\u{06EE}', '\u{06EE}'),
    ('\u{06EF}', '\u{06EF}'),
    ('\u{06FA}', '\u{06FC}'),
    ('\u{06FF}', '\u{06FF}'),
    ('\u{0710}', '\u{0710}'),
    ('\u{0712}', '\u{072F}'),
    ('\u{074D}', '\u{07A5}'),
    ('\u{07B1}', '\u{07B1}'),
    ('\u{07CA}', '\u{07EA}'),
    ('\u{07F4}', '\u{07F4}'),
    ('\u{07F5}', '\u{07F5}'),
    ('\u{07FA}', '\u{07FA}'),
    ('\u{0800}', '\u{0815}'),
    ('\u{081A}', '\u{081A}'),
    ('\u{0824}', '\u{0824}'),
    ('\u{0828}', '\u{0828}'),
    ('\u{0840}', '\u{0858}'),
    ('\u{08A0}', '\u{08A0}'),
    ('\u{08A2}', '\u{08AC}'),
    ('\u{0904}', '\u{0939}'),
    ('\u{093D}', '\u{093D}'),
    ('\u{0950}', '\u{0950}'),
    ('\u{0958}', '\u{0961}'),
    ('\u{0971}', '\u{0977}'),
    ('\u{0979}', '\u{097F}'),
    ('\u{0985}', '\u{098C}'),
    ('\u{098F}', '\u{098F}'),
    ('\u{0990}', '\u{0990}'),
    ('\u{0993}', '\u{09A8}'),
    ('\u{09AA}', '\u{09B0}'),
    ('\u{09B2}', '\u{09B2}'),
    ('\u{09B6}', '\u{09B9}'),
    ('\u{09BD}', '\u{09BD}'),
    ('\u{09CE}', '\u{09CE}'),
    ('\u{09DC}', '\u{09DC}'),
    ('\u{09DD}', '\u{09DD}'),
    ('\u{09DF}', '\u{09E1}'),
    ('\u{09F0}', '\u{09F0}'),
    ('\u{09F1}', '\u{09F1}'),
    ('\u{0A05}', '\u{0A0A}'),
    ('\u{0A0F}', '\u{0A0F}'),
    ('\u{0A10}', '\u{0A10}'),
    ('\u{0A13}', '\u{0A28}'),
    ('\u{0A2A}', '\u{0A30}'),
    ('\u{0A32}', '\u{0A32}'),
    ('\u{0A33}', '\u{0A33}'),
    ('\u{0A35}', '\u{0A35}'),
    ('\u{0A36}', '\u{0A36}'),
    ('\u{0A38}', '\u{0A38}'),
    ('\u{0A39}', '\u{0A39}'),
    ('\u{0A59}', '\u{0A5C}'),
    ('\u{0A5E}', '\u{0A5E}'),
    ('\u{0A72}', '\u{0A74}'),
    ('\u{0A85}', '\u{0A8D}'),
    ('\u{0A8F}', '\u{0A91}'),
    ('\u{0A93}', '\u{0AA8}'),
    ('\u{0AAA}', '\u{0AB0}'),
    ('\u{0AB2}', '\u{0AB2}'),
    ('\u{0AB3}', '\u{0AB3}'),
    ('\u{0AB5}', '\u{0AB9}'),
    ('\u{0ABD}', '\u{0ABD}'),
    ('\u{0AD0}', '\u{0AD0}'),
    ('\u{0AE0}', '\u{0AE0}'),
    ('\u{0AE1}', '\u{0AE1}'),
    ('\u{0B05}', '\u{0B0C}'),
    ('\u{0B0F}', '\u{0B0F}'),
    ('\u{0B10}', '\u{0B10}'),
    ('\u{0B13}', '\u{0B28}'),
    ('\u{0B2A}', '\u{0B30}'),
    ('\u{0B32}', '\u{0B32}'),
    ('\u{0B33}', '\u{0B33}'),
    ('\u{0B35}', '\u{0B39}'),
    ('\u{0B3D}', '\u{0B3D}'),
    ('\u{0B5C}', '\u{0B5C}'),
    ('\u{0B5D}', '\u{0B5D}'),
    ('\u{0B5F}', '\u{0B61}'),
    ('\u{0B71}', '\u{0B71}'),
    ('\u{0B83}', '\u{0B83}'),
    ('\u{0B85}', '\u{0B8A}'),
    ('\u{0B8E}', '\u{0B90}'),
    ('\u{0B92}', '\u{0B95}'),
    ('\u{0B99}', '\u{0B99}'),
    ('\u{0B9A}', '\u{0B9A}'),
    ('\u{0B9C}', '\u{0B9C}'),
    ('\u{0B9E}', '\u{0B9E}'),
    ('\u{0B9F}', '\u{0B9F}'),
    ('\u{0BA3}', '\u{0BA3}'),
    ('\u{0BA4}', '\u{0BA4}'),
    ('\u{0BA8}', '\u{0BAA}'),
    ('\u{0BAE}', '\u{0BB9}'),
    ('\u{0BD0}', '\u{0BD0}'),
    ('\u{0C05}', '\u{0C0C}'),
    ('\u{0C0E}', '\u{0C10}'),
    ('\u{0C12}', '\u{0C28}'),
    ('\u{0C2A}', '\u{0C33}'),
    ('\u{0C35}', '\u{0C39}'),
    ('\u{0C3D}', '\u{0C3D}'),
    ('\u{0C58}', '\u{0C58}'),
    ('\u{0C59}', '\u{0C59}'),
    ('\u{0C60}', '\u{0C60}'),
    ('\u{0C61}', '\u{0C61}'),
    ('\u{0C85}', '\u{0C8C}'),
    ('\u{0C8E}', '\u{0C90}'),
    ('\u{0C92}', '\u{0CA8}'),
    ('\u{0CAA}', '\u{0CB3}'),
    ('\u{0CB5}', '\u{0CB9}'),
    ('\u{0CBD}', '\u{0CBD}'),
    ('\u{0CDE}', '\u{0CDE}'),
    ('\u{0CE0}', '\u{0CE0}'),
    ('\u{0CE1}', '\u{0CE1}'),
    ('\u{0CF1}', '\u{0CF1}'),
    ('\u{0CF2}', '\u{0CF2}'),
    ('\u{0D05}', '\u{0D0C}'),
    ('\u{0D0E}', '\u{0D10}'),
    ('\u{0D12}', '\u{0D3A}'),
    ('\u{0D3D}', '\u{0D3D}'),
    ('\u{0D4E}', '\u{0D4E}'),
    ('\u{0D60}', '\u{0D60}'),
    ('\u{0D61}', '\u{0D61}'),
    ('\u{0D7A}', '\u{0D7F}'),
    ('\u{0D85}', '\u{0D96}'),
    ('\u{0D9A}', '\u{0DB1}'),
    ('\u{0DB3}', '\u{0DBB}'),
    ('\u{0DBD}', '\u{0DBD}'),
    ('\u{0DC0}', '\u{0DC6}'),
    ('\u{0E01}', '\u{0E30}'),
    ('\u{0E32}', '\u{0E32}'),
    ('\u{0E33}', '\u{0E33}'),
    ('\u{0E40}', '\u{0E46}'),
    ('\u{0E81}', '\u{0E81}'),
    ('\u{0E82}', '\u{0E82}'),
    ('\u{0E84}', '\u{0E84}'),
    ('\u{0E87}', '\u{0E87}'),
    ('\u{0E88}', '\u{0E88}'),
    ('\u{0E8A}', '\u{0E8A}'),
    ('\u{0E8D}', '\u{0E8D}'),
    ('\u{0E94}', '\u{0E97}'),
    ('\u{0E99}', '\u{0E9F}'),
    ('\u{0EA1}', '\u{0EA3}'),
    ('\u{0EA5}', '\u{0EA5}'),
    ('\u{0EA7}', '\u{0EA7}'),
    ('\u{0EAA}', '\u{0EAA}'),
    ('\u{0EAB}', '\u{0EAB}'),
    ('\u{0EAD}', '\u{0EB0}'),
    ('\u{0EB2}', '\u{0EB2}'),
    ('\u{0EB3}', '\u{0EB3}'),
    ('\u{0EBD}', '\u{0EBD}'),
    ('\u{0EC0}', '\u{0EC4}'),
    ('\u{0EC6}', '\u{0EC6}'),
    ('\u{0EDC}', '\u{0EDF}'),
    ('\u{0F00}', '\u{0F00}'),
    ('\u{0F40}', '\u{0F47}'),
    ('\u{0F49}', '\u{0F6C}'),
    ('\u{0F88}', '\u{0F8C}'),
    ('\u{1000}', '\u{102A}'),
    ('\u{103F}', '\u{103F}'),
    ('\u{1050}', '\u{1055}'),
    ('\u{105A}', '\u{105D}'),
    ('\u{1061}', '\u{1061}'),
    ('\u{1065}', '\u{1065}'),
    ('\u{1066}', '\u{1066}'),
    ('\u{106E}', '\u{1070}'),
    ('\u{1075}', '\u{1081}'),
    ('\u{108E}', '\u{108E}'),
    ('\u{10A0}', '\u{10C5}'),
    ('\u{10C7}', '\u{10C7}'),
    ('\u{10CD}', '\u{10CD}'),
    ('\u{10D0}', '\u{10FA}'),
    ('\u{10FC}', '\u{1248}'),
    ('\u{124A}', '\u{124D}'),
    ('\u{1250}', '\u{1256}'),
    ('\u{1258}', '\u{1258}'),
    ('\u{125A}', '\u{125D}'),
    ('\u{1260}', '\u{1288}'),
    ('\u{128A}', '\u{128D}'),
    ('\u{1290}', '\u{12B0}'),
    ('\u{12B2}', '\u{12B5}'),
    ('\u{12B8}', '\u{12BE}'),
    ('\u{12C0}', '\u{12C0}'),
    ('\u{12C2}', '\u{12C5}'),
    ('\u{12C8}', '\u{12D6}'),
    ('\u{12D8}', '\u{1310}'),
    ('\u{1312}', '\u{1315}'),
    ('\u{1318}', '\u{135A}'),
    ('\u{1380}', '\u{138F}'),
    ('\u{13A0}', '\u{13F4}'),
    ('\u{1401}', '\u{166C}'),
    ('\u{166F}', '\u{167F}'),
    ('\u{1681}', '\u{169A}'),
    ('\u{16A0}', '\u{16EA}'),
    ('\u{1700}', '\u{170C}'),
    ('\u{170E}', '\u{1711}'),
    ('\u{1720}', '\u{1731}'),
    ('\u{1740}', '\u{1751}'),
    ('\u{1760}', '\u{176C}'),
    ('\u{176E}', '\u{1770}'),
    ('\u{1780}', '\u{17B3}'),
    ('\u{17D7}', '\u{17D7}'),
    ('\u{17DC}', '\u{17DC}'),
    ('\u{1820}', '\u{1877}'),
    ('\u{1880}', '\u{18A8}'),
    ('\u{18AA}', '\u{18AA}'),
    ('\u{18B0}', '\u{18F5}'),
    ('\u{1900}', '\u{191C}'),
    ('\u{1950}', '\u{196D}'),
    ('\u{1970}', '\u{1974}'),
    ('\u{1980}', '\u{19AB}'),
    ('\u{19C1}', '\u{19C7}'),
    ('\u{1A00}', '\u{1A16}'),
    ('\u{1A20}', '\u{1A54}'),
    ('\u{1AA7}', '\u{1AA7}'),
    ('\u{1B05}', '\u{1B33}'),
    ('\u{1B45}', '\u{1B4B}'),
    ('\u{1B83}', '\u{1BA0}'),
    ('\u{1BAE}', '\u{1BAE}'),
    ('\u{1BAF}', '\u{1BAF}'),
    ('\u{1BBA}', '\u{1BE5}'),
    ('\u{1C00}', '\u{1C23}'),
    ('\u{1C4D}', '\u{1C4F}'),
    ('\u{1C5A}', '\u{1C7D}'),
    ('\u{1CE9}', '\u{1CEC}'),
    ('\u{1CEE}', '\u{1CF1}'),
    ('\u{1CF5}', '\u{1CF5}'),
    ('\u{1CF6}', '\u{1CF6}'),
    ('\u{1D00}', '\u{1DBF}'),
    ('\u{1E00}', '\u{1F15}'),
    ('\u{1F18}', '\u{1F1D}'),
    ('\u{1F20}', '\u{1F45}'),
    ('\u{1F48}', '\u{1F4D}'),
    ('\u{1F50}', '\u{1F57}'),
    ('\u{1F59}', '\u{1F59}'),
    ('\u{1F5B}', '\u{1F5B}'),
    ('\u{1F5D}', '\u{1F5D}'),
    ('\u{1F5F}', '\u{1F7D}'),
    ('\u{1F80}', '\u{1FB4}'),
    ('\u{1FB6}', '\u{1FBC}'),
    ('\u{1FBE}', '\u{1FBE}'),
    ('\u{1FC2}', '\u{1FC4}'),
    ('\u{1FC6}', '\u{1FCC}'),
    ('\u{1FD0}', '\u{1FD3}'),
    ('\u{1FD6}', '\u{1FDB}'),
    ('\u{1FE0}', '\u{1FEC}'),
    ('\u{1FF2}', '\u{1FF4}'),
    ('\u{1FF6}', '\u{1FFC}'),
    ('\u{2071}', '\u{2071}'),
    ('\u{207F}', '\u{207F}'),
    ('\u{2090}', '\u{209C}'),
    ('\u{2102}', '\u{2102}'),
    ('\u{2107}', '\u{2107}'),
    ('\u{210A}', '\u{2113}'),
    ('\u{2115}', '\u{2115}'),
    ('\u{2119}', '\u{211D}'),
    ('\u{2124}', '\u{2124}'),
    ('\u{2126}', '\u{2126}'),
    ('\u{2128}', '\u{2128}'),
    ('\u{212A}', '\u{212D}'),
    ('\u{212F}', '\u{2139}'),
    ('\u{213C}', '\u{213F}'),
    ('\u{2145}', '\u{2149}'),
    ('\u{214E}', '\u{214E}'),
    ('\u{2183}', '\u{2183}'),
    ('\u{2184}', '\u{2184}'),
    ('\u{2C00}', '\u{2C2E}'),
    ('\u{2C30}', '\u{2C5E}'),
    ('\u{2C60}', '\u{2CE4}'),
    ('\u{2CEB}', '\u{2CEE}'),
    ('\u{2CF2}', '\u{2CF2}'),
    ('\u{2CF3}', '\u{2CF3}'),
    ('\u{2D00}', '\u{2D25}'),
    ('\u{2D27}', '\u{2D27}'),
    ('\u{2D2D}', '\u{2D2D}'),
    ('\u{2D30}', '\u{2D67}'),
    ('\u{2D6F}', '\u{2D6F}'),
    ('\u{2D80}', '\u{2D96}'),
    ('\u{2DA0}', '\u{2DA6}'),
    ('\u{2DA8}', '\u{2DAE}'),
    ('\u{2DB0}', '\u{2DB6}'),
    ('\u{2DB8}', '\u{2DBE}'),
    ('\u{2DC0}', '\u{2DC6}'),
    ('\u{2DC8}', '\u{2DCE}'),
    ('\u{2DD0}', '\u{2DD6}'),
    ('\u{2DD8}', '\u{2DDE}'),
    ('\u{2E2F}', '\u{2E2F}'),
    ('\u{3005}', '\u{3005}'),
    ('\u{3006}', '\u{3006}'),
    ('\u{3031}', '\u{3035}'),
    ('\u{303B}', '\u{303B}'),
    ('\u{303C}', '\u{303C}'),
    ('\u{3041}', '\u{3096}'),
    ('\u{309D}', '\u{309F}'),
    ('\u{30A1}', '\u{30FA}'),
    ('\u{30FC}', '\u{30FF}'),
    ('\u{3105}', '\u{312D}'),
    ('\u{3131}', '\u{318E}'),
    ('\u{31A0}', '\u{31BA}'),
    ('\u{31F0}', '\u{31FF}'),
    ('\u{3400}', '\u{4DB5}'),
    ('\u{4E00}', '\u{9FCC}'),
    ('\u{A000}', '\u{A48C}'),
    ('\u{A4D0}', '\u{A4FD}'),
    ('\u{A500}', '\u{A60C}'),
    ('\u{A610}', '\u{A61F}'),
    ('\u{A62A}', '\u{A62A}'),
    ('\u{A62B}', '\u{A62B}'),
    ('\u{A640}', '\u{A66E}'),
    ('\u{A67F}', '\u{A697}'),
    ('\u{A6A0}', '\u{A6E5}'),
    ('\u{A717}', '\u{A71F}'),
    ('\u{A722}', '\u{A788}'),
    ('\u{A78B}', '\u{A78E}'),
    ('\u{A790}', '\u{A793}'),
    ('\u{A7A0}', '\u{A7AA}'),
    ('\u{A7F8}', '\u{A801}'),
    ('\u{A803}', '\u{A805}'),
    ('\u{A807}', '\u{A80A}'),
    ('\u{A80C}', '\u{A822}'),
    ('\u{A840}', '\u{A873}'),
    ('\u{A882}', '\u{A8B3}'),
    ('\u{A8F2}', '\u{A8F7}'),
    ('\u{A8FB}', '\u{A8FB}'),
    ('\u{A90A}', '\u{A925}'),
    ('\u{A930}', '\u{A946}'),
    ('\u{A960}', '\u{A97C}'),
    ('\u{A984}', '\u{A9B2}'),
    ('\u{A9CF}', '\u{A9CF}'),
    ('\u{AA00}', '\u{AA28}'),
    ('\u{AA40}', '\u{AA42}'),
    ('\u{AA44}', '\u{AA4B}'),
    ('\u{AA60}', '\u{AA76}'),
    ('\u{AA7A}', '\u{AA7A}'),
    ('\u{AA80}', '\u{AAAF}'),
    ('\u{AAB1}', '\u{AAB1}'),
    ('\u{AAB5}', '\u{AAB5}'),
    ('\u{AAB6}', '\u{AAB6}'),
    ('\u{AAB9}', '\u{AABD}'),
    ('\u{AAC0}', '\u{AAC0}'),
    ('\u{AAC2}', '\u{AAC2}'),
    ('\u{AADB}', '\u{AADD}'),
    ('\u{AAE0}', '\u{AAEA}'),
    ('\u{AAF2}', '\u{AAF4}'),
    ('\u{AB01}', '\u{AB06}'),
    ('\u{AB09}', '\u{AB0E}'),
    ('\u{AB11}', '\u{AB16}'),
    ('\u{AB20}', '\u{AB26}'),
    ('\u{AB28}', '\u{AB2E}'),
    ('\u{ABC0}', '\u{ABE2}'),
    ('\u{AC00}', '\u{D7A3}'),
    ('\u{D7B0}', '\u{D7C6}'),
    ('\u{D7CB}', '\u{D7FB}'),
    ('\u{F900}', '\u{FA6D}'),
    ('\u{FA70}', '\u{FAD9}'),
    ('\u{FB00}', '\u{FB06}'),
    ('\u{FB13}', '\u{FB17}'),
    ('\u{FB1D}', '\u{FB1D}'),
    ('\u{FB1F}', '\u{FB28}'),
    ('\u{FB2A}', '\u{FB36}'),
    ('\u{FB38}', '\u{FB3C}'),
    ('\u{FB3E}', '\u{FB3E}'),
    ('\u{FB40}', '\u{FB40}'),
    ('\u{FB41}', '\u{FB41}'),
    ('\u{FB43}', '\u{FB43}'),
    ('\u{FB44}', '\u{FB44}'),
    ('\u{FB46}', '\u{FBB1}'),
    ('\u{FBD3}', '\u{FD3D}'),
    ('\u{FD50}', '\u{FD8F}'),
    ('\u{FD92}', '\u{FDC7}'),
    ('\u{FDF0}', '\u{FDFB}'),
    ('\u{FE70}', '\u{FE74}'),
    ('\u{FE76}', '\u{FEFC}'),
    ('\u{FF21}', '\u{FF3A}'),
    ('\u{FF41}', '\u{FF5A}'),
    ('\u{FF66}', '\u{FFBE}'),
    ('\u{FFC2}', '\u{FFC7}'),
    ('\u{FFCA}', '\u{FFCF}'),
    ('\u{FFD2}', '\u{FFD7}'),
    ('\u{FFDA}', '\u{FFDC}'),
];
