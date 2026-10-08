//! Label text as upstream Mermaid shows it, following
//! `packages/mermaid/src/diagrams/common/common.ts` in <https://github.com/mermaid-js/mermaid>.

use std::borrow::Cow;

use entities::ENTITIES;
use unicode_width::UnicodeWidthStr;

use crate::control::visualize_control;

/// The text a node label, edge label or subgraph title shows for `source`: entity codes
/// decoded (see [`decode_entities`]), then made safe to show (see [`shown_text`]), a
/// control character a code decodes to included.
fn label_text(source: &str) -> String {
    shown_text(&decode_entities(source))
}

/// `text` with each tab or no-break space a space and any other control character shown
/// as [`visualize_control`] does, so that text drawn from the source never drives the
/// terminal. A tab has no display width of its own, so it would leave the box narrower
/// than the text shown in it.
pub(super) fn shown_text(text: &str) -> String {
    text.chars()
        .map(|character| match character {
            '\t' | '\u{a0}' => ' ',
            _ => visualize_control(character),
        })
        .collect()
}

/// Whether `character` may appear in the name of an entity code: upstream matches codes
/// with `#\w+;`, and `\w` is `[A-Za-z0-9_]`.
pub(super) fn is_entity_name_char(character: char) -> bool {
    character.is_ascii_alphanumeric() || character == '_'
}

/// `text` with each entity code `#name;` replaced by the character it stands for. Upstream
/// turns an all-digit name into `&#digits;` and any other into `&name;` and lets HTML
/// resolve it, so only decimal code points and HTML named character references decode;
/// any other code, `#x26;` among them, is left as written. Upstream hands an unknown name
/// such as `#nosuch;` to the browser as `&nosuch;`, which shows that text; without a
/// browser there is nothing to hand it to, so the code is shown as written.
fn decode_entities(text: &str) -> String {
    let mut decoded = String::with_capacity(text.len());
    let mut rest = text;
    while let Some((before, after_hash)) = rest.split_once('#') {
        decoded.push_str(before);
        let name_end = after_hash.find(|c| !is_entity_name_char(c)).unwrap_or(after_hash.len());
        let (name, after_name) = after_hash.split_at(name_end);
        match after_name.strip_prefix(';').and_then(|after| Some((entity(name)?, after))) {
            Some((character, after)) => {
                decoded.push_str(&character);
                rest = after;
            }
            None => {
                decoded.push('#');
                rest = after_hash;
            }
        }
    }
    decoded.push_str(rest);
    decoded
}

/// The characters a code from 0x80 to 0x9F stands for in a numeric character reference,
/// from the table of HTML's numeric character reference end state
/// (<https://html.spec.whatwg.org/multipage/parsing.html#numeric-character-reference-end-state>),
/// which reads them as Windows-1252; `None` where the table keeps the C1 control.
const C1_REPLACEMENTS: [Option<char>; 32] = [
    Some('\u{20ac}'),
    None,
    Some('\u{201a}'),
    Some('\u{0192}'),
    Some('\u{201e}'),
    Some('\u{2026}'),
    Some('\u{2020}'),
    Some('\u{2021}'),
    Some('\u{02c6}'),
    Some('\u{2030}'),
    Some('\u{0160}'),
    Some('\u{2039}'),
    Some('\u{0152}'),
    None,
    Some('\u{017d}'),
    None,
    None,
    Some('\u{2018}'),
    Some('\u{2019}'),
    Some('\u{201c}'),
    Some('\u{201d}'),
    Some('\u{2022}'),
    Some('\u{2013}'),
    Some('\u{2014}'),
    Some('\u{02dc}'),
    Some('\u{2122}'),
    Some('\u{0161}'),
    Some('\u{203a}'),
    Some('\u{0153}'),
    None,
    Some('\u{017e}'),
    Some('\u{0178}'),
];

