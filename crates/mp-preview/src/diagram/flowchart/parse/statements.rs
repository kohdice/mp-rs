//! Splitting flowchart source into statements: comment lines, `;` and line breaks,
//! and the directives and accessibility statements passed over.

use std::borrow::Cow;
use std::iter::Peekable;
use std::str::CharIndices;

use crate::diagram::flowchart::label::is_entity_name_char;

use super::link::{Link, link, link_start, opens_text};
use super::shape_data::shape_data_end;
use super::tokens::starts_string;

/// `source` with the double-quoted attribute values of its HTML tags single-quoted, as
/// upstream's `cleanupText` in `packages/mermaid/src/preprocess.ts` rewrites every
/// `="([^"]*)"` inside a tag `<(\w+)([^>]*)>` to `='$1'` before anything else reads the
/// source, since a `"` there would end or start a string. Only quotes change, so every
/// byte index and line of `source` stays where it was.
pub(in crate::diagram::flowchart) fn with_single_quoted_attributes(source: &str) -> Cow<'_, str> {
    let mut quotes = Vec::new();
    let mut from = 0;
    while let Some(open) = source.get(from..).and_then(|rest| rest.find('<')).map(|at| from + at) {
        from = open + 1;
        let after_open = source.get(from..).unwrap_or_default();
        let tag = after_open
            .find(|c: char| !(c.is_ascii_alphanumeric() || c == '_'))
            .unwrap_or(after_open.len());
        let Some(close) = after_open.find('>').filter(|_| tag > 0) else { continue };
        let attributes_at = from + tag;
        let attributes = after_open.get(tag..close).unwrap_or_default();
        let mut at = 0;
        while let Some(equals) = attributes.get(at..).and_then(|rest| rest.find("=\"")) {
            let value = at + equals + 2;
            let Some(length) = attributes.get(value..).and_then(|rest| rest.find('"')) else {
                break;
            };
            quotes.extend([attributes_at + value - 1, attributes_at + value + length]);
            at = value + length + 1;
        }
        from += close + 1;
    }
    if quotes.is_empty() {
        return Cow::Borrowed(source);
    }
    Cow::Owned(
        source
            .char_indices()
            .map(|(at, c)| if quotes.binary_search(&at).is_ok() { '\'' } else { c })
            .collect(),
    )
}

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
/// reports: at each line break and `;` outside a double-quoted string, a label, `@{ … }`
/// shape data and the text of a `-- text -->` link, which upstream's lexer reads in
/// states where both are text. Directives and accessibility statements are dropped.
/// The `;` closing an entity code such as `#quot;` splits nothing: upstream's
/// `encodeEntities` in `packages/mermaid/src/utils.ts` replaces every `#\w+;` in the
/// source before parsing it. Statements are slices of `source`, untrimmed, and may be
/// empty.
///
/// `source` comes without its comment lines (see [`without_comment_lines`]). A `%%` after
/// a directive on the same line, with only blanks between, starts a comment and is
/// dropped; any other `%%` is text, as `flow.jison`'s `NODE_STRING` holds `%`.
pub(super) fn split_statements(source: &str) -> Vec<(usize, &str)> {
    let mut statements = Vec::new();
    let mut labels = Vec::new();
    let mut chars = source.char_indices().peekable();
    let mut scan = Scan::START;
    while let Some((index, c)) = chars.next() {
        let (next, statement) = scan.step(source, (index, c), &mut chars, &mut labels);
        statements.extend(statement);
        scan = next;
    }
    let end = scan.comment.unwrap_or(source.len());
    statements.push((scan.start_line, source.get(scan.start..end).unwrap_or_default()));
    statements
}

/// What [`split_statements`] knows about the source read so far, besides the label lexer
/// states it has entered.
#[derive(Clone, Copy)]
struct Scan {
    /// Whether the scanner is inside a double-quoted string (see [`starts_string`]).
    in_quotes: bool,
    /// Whether the scanner is inside the text of a link that does not close on its line:
    /// upstream reads it in the exclusive `edgeText` lexer state, where a `"` starts a
    /// string anywhere and `;`, `@{` and line breaks are text. A link that closes on its
    /// line is passed over whole.
    in_link_text: bool,
    /// The 0-based line of the character being read.
    line: usize,
    /// The 0-based line and the byte index the current statement starts at.
    start_line: usize,
    start: usize,
    /// Where a `%%` comment cut the current statement short.
    comment: Option<usize>,
    /// Whether the line being read holds nothing but blanks so far, which a comment needs.
    /// A directive passed over counts as blank, as upstream removes directives before
    /// comments.
    line_blank: bool,
    /// How many name characters follow the last `#`, while they may still make an entity
    /// code; `None` outside one.
    entity_name: Option<usize>,
    /// Whether the current statement holds nothing but blanks so far: `flow.jison`'s
    /// grammar admits an accessibility statement only as a whole statement.
    blank: bool,
    /// Whether a statement with text, the header, has been split off.
    after_header: bool,
    /// The byte index of the line break ending the line being read, or of the source's
    /// end, found once per line for the `-- text -->` check.
    line_end: usize,
}

