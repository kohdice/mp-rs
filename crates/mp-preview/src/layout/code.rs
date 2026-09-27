//! Code and HTML blocks: verbatim lines with tab stops, never wrapped.

use unicode_width::UnicodeWidthStr;

use crate::ansi::ColorMode;
use crate::highlight::highlight;
use crate::style::{Line, Span, Style};
use crate::theme::solarized::DARK_PALETTE;

const FENCE: &str = "```";
const TAB_STOP: usize = 4;

/// Highlights the body only in `Ansi` mode; otherwise, or when highlighting is
/// unavailable, the body is in the inline code color.
pub(super) fn lay_out_code_block(info: &str, code: &str, color: ColorMode) -> Vec<Line> {
    let fence_style = Style { fg: Some(DARK_PALETTE.code_fence), dim: true, ..Style::default() };
    let body_style = Style { fg: Some(DARK_PALETTE.inline_code), ..Style::default() };
    let mut lines = vec![vec![Span { text: format!("{FENCE}{info}"), style: fence_style }]];
    let highlighted = match color {
        ColorMode::Ansi => highlight(info, code),
        ColorMode::Plain => None,
    };
    lines.extend(match highlighted {
        Some(pieces) => body_lines(pieces),
        None => body_lines([(body_style, code)]),
    });
    lines.push(vec![Span { text: FENCE.to_owned(), style: fence_style }]);
    lines
}

pub(super) fn lay_out_html(html: &str) -> Vec<Line> {
    // An HTML block left unclosed at the end of its container (e.g. `<!--`) keeps the
    // trailing blank lines, whitespace-only ones included, in comrak's literal.
    body_lines([(Style::default(), html.trim_end())])
}

/// Splits styled pieces into lines at `\n`; a final `\n` ends the last line rather than
/// starting an empty one. Tab stops are counted across pieces from each line start.
fn body_lines<'a>(pieces: impl IntoIterator<Item = (Style, &'a str)>) -> Vec<Line> {
    let mut lines = Vec::new();
    let mut line = Line::new();
    let mut column = 0;
    for (style, text) in pieces {
        for (index, segment) in text.split('\n').enumerate() {
            if index > 0 {
                lines.push(std::mem::take(&mut line));
                column = 0;
            }
            if !segment.is_empty() {
                push_expanded(&mut line, &mut column, segment, style);
            }
        }
    }
    if !line.is_empty() {
        lines.push(line);
    }
    lines
}

/// Appends `text` in `style`, replacing each tab with spaces up to the next multiple of
/// `TAB_STOP` columns. `column` is the line's width so far and is advanced past `text`.
fn push_expanded(line: &mut Line, column: &mut usize, text: &str, style: Style) {
    if line.last().is_none_or(|last| last.style != style) {
        line.push(Span { text: String::with_capacity(text.len()), style });
    }
    let Some(span) = line.last_mut() else {
        return;
    };
    for (index, part) in text.split('\t').enumerate() {
        if index > 0 {
            let spaces = TAB_STOP - *column % TAB_STOP;
            span.text.extend(std::iter::repeat_n(' ', spaces));
            *column += spaces;
        }
        span.text.push_str(part);
        *column += part.width();
    }
}