/// The character the decimal entity code `code` stands for, as HTML decodes a numeric
/// character reference: U+FFFD for 0 and for a code that is not a Unicode scalar value,
/// the [`C1_REPLACEMENTS`] for 0x80 to 0x9F, and otherwise the code point itself. A
/// control character it gives is shown by [`label_text`] as for any other.
fn decimal_character(code: u32) -> char {
    let replaced = code
        .checked_sub(0x80)
        .and_then(|offset| C1_REPLACEMENTS.get(usize::try_from(offset).ok()?).copied().flatten());
    match (code, replaced) {
        (0, _) => char::REPLACEMENT_CHARACTER,
        (_, Some(character)) => character,
        _ => char::from_u32(code).unwrap_or(char::REPLACEMENT_CHARACTER),
    }
}

/// The characters the entity code named `name` stands for: an all-digit name is a decimal
/// code (see [`decimal_character`]), U+FFFD when it does not fit a `u32`; any other is an
/// HTML named character reference. `None` for an empty or unknown name.
fn entity(name: &str) -> Option<Cow<'static, str>> {
    if name.is_empty() {
        return None;
    }
    if name.bytes().all(|byte| byte.is_ascii_digit()) {
        let character = name.parse::<u32>().map_or(char::REPLACEMENT_CHARACTER, decimal_character);
        return Some(Cow::Owned(character.to_string()));
    }
    ENTITIES
        .iter()
        .find(|entity| entity.entity.strip_circumfix('&', ';') == Some(name))
        .map(|entity| Cow::Borrowed(entity.characters))
}

/// A stretch of label text drawn in one style.
#[derive(Debug, Clone, PartialEq, Eq)]
pub(super) struct Run {
    pub text: String,
    pub bold: bool,
    pub italic: bool,
}

impl Run {
    fn plain(text: String) -> Self {
        Self { text, bold: false, italic: false }
    }
}

/// The runs of one label row, from left to right.
pub(super) type Row = Vec<Run>;

pub(super) fn row_width(row: &[Run]) -> usize {
    row.iter().map(|run| run.text.width()).sum()
}

/// The text of a node label, edge label or subgraph title, as rows from top to bottom;
/// never empty.
#[derive(Debug, Clone, PartialEq, Eq)]
pub(super) struct Label {
    rows: Vec<Row>,
}

impl Label {
    /// The label written as `source`, which may be a double-quoted string: a markdown
    /// string when its content is wrapped in backticks (see [`Label::markdown`]), and
    /// otherwise a row for each part between line breaks (see [`split_rows`]), the source
    /// lines a quoted string spans joined by a space, its text normalised by
    /// [`label_text`].
    pub(super) fn parse(source: &str) -> Self {
        let text = unquoted(source);
        if text.len() < source.len() { Self::string(text) } else { Self::text(text) }
    }

    /// The label written as `source`, which may start with a double-quoted string and go
    /// on in plain text: the text is appended to the string's content, and the string
    /// decides the kind of the whole, so a markdown string stays one (`flow.jison`'s
    /// `text` and `edgeText` rules). Without a leading string it is read as
    /// [`Label::parse`] reads it.
    pub(super) fn parse_after_string(source: &str) -> Self {
        match leading_string(source) {
            Some((content, rest)) if !rest.is_empty() => Self::string_then(content, rest),
            _ => Self::parse(source),
        }
    }

    /// The label of a quoted string whose content is `content`: a markdown string when
    /// wrapped in backticks, and otherwise read as [`Label::parse`] reads plain text.
    pub(super) fn string(content: &str) -> Self {
        Self::string_then(content, "")
    }

    /// The label of a quoted string whose content is `content`, followed by the plain
    /// text `rest`, which takes the kind of the string.
    fn string_then(content: &str, rest: &str) -> Self {
        match content.strip_circumfix("`", "`") {
            Some(markdown) => Self::markdown(&format!("{markdown}{rest}")),
            None => Self::text(&format!("{content}{rest}")),
        }
    }

