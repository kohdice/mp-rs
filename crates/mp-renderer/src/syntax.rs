use std::sync::LazyLock;

#[cfg(test)]
use std::cell::Cell;
#[cfg(test)]
use std::sync::atomic::{AtomicUsize, Ordering};

use syntect::easy::HighlightLines;
use syntect::highlighting::{FontStyle, Style as SyntectStyle, Theme};
use syntect::parsing::{SyntaxReference, SyntaxSet};
use syntect::util::LinesWithEndings;
use two_face::theme::EmbeddedThemeName;

use crate::style::TextStyle;
use crate::theme::Rgb;

const MAX_CODE_BLOCK_BYTES: usize = 512 * 1024;
const MAX_CODE_BLOCK_LINES: usize = 10_000;

static SYNTAX_SET: LazyLock<SyntaxSet> = LazyLock::new(load_syntax_set);
static SYNTAX_THEME: LazyLock<Theme> = LazyLock::new(load_syntax_theme);

#[cfg(test)]
static SYNTAX_SET_INITIALIZATIONS: AtomicUsize = AtomicUsize::new(0);

#[cfg(test)]
static SYNTAX_THEME_INITIALIZATIONS: AtomicUsize = AtomicUsize::new(0);

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
) -> Option<Vec<StyledRange<'a>>> {
    if !is_within_highlight_limits(source) {
        return None;
    }

    let syntax = syntax_for_info(info?)?;
    highlighted_ranges_for_syntax(source, syntax).ok()
}

fn highlighted_ranges_for_syntax<'a>(
    source: &'a str,
    syntax: &SyntaxReference,
) -> Result<Vec<StyledRange<'a>>, syntect::Error> {
    #[cfg(test)]
    if take_forced_highlight_error_for_test() {
        return Err(std::io::Error::other("forced syntax highlighting error").into());
    }

    let syntax_set = syntax_set();
    let mut highlighter = HighlightLines::new(syntax, syntax_theme());
    collect_highlighted_ranges(source, |line| highlighter.highlight_line(line, syntax_set))
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
    syntax_set().find_syntax_by_token(token)
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

fn syntax_set() -> &'static SyntaxSet {
    &SYNTAX_SET
}

fn syntax_theme() -> &'static Theme {
    &SYNTAX_THEME
}

fn load_syntax_set() -> SyntaxSet {
    #[cfg(test)]
    SYNTAX_SET_INITIALIZATIONS.fetch_add(1, Ordering::Relaxed);

    two_face::syntax::extra_newlines()
}

fn load_syntax_theme() -> Theme {
    #[cfg(test)]
    SYNTAX_THEME_INITIALIZATIONS.fetch_add(1, Ordering::Relaxed);

    two_face::theme::extra()[EmbeddedThemeName::SolarizedDark].clone()
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
pub(crate) fn syntax_theme_name_for_test() -> Option<&'static str> {
    syntax_theme().name.as_deref()
}

#[cfg(test)]
pub(crate) fn syntax_set_address_for_test() -> usize {
    std::ptr::from_ref(syntax_set()).addr()
}

#[cfg(test)]
pub(crate) fn syntax_theme_address_for_test() -> usize {
    std::ptr::from_ref(syntax_theme()).addr()
}

#[cfg(test)]
pub(crate) fn syntax_set_initializations_for_test() -> usize {
    SYNTAX_SET_INITIALIZATIONS.load(Ordering::Relaxed)
}

#[cfg(test)]
pub(crate) fn syntax_theme_initializations_for_test() -> usize {
    SYNTAX_THEME_INITIALIZATIONS.load(Ordering::Relaxed)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn uses_the_bundled_solarized_dark_syntax_theme() {
        assert_eq!(syntax_theme_name_for_test(), Some("Solarized (dark)"));
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

        assert_eq!(highlighted_ranges(Some("rust"), &source), None);
    }

    #[test]
    fn returns_no_highlighted_ranges_when_highlighting_fails() {
        force_next_highlight_error_for_test();

        assert_eq!(highlighted_ranges(Some("rust"), "fn main() {}\n"), None);
    }

    #[test]
    fn reuses_syntax_and_theme_assets_through_lazy_singletons() {
        let syntax_set_a = syntax_set_address_for_test();
        let syntax_set_b = syntax_set_address_for_test();
        let theme_a = syntax_theme_address_for_test();
        let theme_b = syntax_theme_address_for_test();

        assert_ne!(syntax_set_a, 0);
        assert_eq!(syntax_set_a, syntax_set_b);
        assert_ne!(theme_a, 0);
        assert_eq!(theme_a, theme_b);
        assert_eq!(syntax_set_initializations_for_test(), 1);
        assert_eq!(syntax_theme_initializations_for_test(), 1);
    }
}
