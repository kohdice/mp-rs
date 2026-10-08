//! Syntax highlighting of code blocks. The only module that names syntect types.

use std::sync::LazyLock;

use syntect::easy::HighlightLines;
use syntect::highlighting::{FontStyle, Style as SyntectStyle, Theme};
use syntect::parsing::{SyntaxReference, SyntaxSet};
use syntect::util::LinesWithEndings;
use two_face::theme::EmbeddedThemeName;

use crate::style::Style;
use crate::theme::Rgb;

// Highlighting is skipped above either limit: syntect's cost grows with the body, and a
// pasted log or data dump should not stall the preview for a few colors.
/// The largest code block, in bytes, that [`highlight`] colors.
const MAX_CODE_BLOCK_BYTES: usize = 512 * 1024;
/// The most lines a code block that [`highlight`] colors may have.
const MAX_CODE_BLOCK_LINES: usize = 10_000;

static SYNTAX_SET: LazyLock<SyntaxSet> = LazyLock::new(two_face::syntax::extra_newlines);

static THEME: LazyLock<Theme> =
    LazyLock::new(|| two_face::theme::extra()[EmbeddedThemeName::SolarizedDark].clone());

/// Splits `code` into styled pieces whose texts concatenate back to `code`; line
/// endings stay inside the pieces. Returns `None` when the info string names no known
/// language, the block exceeds [`MAX_CODE_BLOCK_BYTES`] or [`MAX_CODE_BLOCK_LINES`], or
/// highlighting fails.
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

/// The first whitespace-separated word of a fence info string, without a `,flags`
/// suffix; `None` when there is no word.
pub(crate) fn language_token(info: &str) -> Option<&str> {
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
    use super::{highlight, language_token};

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
    fn rejects_empty_and_unsupported_language_tokens() {
        assert_eq!(highlight("   ", "x"), None);
        assert_eq!(highlight("definitely-not-a-language", "x"), None);
    }

    #[test]
    fn highlight_pieces_concatenate_to_the_code_with_line_endings_inside() {
        let code = "fn a() {}\n\nlet x = 1;\n";
        let pieces = highlight("rust", code)
            .unwrap_or_else(|| panic!("rust code within the guardrails should be highlighted"));
        let texts: Vec<&str> = pieces.iter().map(|&(_, text)| text).collect();

        assert_eq!(texts.concat(), code);
        assert!(texts.iter().all(|text| !text.is_empty()), "empty piece in {texts:?}");
    }

    #[test]
    fn line_guardrail_rejects_only_above_ten_thousand_lines() {
        let most_lines = "x\n".repeat(10_000);
        let one_line_too_many = "x\n".repeat(10_001);

        assert!(highlight("rust", &most_lines).is_some());
        assert_eq!(highlight("rust", &one_line_too_many), None);
    }

    #[test]
    fn byte_guardrail_rejects_only_above_512_kib() {
        // 8,192 lines of 64 bytes stay under the line guardrail, and plain text keeps
        // highlighting the accepted block fast in debug builds.
        let largest = format!("{}\n", "x".repeat(63)).repeat(8_192);
        let one_byte_too_many = format!("{largest}x");
        assert_eq!(largest.len(), 512 * 1024);

        assert!(highlight("txt", &largest).is_some());
        assert_eq!(highlight("txt", &one_byte_too_many), None);
    }
}