    /// A row for each part of `text` between line breaks, without FontAwesome icons (see
    /// [`without_icons`]).
    fn text(text: &str) -> Self {
        let rows = split_rows(text)
            .into_iter()
            .map(|row| vec![Run::plain(label_text(&without_icons(&joined_lines(row))))]);
        Self { rows: rows.collect() }
    }

    /// The label of the markdown string whose content between the backticks is `source`:
    /// a row for each source line and each `<br>`, its leading and trailing blanks and
    /// blank lines dropped (upstream's markdown collapses them), the emphasis of each row
    /// read by [`emphasis_runs`], and then FontAwesome icons dropped from its runs (see
    /// [`without_icon_runs`]). Upstream also wraps a markdown string at a
    /// pixel width (`wrappingWidth`); a text drawing has no such width, so rows break only
    /// where the source does.
    fn markdown(source: &str) -> Self {
        let rows: Vec<Row> = split_rows(source)
            .into_iter()
            .flat_map(|row| row.split('\n'))
            .map(str::trim)
            .filter(|line| !line.is_empty())
            .map(|line| without_icon_runs(emphasis_runs(line)))
            .collect();
        if rows.is_empty() {
            return Self::plain("");
        }
        Self { rows }
    }

    /// A one-row label showing `text`, such as a node's id, without reading markup or
    /// entity codes in it, but with its blanks and control characters shown as
    /// [`shown_text`] shows them in every other label.
    pub(super) fn plain(text: &str) -> Self {
        Self { rows: vec![vec![Run::plain(shown_text(text))]] }
    }

    pub(super) fn rows(&self) -> &[Row] {
        &self.rows
    }

    /// The label on one row, a space between its rows: a subgraph title lives on its
    /// frame's top border, a single row, where stacked rows would push the frame's
    /// content. Upstream draws such a title on several lines down from the cluster's top
    /// border (`rect` in `clusters.js`) and makes room only for
    /// `flowchart.subGraphTitleMargin`, which is 0 by default, so the lines may overlap
    /// the content; a border made of one row of glyphs cannot hold several lines, and
    /// glyphs overlapping the content would hide it.
    pub(super) fn joined(self) -> Self {
        let mut row = Vec::new();
        for (index, runs) in self.rows.into_iter().enumerate() {
            if index > 0 {
                row.push(Run::plain(" ".to_owned()));
            }
            row.extend(runs);
        }
        Self { rows: vec![row] }
    }

    /// Display cells of the widest row.
    pub(super) fn width(&self) -> usize {
        self.rows.iter().map(|row| row_width(row)).max().unwrap_or(0)
    }

    /// The number of rows.
    pub(super) fn height(&self) -> usize {
        self.rows.len()
    }

    /// The row a link's line runs through when the label runs along it, or that links
    /// meet beside a box: the middle row, the upper of the two middle rows for an even
    /// count.
    pub(super) fn middle_row(&self) -> usize {
        (self.height() - 1) / 2
    }
}

/// The prefixes of a FontAwesome icon token `prefix:fa-name`: those of upstream's
/// `replaceIconSubstring` in `packages/mermaid/src/rendering-util/createText.ts`, which
/// matches `(fa[bklrs]?):fa-([\w-]+)`.
// The docs' "Supported prefixes: fa, fab, fas, far, fal, fad"
// (<https://mermaid.js.org/syntax/flowchart.html>) describe registering icon packs and
// do not match the drawing code: a browser shows `fad:fa-x` as written, so it stays text.
const ICON_PREFIXES: [&str; 6] = ["fab", "fas", "far", "fal", "fak", "fa"];

/// The byte length of the icon token `text` starts with and the icon's name without its
/// `fa-`, if it starts with one.
fn icon_token(text: &str) -> Option<(usize, &str)> {
    let after_prefix =
        ICON_PREFIXES.iter().find_map(|prefix| text.strip_prefix(prefix)?.strip_prefix(":fa-"))?;
    let name_length = after_prefix
        .find(|c: char| !(c.is_ascii_alphanumeric() || c == '_' || c == '-'))
        .unwrap_or(after_prefix.len());
    let name = after_prefix.get(..name_length).filter(|name| !name.is_empty())?;
    Some((text.len() - after_prefix.len() + name_length, name))
}

