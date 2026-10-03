//! Syntax highlighting of code blocks. The only module that names syntect types.

use std::sync::LazyLock;

use syntect::easy::HighlightLines;
use syntect::highlighting::{FontStyle, Style as SyntectStyle, Theme};
use syntect::parsing::{SyntaxReference, SyntaxSet};
use syntect::util::LinesWithEndings;
use two_face::theme::EmbeddedThemeName;

use crate::style::Style;
use crate::theme::Rgb;

const MAX_CODE_BLOCK_BYTES: usize = 512 * 1024;
const MAX_CODE_BLOCK_LINES: usize = 10_000;

static SYNTAX_SET: LazyLock<SyntaxSet> = LazyLock::new(two_face::syntax::extra_newlines);

static THEME: LazyLock<Theme> =
    LazyLock::new(|| two_face::theme::extra()[EmbeddedThemeName::SolarizedDark].clone());

/// Splits `code` into styled pieces whose texts concatenate back to `code`; line
/// endings stay inside the pieces. Returns `None` when the info string names no known
/// language, the block exceeds the size limits, or highlighting fails.
pub(crate) fn highlight<'a>(info: &str, code: &'a str) -> Option<Vec<(Style, &'a str)>> {
    // Resolve the syntax first so a block in an unknown language skips the body scan
    // in `is_within_highlight_limits`.
    let syntax = syntax_for_info(info)?;
    if !is_within_highlight_limits(code) {
        return None;
    }

    highlight_with_syntax(code, syntax).ok()
}

fn highlight_with_syntax<'a>(
    code: &'a str,
    syntax: &SyntaxReference,
) -> Result<Vec<(Style, &'a str)>, syntect::Error> {
    let mut highlighter = HighlightLines::new(syntax, &THEME);
    let mut pieces = Vec::new();
    for line in LinesWithEndings::from(code) {
        for (style, text) in highlighter.highlight_line(line, &SYNTAX_SET)? {
            if !text.is_empty() {
                pieces.push((convert_style(style), text));
            }
        }
    }
    Ok(pieces)
}

fn syntax_for_info(info: &str) -> Option<&'static SyntaxReference> {
    let token = language_token(info)?;
    SYNTAX_SET.find_syntax_by_token(token)
}

fn language_token(info: &str) -> Option<&str> {
    let token = info.trim_ascii().split_ascii_whitespace().next()?;
    let token = token.split_once(',').map_or(token, |(language, _flags)| language);
    if token.is_empty() { None } else { Some(token) }
}

fn is_within_highlight_limits(code: &str) -> bool {
    code.len() <= MAX_CODE_BLOCK_BYTES
        && LinesWithEndings::from(code).nth(MAX_CODE_BLOCK_LINES).is_none()
}

fn convert_style(style: SyntectStyle) -> Style {
    let SyntectStyle { foreground, font_style, .. } = style;
    Style {
        fg: Some(Rgb { r: foreground.r, g: foreground.g, b: foreground.b }),
        bold: font_style.contains(FontStyle::BOLD),
        italic: font_style.contains(FontStyle::ITALIC),
        underline: font_style.contains(FontStyle::UNDERLINE),
        ..Style::default()
    }
}

#[cfg(test)]
mod tests {
    use super::{highlight, syntax_for_info};

    fn syntax_name(info: &str) -> Option<&'static str> {
        syntax_for_info(info).map(|syntax| syntax.name.as_str())
    }

    #[test]
    fn selects_rust_from_rustdoc_style_info_strings() {
        assert_eq!(syntax_name("rust,no_run"), Some("Rust"));
    }

    #[test]
    fn selects_rust_from_info_strings_with_extra_words() {
        assert_eq!(syntax_name("rust linenums"), Some("Rust"));
    }

    #[test]
    fn rejects_empty_and_unsupported_language_tokens() {
        assert_eq!(syntax_name("   "), None);
        assert_eq!(syntax_name("definitely-not-a-language"), None);
    }

    #[test]
    fn rejects_code_blocks_over_the_line_guardrail() {
        let code = "x\n".repeat(10_001);

        assert_eq!(highlight("rust", &code), None);
    }
}
