//! Splitting flowchart source into statements: comment lines, `;` and line breaks,
//! and the directives and accessibility statements passed over.

use crate::diagram::flowchart::label::is_entity_name_char;

use super::link::{Link, link, link_start, opens_text};
use super::shape_data::shape_data_end;
use super::tokens::starts_string;

/// `source` without its comment lines, plus the 0-based source line of each remaining
/// line (and one past the end), so that errors name lines as written. A comment line is
/// `%%` followed by a character other than `{` or a line break, with any blank lines
/// right before it;
/// upstream's `cleanupComments` (`/^\s*%%(?!{)[^\n]+\n?/gm`) removes it even inside a
/// multi-line quoted string.
pub(super) fn without_comment_lines(source: &str) -> (String, Vec<usize>) {
    let mut cleaned = String::with_capacity(source.len());
    let mut source_lines = Vec::new();
    // Blank lines read since the last other line, held back until the next line tells
    // whether a comment removes them with it.
    let mut blanks: Vec<(usize, &str)> = Vec::new();
    let mut count = 0;
    for (index, line) in source.split_inclusive('\n').enumerate() {
        count = index + 1;
        let content = line.trim_start();
        if starts_comment(content) {
            blanks.clear();
        } else if content.is_empty() {
            blanks.push((index, line));
        } else {
            for (kept, text) in blanks.drain(..).chain(std::iter::once((index, line))) {
                cleaned.push_str(text);
                source_lines.push(kept);
            }
        }
    }
    for (kept, text) in blanks {
        cleaned.push_str(text);
        source_lines.push(kept);
    }
    source_lines.push(count);
    (cleaned, source_lines)
}

/// Splits `source` into statements, each with the 0-based line it starts on, for error
/// reports: at each line break and `;`, and drops the text [`passed_over`] names,
/// ignoring line breaks, `;` and that text inside a double-quoted string (see
/// [`starts_string`]), so that it may span lines as `flow.jison`'s `string` lexer state
/// lets it, inside `@{ … }` shape data up to its closing brace (see [`shape_data_end`]),
/// which upstream's `shapeData` lexer state reads across lines, and inside the text of a
/// `-- text -->` link, from its `--`, `==` or `-.` to its closing link or the end of its
/// statement, which upstream reads in its exclusive `edgeText` lexer states, where a `"`
/// starts a string anywhere. The `;` closing an entity code such as `#quot;` splits
/// nothing: upstream's `encodeEntities` in `packages/mermaid/src/utils.ts` replaces every
/// `#\w+;` in the source before parsing it. Statements are slices of `source`, untrimmed,
/// and may be empty.
///
/// `source` comes without its comment lines (see [`without_comment_lines`]). A `%%` after
/// a directive on the same line, with only blanks between, starts a comment and is
/// dropped; any other `%%` is text, as `flow.jison`'s `NODE_STRING` holds `%`.
pub(super) fn split_statements(source: &str) -> Vec<(usize, &str)> {
    let mut statements: Vec<(usize, &str)> = Vec::new();
    let mut in_quotes = false;
    // Whether the scanner is inside the text of a link that does not close on its line:
    // upstream reads it in the exclusive `edgeText` lexer state, where a `"` starts a
    // string anywhere and `;` and `@{` are text. A link that closes on its line is passed
    // over whole below.
    let mut in_link_text = false;
    let mut line = 0;
    let (mut start_line, mut start) = (0, 0);
    // Where a `%%` comment cut the current statement short.
    let mut comment: Option<usize> = None;
    // Whether the line being read holds nothing but blanks so far, which a comment needs.
    // A directive passed over counts as blank, as upstream removes directives before
    // comments.
    let mut line_blank = true;
    // How many name characters follow the last `#`, while they may still make an entity
    // code; `None` outside one.
    let mut entity_name: Option<usize> = None;
    // Whether the current statement holds nothing but blanks so far. An accessibility
    // statement is read only where a statement starts: this scanner does not track
    // bracket labels, so "accDescr:" inside A[see accDescr: x] must stay text, as
    // upstream's lexer keeps it in its text state.
    let mut blank = true;
    // Whether a statement with text, the header, has been split off.
    let mut after_header = false;
    // The byte index of the line break ending the line being read, or of the source's
    // end, found once per line for the `-- text -->` check.
    let mut line_end = 0;
    let mut chars = source.char_indices().peekable();
    while let Some((index, c)) = chars.next() {
        let mut ends_statement = false;
        // Whether a statement starts right after the text this character begins: after
        // text passed over, the next statement starts on the same line, so that point is
        // the start of a statement.
        let mut restarts = false;
        match c {
            '\n' => ends_statement = !in_quotes,
            _ if comment.is_some() => {}
            '"' if in_quotes => in_quotes = false,
            '"' => in_quotes = in_link_text || starts_string(source, index),
            ';' => {
                ends_statement =
                    !in_quotes && !in_link_text && !entity_name.is_some_and(|length| length > 0);
            }
            '%' | 'a' if !in_quotes && !in_link_text => {
                let rest = source.get(index..).unwrap_or_default();
                match passed_over(rest, after_header, blank) {
                    Some(length) => {
                        let before = source.get(start..index).unwrap_or_default();
                        after_header |= !before.trim().is_empty();
                        statements.push((start_line, before));
                        while let Some(&(at, skipped)) = chars.peek()
                            && at < index + length
                        {
                            line += usize::from(skipped == '\n');
                            chars.next();
                        }
                        start = index + length;
                        start_line = line;
                        restarts = true;
                    }
                    None if c == '%' && line_blank && starts_comment(rest) => {
                        comment = Some(index);
                    }
                    None => {}
                }
            }
            '-' | '=' if !in_quotes => {
                // Upstream reads the text of a `-- text -->` link in exclusive lexer
                // states (`edgeText` and its thick and dotted kin), where `@{`, `;` and
                // `%%` are text, so a link closing on its line is passed over whole, and
                // one that does not puts the scanner in its text until the link closes.
                if line_end <= index {
                    line_end = source
                        .get(index..)
                        .and_then(|rest| rest.find('\n'))
                        .map_or(source.len(), |at| index + at);
                }
                let line_rest = source.get(index..line_end).unwrap_or_default();
                match link(line_rest) {
                    Some((Link { label: Some(_), .. }, after)) => {
                        let end = index + line_rest.len() - after.len();
                        while chars.next_if(|&(at, _)| at < end).is_some() {}
                    }
                    // The rest of a run of line characters starts no link with text:
                    // upstream's lexer takes the run as one `LINK` token. A lone `-` is
                    // link text and leaves the state as it is.
                    _ => {
                        while chars.next_if(|&(_, next)| next == c).is_some() {}
                        if let Some((_, stroke, after_base)) = link_start(line_rest) {
                            in_link_text = opens_text(stroke, after_base);
                        }
                    }
                }
            }
            '@' if !in_quotes
                && !in_link_text
                && chars.peek().is_some_and(|&(_, next)| next == '{') =>
            {
                // Unclosed shape data runs to the end of the source, for the statement to
                // report it.
                let data = index + 2;
                let end = source
                    .get(data..)
                    .and_then(shape_data_end)
                    .map_or(source.len(), |end| data + end);
                while let Some(&(at, skipped)) = chars.peek()
                    && at < end
                {
                    line += usize::from(skipped == '\n');
                    chars.next();
                }
            }
            _ => {}
        }
        if ends_statement {
            in_link_text = false;
            let end = comment.take().unwrap_or(index);
            let text = source.get(start..end).unwrap_or_default();
            after_header |= !text.trim().is_empty();
            statements.push((start_line, text));
            start = index + c.len_utf8();
            start_line = line + usize::from(c == '\n');
        }
        blank = ends_statement || restarts || (blank && c.is_whitespace());
        line_blank = match c {
            '\n' => true,
            '%' if restarts => line_blank,
            _ => line_blank && c.is_whitespace(),
        };
        line += usize::from(c == '\n');
        entity_name = match c {
            '#' => Some(0),
            _ if is_entity_name_char(c) => entity_name.map(|length| length + 1),
            _ => None,
        };
    }
    let end = comment.unwrap_or(source.len());
    statements.push((start_line, source.get(start..end).unwrap_or_default()));
    statements
}