/// `row` without its FontAwesome icon tokens (see [`ICON_PREFIXES`]), each dropped with
/// the blanks after it, or, at the end of the row or before other text, the blanks
/// before it; a row of nothing but icons shows their names. Upstream draws each token as
/// its icon; a terminal has no icon font, so the token is dropped rather than shown as
/// text upstream never shows, and an icon-only row keeps the one text that still says
/// what the icon was, instead of leaving the box empty.
fn without_icons(row: &str) -> Cow<'_, str> {
    let (kept, names) = drop_icons(row);
    if names.is_empty() {
        kept
    } else if kept.trim().is_empty() {
        Cow::Owned(names.join(" "))
    } else {
        kept
    }
}

/// `row` without its icon tokens, dropped as [`without_icons`] drops them, and the names
/// of the icons dropped, in order.
fn drop_icons(row: &str) -> (Cow<'_, str>, Vec<&str>) {
    let mut kept = String::with_capacity(row.len());
    let mut names = Vec::new();
    let mut rest = row;
    while let Some(c) = rest.chars().next() {
        match icon_token(rest) {
            Some((length, name)) => {
                names.push(name);
                let after = rest.get(length..).unwrap_or_default();
                let after_blanks = after.trim_start_matches([' ', '\t']);
                if after_blanks.len() == after.len() {
                    kept.truncate(kept.trim_end_matches([' ', '\t']).len());
                }
                rest = after_blanks;
            }
            None => {
                kept.push(c);
                rest = rest.get(c.len_utf8()..).unwrap_or_default();
            }
        }
    }
    if names.is_empty() { (Cow::Borrowed(row), names) } else { (Cow::Owned(kept), names) }
}

/// The runs of a markdown string's `row` without their icon tokens, each run's dropped as
/// [`without_icons`] drops them: upstream's `createText.ts` applies
/// `replaceIconSubstring` to the HTML `markdownToHTML` gives, so `**fa:fa-car**` is an
/// icon in bold and its markers are not shown. Where runs meet, a blank after a blank is
/// dropped, and so are the blanks at the row's ends, as HTML collapses them. A row of
/// nothing but icons shows their names in the style of the first run holding one.
fn without_icon_runs(row: Row) -> Row {
    let mut names = Vec::new();
    let mut icon_style = None;
    let mut kept: Row = Vec::new();
    for run in &row {
        let (text, run_names) = drop_icons(&run.text);
        if !run_names.is_empty() {
            icon_style.get_or_insert((run.bold, run.italic));
            names.extend(run_names);
        }
        let text = match kept.last() {
            Some(last) if !last.text.ends_with([' ', '\t']) => &text,
            _ => text.trim_start_matches([' ', '\t']),
        };
        if !text.is_empty() {
            kept.push(Run { text: text.to_owned(), bold: run.bold, italic: run.italic });
        }
    }
    let Some((bold, italic)) = icon_style else { return row };
    if let Some(last) = kept.last_mut() {
        last.text.truncate(last.text.trim_end_matches([' ', '\t']).len());
    }
    if kept.iter().all(|run| run.text.trim().is_empty()) {
        return vec![Run { text: names.join(" "), bold, italic }];
    }
    kept
}

