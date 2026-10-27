//! Flowchart source text to nodes and edges, following upstream Mermaid's grammar
//! (`packages/mermaid/src/diagrams/flowchart/parser/flow.jison` in
//! <https://github.com/mermaid-js/mermaid>).

mod builder;
mod front_matter;
mod link;
mod shape_data;
mod statements;
mod tokens;

use crate::diagram::{Failure, SyntaxError};

use super::chart::{Direction, Flowchart};

use self::builder::Builder;
use self::statements::{split_statements, without_comment_lines};

pub(super) use self::front_matter::front_matter;
pub(super) use self::statements::{text_size, with_single_quoted_attributes};

/// Parses the flowchart `source`, whose first line is line `first_line` of the
/// diagram's source. `source` uses `\n` line endings: the Markdown layer
/// (`sanitize_block_body`) normalises CR before any diagram source reaches the parser.
/// Its HTML tags' attribute values are already single-quoted (see
/// [`with_single_quoted_attributes`]).
pub(super) fn parse(source: &str, first_line: usize) -> Result<Flowchart, Failure> {
    let (cleaned, source_lines) = without_comment_lines(source);
    let ends_at_semicolon = |statement: &str| {
        cleaned
            .substr_range(statement)
            .and_then(|range| cleaned.get(range.end..))
            .is_some_and(|after| after.starts_with(';'))
    };
    let mut statements = split_statements(&cleaned)
        .into_iter()
        .map(|(line, text)| (first_line + source_lines.get(line).copied().unwrap_or(line), text));
    // Upstream's detector needs the header at the start of the source after blanks,
    // directives and comments, so a `;` before it leaves the diagram type unknown.
    let (header_line, header) = loop {
        let (line, text) = statements.next().ok_or(Failure::Unsupported)?;
        if !text.trim().is_empty() {
            break (line, text);
        }
        if ends_at_semicolon(text) {
            return Err(Failure::Unsupported);
        }
    };
    let mut builder =
        Builder::new(direction(header_line, header.trim(), ends_at_semicolon(header))?);
    // Where the line of the last direction statement ends: a statement starting before
    // it is part of that direction statement.
    let mut taken_until = 0;
    // Where the current statement's line ends, and its `direction_words` from the line's
    // first statement on: many statements may share a line, which is searched only once.
    let (mut line_end, mut line_words) = (0, [None; DIRECTION_WORDS.len()]);
    for (line, text) in statements {
        let start = cleaned.substr_range(text).map_or(0, |range| range.start)
            + (text.len() - text.trim_start().len());
        let text = text.trim();
        if text.is_empty() || start < taken_until {
            continue;
        }
        if start >= line_end {
            let rest_of_line =
                cleaned.get(start..).unwrap_or_default().split('\n').next().unwrap_or_default();
            line_end = start + rest_of_line.len();
            line_words = direction_words(rest_of_line).map(|at| at.map(|at| start + at));
        }
        let words = line_words.map(|at| at.and_then(|at| at.checked_sub(start)));
        let followed_by = cleaned.get(start + text.len()..).and_then(|after| after.chars().next());
        if builder.statement(line, text, words, followed_by)? {
            taken_until = line_end;
        }
    }
    builder.finish()
}

/// Reads the direction of a `flowchart`, `graph` or `flowchart-elk` header; `TD`, `TB`,
/// `v`, `BR` and an omitted direction mean top-down, and `>`, `<` and `^` mean `LR`, `RL`
/// and `BT`, as Mermaid does; any other word is a syntax error. `flowchart-elk` only
/// chooses upstream's layout engine, which has no counterpart in a text drawing. Other
/// diagram kinds are unsupported. Like Mermaid, the direction must end the statement:
/// anything after it on the same line needs a `;` first. A header without a direction
/// must end at a line break, not at a `;` (`semicolon`): after the keyword upstream's
/// lexer is in its `dir` state, whose only rules are the direction words and `NODIR`, a
/// line break. The flag says whether the direction is spelled `BR` (see
/// [`Flowchart::spelled_br`]).
fn direction(line: usize, header: &str, semicolon: bool) -> Result<(Direction, bool), Failure> {
    let mut words = header.split_ascii_whitespace();
    if !matches!(words.next(), Some("flowchart" | "graph" | "flowchart-elk")) {
        return Err(Failure::Unsupported);
    }
    let word = words.next();
    let direction = match word {
        None if semicolon => {
            return Err(syntax_error(line, "expected a direction or a new line after the header"));
        }
        Some("LR" | ">") => Direction::LeftToRight,
        None | Some("TD" | "TB" | "v") => Direction::TopDown,
        // Upstream's lexer reads `BR` as a direction and keeps it, and its layout draws a
        // direction it does not know top-down.
        Some("BR") => Direction::TopDown,
        Some("BT" | "^") => Direction::BottomUp,
        Some("RL" | "<") => Direction::RightToLeft,
        Some(other) => return Err(syntax_error(line, format!("unknown direction \"{other}\""))),
    };
    if words.next().is_some() {
        return Err(syntax_error(line, "expected a new line or \";\" after the direction"));
    }
    Ok((direction, word == Some("BR")))
}

/// The words of upstream's lexer rules `.*direction\s+TB[^\n]*`, then `BT`, `RL`, `LR`
/// and `TD`, in the order the rules come.
const DIRECTION_WORDS: [(&str, Direction); 5] = [
    ("TB", Direction::TopDown),
    ("BT", Direction::BottomUp),
    ("RL", Direction::RightToLeft),
    ("LR", Direction::LeftToRight),
    ("TD", Direction::TopDown),
];

/// For each of [`DIRECTION_WORDS`], where in `line` (one line, without its line break)
/// the last `direction` followed by blanks and that word starts, the word possibly
/// starting a longer one (`TBX`). `direction` cannot overlap itself, so
/// `match_indices` finds every occurrence.
fn direction_words(line: &str) -> [Option<usize>; DIRECTION_WORDS.len()] {
    let mut last = [None; DIRECTION_WORDS.len()];
    for (at, keyword) in line.match_indices("direction") {
        let after = line.get(at + keyword.len()..).unwrap_or_default();
        let word_start = after.trim_start();
        if word_start.len() < after.len() {
            for (slot, (word, _)) in last.iter_mut().zip(DIRECTION_WORDS) {
                if word_start.starts_with(word) {
                    *slot = Some(at);
                }
            }
        }
    }
    last
}

/// The word and the direction of a statement when it is a `direction` statement, as
/// upstream's direction rules read one: the whole line, when `direction`, blanks and a
/// word of [`DIRECTION_WORDS`] appear anywhere on it from the statement's start, the
/// first word in that order winning. `words` holds the line's [`direction_words`],
/// counted from a point at or before the statement's start, which is `offset` past that
/// point; a position before it is `None`. Only the statement's first line is searched, as
/// `.` stops at a line break, and a `;` on that line ends nothing, so the statement takes
/// the rest of its line.
fn direction_statement(
    words: [Option<usize>; DIRECTION_WORDS.len()],
    offset: usize,
) -> Option<(&'static str, Direction)> {
    DIRECTION_WORDS
        .into_iter()
        .zip(words)
        .find_map(|(word, at)| at.is_some_and(|at| at >= offset).then_some(word))
}

fn syntax_error(line: usize, message: impl Into<String>) -> Failure {
    Failure::Syntax(SyntaxError { line: Some(line), message: message.into() })
}