/// Whether `rest`, which starts a line after blanks, starts a comment: `%%` and a
/// character other than `{` on the same line.
fn starts_comment(rest: &str) -> bool {
    rest.strip_prefix("%%")
        .and_then(|after| after.chars().next())
        .is_some_and(|next| !matches!(next, '{' | '\n'))
}

/// The byte length of the text at the start of `rest` that [`split_statements`] passes
/// over whole, ending the statement before it, where `after_header` tells whether the
/// header has been read and `blank` whether `rest` starts a statement:
///
/// - A directive, `%%{` and a keyword (`init: …`), up to and including its `}%%` or the
///   end of the source; `%%{` without a keyword is neither a directive nor a comment, as
///   `directiveRegex` in `packages/mermaid/src/diagram-api/regexes.ts` needs one.
/// - An accessibility statement (see [`accessibility_end`]) after the header, which draws
///   nothing; before the header it is text, as upstream's detector needs the source to
///   start with `flowchart` or `graph`.
fn passed_over(rest: &str, after_header: bool, blank: bool) -> Option<usize> {
    if let Some(after_open) = rest.strip_prefix("%%{") {
        let keyword =
            after_open.trim_start().starts_with(|c: char| c.is_ascii_alphanumeric() || c == '_');
        return keyword.then(|| rest.find("}%%").map_or(rest.len(), |at| at + "}%%".len()));
    }
    if after_header && blank { accessibility_end(rest) } else { None }
}

/// The byte length of the accessibility statement `text` starts with, `flow.jison`'s
/// `acc_title` and `acc_descr` rules: `accTitle:` or `accDescr:` and the rest of the
/// line, `;` included, or `accDescr {` up to the closing `}`. `None` when `text` starts
/// with none, or with an `accDescr {` that nothing closes.
fn accessibility_end(text: &str) -> Option<usize> {
    let blank = [' ', '\t'];
    let rest = match text.strip_prefix("accTitle") {
        Some(after) => after.trim_start_matches(blank).strip_prefix(':')?,
        None => {
            let after = text.strip_prefix("accDescr")?;
            match after.trim_start_matches(blank).strip_prefix(':') {
                Some(after_colon) => after_colon,
                None => {
                    let after_brace = after.trim_start().strip_prefix('{')?;
                    let close = after_brace.find('}')?;
                    return Some(text.len() - after_brace.len() + close + 1);
                }
            }
        }
    };
    Some(text.len() - rest.len() + rest.find('\n').unwrap_or(rest.len()))
}

/// The word after `keyword` and the rest of `statement` after that word, both trimmed
/// and possibly empty, when `statement` is `keyword` alone or `keyword` and a blank;
/// `None` otherwise.
pub(super) fn keyword_statement<'a>(
    statement: &'a str,
    keyword: &str,
) -> Option<(&'a str, &'a str)> {
    let rest = statement.strip_prefix(keyword)?;
    if !(rest.is_empty() || rest.starts_with(|c: char| c.is_ascii_whitespace())) {
        return None;
    }
    let rest = rest.trim_start();
    let (word, after) = rest.split_once(|c: char| c.is_ascii_whitespace()).unwrap_or((rest, ""));
    Some((word, after.trim()))
}