/// The rows of `source` between line breaks, upstream's `lineBreakRegex`
/// `/<\/?br\s*\/?>/gi`: `<br>`, `<br/>`, `<br />` and `</br>` in any case, with any
/// blanks before the closing `/` or `>`. The blanks around a break are dropped, as a
/// browser drops them at the ends of a line.
fn split_rows(source: &str) -> Vec<&str> {
    let mut rows = Vec::new();
    let mut start = 0;
    for (at, _) in source.match_indices('<') {
        if at < start {
            continue;
        }
        let Some(length) = source.get(at..).and_then(line_break_length) else { continue };
        rows.push(source.get(start..at).unwrap_or_default());
        start = at + length;
    }
    // A last break ends the row before it without starting another, as in HTML, unless
    // nothing else is written: a lone `<br>` is one empty row.
    let rest = source.get(start..).unwrap_or_default();
    if rows.is_empty() || !rest.trim().is_empty() {
        rows.push(rest);
    }
    trimmed_at_breaks(rows)
}

/// The length in bytes of the line break tag `text` starts with, if it starts with one
/// (see [`split_rows`]).
fn line_break_length(text: &str) -> Option<usize> {
    let rest = text.strip_prefix('<')?;
    let rest = rest.strip_prefix('/').unwrap_or(rest);
    rest.get(..2).filter(|name| name.eq_ignore_ascii_case("br"))?;
    let rest = rest.get(2..)?.trim_start();
    let rest = rest.strip_prefix('/').unwrap_or(rest);
    let rest = rest.strip_prefix('>')?;
    Some(text.len() - rest.len())
}

/// `text` with the source lines a quoted string spans joined by one space, the blanks
/// around each line break dropped, as upstream's HTML label collapses them.
fn joined_lines(text: &str) -> Cow<'_, str> {
    if !text.contains('\n') {
        return Cow::Borrowed(text);
    }
    Cow::Owned(trimmed_at_breaks(text.split('\n').collect()).join(" "))
}

/// `parts`, the pieces of a text cut at line breaks, without the blanks next to a break:
/// those at the end of every part but the last and at the start of every part but the
/// first.
fn trimmed_at_breaks(parts: Vec<&str>) -> Vec<&str> {
    let last = parts.len().saturating_sub(1);
    parts
        .into_iter()
        .enumerate()
        .map(|(index, part)| {
            let part = if index > 0 { part.trim_start() } else { part };
            if index < last { part.trim_end() } else { part }
        })
        .collect()
}

/// A piece of a markdown string's row: text, or a run of one emphasis character.
enum Piece {
    Text(String),
    Delimiter(Delimiter),
}

/// A run of `*` or `_`, as CommonMark's emphasis rules read it.
struct Delimiter {
    character: char,
    /// The characters not yet used as markers.
    count: usize,
    can_open: bool,
    can_close: bool,
}

/// The styled runs of one row of a markdown string: `**text**` (or `__text__`) is bold,
/// `*text*` and `_text_` are italic, they nest, and their markers are not shown. Markers
/// pair as CommonMark pairs them, simplified: a run of `*` or `_` may open emphasis
/// when a non-blank follows it and close emphasis when a non-blank precedes it, an `_`
/// only at a word's edge; a closing run pairs with the nearest opening run of the same
/// character before it, two markers at a time for bold when both have two, otherwise
/// one for italic. Markers left unpaired are shown as written. The text is normalised
/// by [`label_text`].
fn emphasis_runs(row: &str) -> Row {
    let mut pieces = delimiter_pieces(row);
    let depth = pair_delimiters(&mut pieces);
    let mut runs: Row = Vec::new();
    for (piece, &(bold, italic)) in pieces.iter().zip(&depth) {
        let text = match piece {
            Piece::Text(text) => label_text(text),
            Piece::Delimiter(delimiter) => {
                std::iter::repeat_n(delimiter.character, delimiter.count).collect()
            }
        };
        let (bold, italic) = (bold > 0, italic > 0);
        match runs.last_mut() {
            Some(last) if last.bold == bold && last.italic == italic => last.text.push_str(&text),
            _ if text.is_empty() => {}
            _ => runs.push(Run { text, bold, italic }),
        }
    }
    if runs.is_empty() {
        runs.push(Run::plain(String::new()));
    }
    runs
}