impl Scan {
    const START: Self = Self {
        in_quotes: false,
        in_link_text: false,
        line: 0,
        start_line: 0,
        start: 0,
        comment: None,
        line_blank: true,
        entity_name: None,
        blank: true,
        after_header: false,
        line_end: 0,
    };

    /// The scan after the character `c` at byte `index` of `source`, and the statement it
    /// ends, if any. Text read whole from `c` on (a directive, an accessibility statement,
    /// shape data, a link) is taken from `chars`.
    ///
    /// `labels` holds the label lexer states upstream has pushed and not yet popped,
    /// innermost last. `[`, `(` and `{` push `text` everywhere, as their rules are `<*>`,
    /// except that `[/` and `[\` push `trapText`. `|` pushes `text` outside a label and
    /// pops it inside one, as `<text>"|"` comes before `<*>"|"`, but is a character in
    /// `trapText`, whose `TEXT` rule holds it. The `>` opening an asymmetric label pushes
    /// `text` outside a label only. `]`, `)` and `}` pop whichever state is innermost.
    /// Upstream's `;` rule has no state prefix, so inside a label `;` and line breaks are
    /// text.
    fn step<'a>(
        self,
        source: &'a str,
        (index, c): (usize, char),
        chars: &mut Peekable<CharIndices<'a>>,
        labels: &mut Vec<LabelState>,
    ) -> (Self, Option<(usize, &'a str)>) {
        let mut next = self;
        let mut statement = None;
        let mut ends_statement = false;
        // Whether a statement starts right after the text this character begins: after
        // text passed over, the next statement starts on the same line, so that point is
        // the start of a statement.
        let mut restarts = false;
        let outside_text = !self.in_quotes && !self.in_link_text;
        match c {
            '\n' => ends_statement = outside_text && labels.is_empty(),
            _ if self.comment.is_some() => {}
            '"' if self.in_quotes => next.in_quotes = false,
            '"' => next.in_quotes = self.in_link_text || starts_string(source, index),
            '[' if outside_text
                && chars.peek().is_some_and(|&(_, after)| matches!(after, '/' | '\\')) =>
            {
                labels.push(LabelState::Trap);
            }
            '[' | '(' | '{' if outside_text => labels.push(LabelState::Text),
            ']' | ')' | '}' if outside_text => {
                labels.pop();
            }
            '|' if outside_text => match labels.last() {
                Some(LabelState::Text) => {
                    labels.pop();
                }
                Some(LabelState::Trap) => {}
                None => labels.push(LabelState::Text),
            },
            // The `>` closing an arrow belongs to its `LINK` token, and the header's `>`
            // is a direction, read in upstream's `dir` lexer state.
            '>' if outside_text
                && labels.is_empty()
                && self.after_header
                && source.get(..index).is_none_or(|before| {
                    !["--", "==", ".-"].iter().any(|end| before.ends_with(end))
                }) =>
            {
                labels.push(LabelState::Text);
            }
            ';' => {
                ends_statement = outside_text
                    && labels.is_empty()
                    && self.entity_name.is_none_or(|length| length == 0);
            }
            '%' | 'a' if outside_text => {
                let rest = source.get(index..).unwrap_or_default();
                match passed_over(rest, self.after_header, self.blank) {
                    Some(length) => {
                        let before = source.get(self.start..index).unwrap_or_default();
                        next.after_header |= !before.trim().is_empty();
                        statement = Some((self.start_line, before));
                        next.line += skip_to(chars, index + length);
                        next.start = index + length;
                        next.start_line = next.line;
                        restarts = true;
                    }
                    None if c == '%' && self.line_blank && starts_comment(rest) => {
                        next.comment = Some(index);
                    }
                    None => {}
                }
            }
            '-' | '=' if !self.in_quotes && labels.is_empty() => {
                // Upstream reads the text of a `-- text -->` link in exclusive lexer
                // states (`edgeText` and its thick and dotted kin), where `@{`, `;` and
                // `%%` are text, so a link closing on its line is passed over whole, and
                // one that does not puts the scanner in its text until the link closes.
                if self.line_end <= index {
                    next.line_end = source
                        .get(index..)
                        .and_then(|rest| rest.find('\n'))
                        .map_or(source.len(), |at| index + at);
                }
                let line_rest = source.get(index..next.line_end).unwrap_or_default();
                match link(line_rest) {
                    Some((Link { label: Some(_), .. }, after)) => {
                        let end = index + line_rest.len() - after.len();
                        while chars.next_if(|&(at, _)| at < end).is_some() {}
                    }
                    // The rest of a run of line characters starts no link with text:
                    // upstream's lexer takes the run as one `LINK` token. A lone `-` is
                    // link text and leaves the state as it is.
                    _ => {
                        while chars.next_if(|&(_, after)| after == c).is_some() {}
                        if let Some((_, stroke, after_base)) = link_start(line_rest) {
                            next.in_link_text = opens_text(stroke, after_base);
                        }
                    }
                }
            }
            '@' if outside_text
                && labels.is_empty()
                && chars.peek().is_some_and(|&(_, after)| after == '{') =>
            {
                // Unclosed shape data runs to the end of the source, for the statement to
                // report it.
                let data = index + 2;
                let end = source
                    .get(data..)
                    .and_then(shape_data_end)
                    .map_or(source.len(), |end| data + end);
                next.line += skip_to(chars, end);
            }
            _ => {}
        }
        if ends_statement {
            next.in_link_text = false;
            let end = next.comment.take().unwrap_or(index);
            let text = source.get(self.start..end).unwrap_or_default();
            next.after_header |= !text.trim().is_empty();
            statement = Some((self.start_line, text));
            next.start = index + c.len_utf8();
            next.start_line = next.line + usize::from(c == '\n');
        }
        next.blank = ends_statement || restarts || (self.blank && c.is_whitespace());
        next.line_blank = match c {
            '\n' => true,
            '%' if restarts => self.line_blank,
            _ => self.line_blank && c.is_whitespace(),
        };
        next.line += usize::from(c == '\n');
        next.entity_name = match c {
            '#' => Some(0),
            _ if is_entity_name_char(c) => self.entity_name.map(|length| length + 1),
            _ => None,
        };
        (next, statement)
    }
}

