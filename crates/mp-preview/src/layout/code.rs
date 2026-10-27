//! Code and HTML blocks: verbatim lines with tab stops, never wrapped.

use unicode_width::UnicodeWidthStr;

use crate::diagram::{Failure, render_mermaid};
use crate::highlight::highlight;
use crate::style::{ColorMode, Line, Span, Style};
use crate::theme::solarized::DARK_PALETTE;

const FENCE: &str = "```";
const TAB_STOP: usize = 4;

/// Lays out a fenced code block verbatim: the opening fence with its info string, the
/// body with tabs expanded, and the closing fence. Code is never wrapped. The body is
/// highlighted only in `Ansi` mode; otherwise, or when highlighting is unavailable, it
/// is in the inline code color.
///
/// A `mermaid` block that can be drawn within `width` columns is replaced by its
/// drawing, without fences. One with a syntax error keeps the code block and gets a
/// `mermaid: …` reason line above it; one that is valid but cannot be drawn keeps the
/// code block alone, since there is nothing for the author to fix. Upstream shows an
/// error image in place of the diagram, which a terminal cannot; the source with one
/// line of text is the closest it can show, in wording of this crate's own.
pub(super) fn lay_out_code_block(
    info: &str,
    code: &str,
    width: Option<usize>,
    color: ColorMode,
) -> Vec<Line> {
    let fence_style = Style { fg: Some(DARK_PALETTE.code_fence), dim: true, ..Style::default() };
    let body_style = Style { fg: Some(DARK_PALETTE.inline_code), ..Style::default() };
    let mut lines = Vec::new();
    let language = language_token(info);
    if language == Some("mermaid") {
        match render_mermaid(code, width) {
            Ok(drawing) => return drawing,
            Err(Failure::Syntax(error)) => {
                lines.push(vec![Span { text: format!("mermaid: {error}"), style: fence_style }]);
            }
            Err(Failure::Unsupported) => {}
        }
    }
    lines.push(vec![Span { text: format!("{FENCE}{info}"), style: fence_style }]);
    let highlighted = match color {
        ColorMode::Ansi => language.and_then(|language| highlight(language, code)),
        ColorMode::Plain => None,
    };
    lines.extend(match highlighted {
        Some(pieces) => body_lines(pieces),
        None => body_lines([(body_style, code)]),
    });
    lines.push(vec![Span { text: FENCE.to_owned(), style: fence_style }]);
    lines
}

/// The first whitespace-separated word of a fence info string, without a `,flags`
/// suffix; `None` when there is no word.
fn language_token(info: &str) -> Option<&str> {
    let token = info.trim_ascii().split_ascii_whitespace().next()?;
    let token = token.split_once(',').map_or(token, |(language, _flags)| language);
    (!token.is_empty()).then_some(token)
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
                column = push_expanded(&mut line, column, segment, style);
            }
        }
    }
    if !line.is_empty() {
        lines.push(line);
    }
    lines
}

/// Appends `text` in `style`, replacing each tab with spaces up to the next multiple of
/// `TAB_STOP` columns. `column` is the line's width so far; returns the width after
/// `text`.
fn push_expanded(line: &mut Line, mut column: usize, text: &str, style: Style) -> usize {
    if line.last().is_none_or(|last| last.style != style) {
        line.push(Span { text: String::with_capacity(text.len()), style });
    }
    let Some(span) = line.last_mut() else {
        return column;
    };
    for (index, part) in text.split('\t').enumerate() {
        if index > 0 {
            let spaces = TAB_STOP - column % TAB_STOP;
            span.text.extend(std::iter::repeat_n(' ', spaces));
            column += spaces;
        }
        span.text.push_str(part);
        column += part.width();
    }
    column
}

#[cfg(test)]
mod tests {
    use super::{body_lines, language_token};
    use crate::style::Style;

    #[test]
    fn language_token_drops_rustdoc_style_flags() {
        assert_eq!(language_token("rust,no_run"), Some("rust"));
    }

    #[test]
    fn language_token_keeps_only_the_first_word() {
        assert_eq!(language_token("rust linenums"), Some("rust"));
    }

    #[test]
    fn language_token_is_none_without_a_language_word() {
        assert_eq!(language_token("  "), None);
        assert_eq!(language_token(",flags"), None);
    }

    #[test]
    fn body_lines_expands_a_tab_that_spans_two_pieces() {
        let style_a = Style { bold: true, ..Style::default() };
        let style_b = Style { italic: true, ..Style::default() };

        let lines = body_lines([(style_a, "let"), (style_b, " x\t= 1;\n")]);

        let [line] = lines.as_slice() else {
            panic!("expected one line, got {lines:?}");
        };
        let text: String = line.iter().map(|span| span.text.as_str()).collect();
        assert_eq!(text, "let x   = 1;");
    }
}