/// Pairs the delimiters of `pieces` as [`emphasis_runs`] describes, using up the markers
/// paired, and returns how many bold and how many italic pairs enclose each piece.
fn pair_delimiters(pieces: &mut [Piece]) -> Vec<(usize, usize)> {
    let mut depth = vec![(0, 0); pieces.len()];
    // The delimiters that may still open emphasis, in order.
    let mut openers: Vec<usize> = Vec::new();
    for closer in 0..pieces.len() {
        let Some(Piece::Delimiter(delimiter)) = pieces.get(closer) else { continue };
        let (character, can_open, can_close) =
            (delimiter.character, delimiter.can_open, delimiter.can_close);
        // A closing run pairs as long as it has markers and an opener is left.
        while can_close
            && let Some(at) = openers.iter().rposition(|&opener| {
                matches!(pieces.get(opener), Some(Piece::Delimiter(open)) if open.character == character)
            })
            && let Some(&opener) = openers.get(at)
        {
            let (open_count, close_count) =
                (available_count(pieces.get(opener)), available_count(pieces.get(closer)));
            let used = if open_count >= 2 && close_count >= 2 { 2 } else { 1 };
            for (bold, italic) in depth.get_mut(opener + 1..closer).unwrap_or_default() {
                if used == 2 {
                    *bold += 1;
                } else {
                    *italic += 1;
                }
            }
            for index in [opener, closer] {
                if let Some(Piece::Delimiter(delimiter)) = pieces.get_mut(index) {
                    delimiter.count -= used;
                }
            }
            // Openers after the one used lie inside the emphasis and stay unpaired.
            openers.truncate(at + 1);
            if open_count == used {
                openers.pop();
            }
            if close_count == used {
                break;
            }
        }
        if can_open && available_count(pieces.get(closer)) > 0 {
            openers.push(closer);
        }
    }
    depth
}

/// The markers a delimiter piece has left; 0 for text.
fn available_count(piece: Option<&Piece>) -> usize {
    match piece {
        Some(Piece::Delimiter(delimiter)) => delimiter.count,
        _ => 0,
    }
}

/// `row` cut into text and runs of `*` or `_`, each run with whether it may open or
/// close emphasis (see [`emphasis_runs`]).
fn delimiter_pieces(row: &str) -> Vec<Piece> {
    let mut pieces = Vec::new();
    let mut text = String::new();
    let mut chars = row.chars().peekable();
    let mut previous: Option<char> = None;
    while let Some(c) = chars.next() {
        if c != '*' && c != '_' {
            text.push(c);
            previous = Some(c);
            continue;
        }
        let mut count = 1;
        while chars.next_if_eq(&c).is_some() {
            count += 1;
        }
        let next = chars.peek().copied();
        let opens = next.is_some_and(|next| !next.is_whitespace());
        let closes = previous.is_some_and(|previous| !previous.is_whitespace());
        let (can_open, can_close) = if c == '_' {
            (
                opens && !previous.is_some_and(char::is_alphanumeric),
                closes && !next.is_some_and(char::is_alphanumeric),
            )
        } else {
            (opens, closes)
        };
        if !text.is_empty() {
            pieces.push(Piece::Text(std::mem::take(&mut text)));
        }
        pieces.push(Piece::Delimiter(Delimiter { character: c, count, can_open, can_close }));
        previous = Some(c);
    }
    if !text.is_empty() {
        pieces.push(Piece::Text(text));
    }
    pieces
}

/// Splits the double-quoted string `label` starts with, if any, into its content and the
/// text after its closing quote.
pub(super) fn leading_string(label: &str) -> Option<(&str, &str)> {
    label.strip_prefix('"')?.split_once('"')
}

/// `text` without the double quotes around it when the whole of it is one quoted
/// string, the `STR` token of Mermaid's
/// `packages/mermaid/src/diagrams/flowchart/parser/flow.jison`, which holds no quote
/// itself.
pub(super) fn unquoted(text: &str) -> &str {
    text.strip_circumfix('"', '"').filter(|inner| !inner.contains('"')).unwrap_or(text)
}