/// Takes the characters before byte `end` from `chars`, returning how many line breaks
/// they hold.
fn skip_to(chars: &mut Peekable<CharIndices<'_>>, end: usize) -> usize {
    let mut breaks = 0;
    while let Some((_, skipped)) = chars.next_if(|&(at, _)| at < end) {
        breaks += usize::from(skipped == '\n');
    }
    breaks
}

/// A label lexer state of upstream's `flow.jison` that [`split_statements`] tracks.
enum LabelState {
    /// `text`, pushed by `[`, `(`, `{`, `|` and `>`.
    Text,
    /// `trapText`, pushed by `[/` and `[\`.
    Trap,
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
/// - A directive (see [`directive_end`]).
/// - An accessibility statement (see [`accessibility_end`]) after the header, which draws
///   nothing; before the header it is text, as upstream's detector needs the source to
///   start with `flowchart` or `graph`.
fn passed_over(rest: &str, after_header: bool, blank: bool) -> Option<usize> {
    directive_end(rest)
        .or_else(|| if after_header && blank { accessibility_end(rest) } else { None })
}

/// The byte length of the directive `text` starts with, as `directiveRegex` in
/// `packages/mermaid/src/diagram-api/regexes.ts` matches it: `%%{`, a keyword with an
/// optional `:` (`init: …`), then an argument, and the `}%%` closing it if one follows.
/// The argument is a single word when one follows the keyword, and else everything up to
/// the next `}%%` or the end of `text`; so `%%{init: theme dark}%%` is only `%%{init:
/// theme `, and leaves `dark}%%` as text. `None` when `text` starts with none; `%%{`
/// without a keyword is neither a directive nor a comment, as the regex needs one.
fn directive_end(text: &str) -> Option<usize> {
    let word_length = |text: &str| {
        text.find(|c: char| !(c.is_ascii_alphanumeric() || c == '_')).unwrap_or(text.len())
    };
    let after_open = text.strip_prefix("%%{")?.trim_start();
    let keyword = word_length(after_open);
    if keyword == 0 {
        return None;
    }
    let after_keyword = after_open.get(keyword..).unwrap_or_default().trim_start();
    let argument = after_keyword.strip_prefix(':').map_or(after_keyword, str::trim_start);
    let after_argument = match word_length(argument) {
        0 => argument.get(argument.find("}%%").unwrap_or(argument.len())..),
        word => argument.get(word..),
    }
    .unwrap_or_default()
    .trim_start();
    let rest = after_argument.strip_prefix("}%%").unwrap_or(after_argument);
    Some(text.len() - rest.len())
}

/// The length of `source`, a diagram's source after its frontmatter, that upstream
/// compares with `maxTextSize`: in UTF-16 units, without its directives, then without its
/// comment lines (see [`without_comment_lines`]), then without the blanks it starts
/// with, as `preprocessDiagram` in `packages/mermaid/src/preprocess.ts` builds the
/// `code.cleaned` that `renderDiagram` measures.
pub(in crate::diagram::flowchart) fn text_size(source: &str) -> usize {
    let mut without_directives = String::with_capacity(source.len());
    let mut rest = source;
    while let Some(at) = rest.find("%%{") {
        let (before, from) = rest.split_at(at);
        without_directives.push_str(before);
        let end = match directive_end(from) {
            Some(end) => end,
            None => {
                without_directives.push_str("%%{");
                "%%{".len()
            }
        };
        rest = from.get(end..).unwrap_or_default();
    }
    without_directives.push_str(rest);
    let (cleaned, _) = without_comment_lines(&without_directives);
    cleaned.trim_start().encode_utf16().count()
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
