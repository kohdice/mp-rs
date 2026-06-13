use std::sync::LazyLock;

#[cfg(test)]
use std::cell::Cell;

use syntect::easy::HighlightLines;
use syntect::highlighting::{FontStyle, Style as SyntectStyle, Theme};
use syntect::parsing::{SyntaxReference, SyntaxSet};
use syntect::util::LinesWithEndings;
use two_face::theme::EmbeddedThemeName;

use crate::renderer::CodeTheme;
use crate::style::TextStyle;
use crate::theme::Rgb;

const MAX_CODE_BLOCK_BYTES: usize = 512 * 1024;
const MAX_CODE_BLOCK_LINES: usize = 10_000;

// Syntax definitions are theme-independent, so a single global set serves every theme.
static SYNTAX_SET: LazyLock<SyntaxSet> = LazyLock::new(two_face::syntax::extra_newlines);

#[cfg(test)]
thread_local! {
    static FORCE_NEXT_HIGHLIGHT_ERROR: Cell<bool> = const { Cell::new(false) };
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) struct StyledRange<'a> {
    pub(crate) style: TextStyle,
    pub(crate) text: &'a str,
}

pub(crate) fn highlighted_ranges<'a>(
    info: Option<&str>,
    source: &'a str,
    code_theme: CodeTheme,
) -> Option<Vec<StyledRange<'a>>> {
    // Resolve the syntax first: an unhighlightable block then skips the up-to-512-KiB
    // body scan in `is_within_highlight_limits`.
    let syntax = syntax_for_info(info?)?;
    if !is_within_highlight_limits(source) {
        return None;
    }

    highlighted_ranges_for_syntax(source, syntax, code_theme).ok()
}

fn highlighted_ranges_for_syntax<'a>(
    source: &'a str,
    syntax: &SyntaxReference,
    code_theme: CodeTheme,
) -> Result<Vec<StyledRange<'a>>, syntect::Error> {
    #[cfg(test)]
    if take_forced_highlight_error_for_test() {
        return Err(std::io::Error::other("forced syntax highlighting error").into());
    }

    let mut highlighter = HighlightLines::new(syntax, syntax_theme(code_theme));
    collect_highlighted_ranges(source, |line| highlighter.highlight_line(line, &SYNTAX_SET))
}

fn collect_highlighted_ranges<'a, E>(
    source: &'a str,
    mut highlight_line: impl FnMut(&'a str) -> Result<Vec<(SyntectStyle, &'a str)>, E>,
) -> Result<Vec<StyledRange<'a>>, E> {
    let mut ranges = Vec::new();
    for line in LinesWithEndings::from(source) {
        for (style, text) in highlight_line(line)? {
            if !text.is_empty() {
                ranges.push(StyledRange { style: text_style(style), text });
            }
        }
    }
    Ok(ranges)
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

fn is_within_highlight_limits(source: &str) -> bool {
    source.len() <= MAX_CODE_BLOCK_BYTES && !exceeds_line_limit(source)
}

fn exceeds_line_limit(source: &str) -> bool {
    let mut lines = 0;
    for _line in LinesWithEndings::from(source) {
        lines += 1;
        if lines > MAX_CODE_BLOCK_LINES {
            return true;
        }
    }
    false
}

fn text_style(style: SyntectStyle) -> TextStyle {
    let foreground = style.foreground;
    let mut text_style =
        TextStyle::default().fg(Rgb { r: foreground.r, g: foreground.g, b: foreground.b });
    let font_style = style.font_style;
    if font_style.contains(FontStyle::BOLD) {
        text_style = text_style.bold();
    }
    if font_style.contains(FontStyle::ITALIC) {
        text_style = text_style.italic();
    }
    if font_style.contains(FontStyle::UNDERLINE) {
        text_style = text_style.underline();
    }
    text_style
}

fn syntax_theme(code_theme: CodeTheme) -> &'static Theme {
    // Each theme is loaded once and cached for the process; the syntax set is shared.
    match code_theme {
        CodeTheme::SolarizedDark => {
            static THEME: LazyLock<Theme> =
                LazyLock::new(|| embedded_theme(EmbeddedThemeName::SolarizedDark));
            &THEME
        }
        CodeTheme::SolarizedLight => {
            static THEME: LazyLock<Theme> =
                LazyLock::new(|| embedded_theme(EmbeddedThemeName::SolarizedLight));
            &THEME
        }
    }
}

fn embedded_theme(name: EmbeddedThemeName) -> Theme {
    two_face::theme::extra()[name].clone()
}

#[cfg(test)]
pub(crate) fn force_next_highlight_error_for_test() {
    FORCE_NEXT_HIGHLIGHT_ERROR.with(|force| force.set(true));
}

#[cfg(test)]
fn take_forced_highlight_error_for_test() -> bool {
    FORCE_NEXT_HIGHLIGHT_ERROR.with(|force| force.replace(false))
}

#[cfg(test)]
pub(crate) fn syntax_name_for_info_for_test(info: &str) -> Option<&'static str> {
    syntax_for_info(info).map(|syntax| syntax.name.as_str())
}

#[cfg(test)]
pub(crate) fn syntax_theme_name_for_test(code_theme: CodeTheme) -> Option<&'static str> {
    syntax_theme(code_theme).name.as_deref()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn resolves_each_code_theme_to_its_bundled_solarized_variant() {
        assert_eq!(syntax_theme_name_for_test(CodeTheme::SolarizedDark), Some("Solarized (dark)"));
        assert_eq!(
            syntax_theme_name_for_test(CodeTheme::SolarizedLight),
            Some("Solarized (light)"),
        );
    }

    #[test]
    fn selects_rust_from_rustdoc_style_info_strings() {
        assert_eq!(syntax_name_for_info_for_test("rust,no_run"), Some("Rust"));
    }

    #[test]
    fn selects_rust_from_info_strings_with_extra_words() {
        assert_eq!(syntax_name_for_info_for_test("rust linenums"), Some("Rust"));
    }

    #[test]
    fn rejects_empty_and_unsupported_language_tokens() {
        assert_eq!(syntax_name_for_info_for_test("   "), None);
        assert_eq!(syntax_name_for_info_for_test("definitely-not-a-language"), None);
    }

    #[test]
    fn rejects_code_blocks_over_the_line_guardrail() {
        let source = "x\n".repeat(10_001);

        assert_eq!(highlighted_ranges(Some("rust"), &source, CodeTheme::SolarizedDark), None);
    }

    #[test]
    fn returns_no_highlighted_ranges_when_highlighting_fails() {
        force_next_highlight_error_for_test();

        assert_eq!(
            highlighted_ranges(Some("rust"), "fn main() {}\n", CodeTheme::SolarizedDark),
            None,
        );
    }
}
