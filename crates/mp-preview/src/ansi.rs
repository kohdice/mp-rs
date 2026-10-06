//! The only encoder from styled lines to terminal output.

use std::fmt::Write as _;

use crate::style::{Line, Style};
use crate::theme::Rgb;

/// Whether output carries ANSI escape sequences.
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq)]
pub enum ColorMode {
    /// 24-bit color and SGR attributes; code blocks are syntax highlighted.
    Ansi,
    /// Text only, with no escape sequences.
    #[default]
    Plain,
}

pub(crate) fn to_ansi(lines: &[Line], mode: ColorMode) -> String {
    let mut encoded = String::new();
    for (index, line) in lines.iter().enumerate() {
        if index > 0 {
            encoded.push('\n');
        }
        let mut current = Style::default();
        for span in line {
            if mode == ColorMode::Ansi && span.style != current {
                encoded.push_str("\x1b[");
                if current != Style::default() {
                    encoded.push('0');
                    if span.style != Style::default() {
                        encoded.push(';');
                    }
                }
                push_sgr_params(&mut encoded, span.style);
                encoded.push('m');
                current = span.style;
            }
            encoded.push_str(&span.text);
        }
        if current != Style::default() {
            encoded.push_str("\x1b[0m");
        }
    }
    encoded
}

fn push_sgr_params(encoded: &mut String, style: Style) {
    let flags = [
        (style.bold, "1"),
        (style.dim, "2"),
        (style.italic, "3"),
        (style.underline, "4"),
        (style.strikethrough, "9"),
    ];
    let mut separator = "";
    for (enabled, code) in flags {
        if enabled {
            encoded.push_str(separator);
            encoded.push_str(code);
            separator = ";";
        }
    }
    if let Some(Rgb { r, g, b }) = style.fg {
        // Formatting into a `String` cannot fail.
        let _ = write!(encoded, "{separator}38;2;{r};{g};{b}");
        separator = ";";
    }
    if let Some(Rgb { r, g, b }) = style.bg {
        // Formatting into a `String` cannot fail.
        let _ = write!(encoded, "{separator}48;2;{r};{g};{b}");
    }
}

#[cfg(test)]
mod tests {
    use super::{ColorMode, to_ansi};
    use crate::style::{Span, Style};
    use crate::theme::Rgb;

    fn span(text: &str, style: Style) -> Span {
        Span { text: text.to_owned(), style }
    }

    #[test]
    fn to_ansi_joins_lines_with_newlines_without_escapes_in_plain_mode() {
        let bold_colored =
            Style { fg: Some(Rgb { r: 1, g: 2, b: 3 }), bold: true, ..Style::default() };
        let lines = vec![
            vec![span("a", Style::default()), span(" b", bold_colored)],
            vec![span("c", Style::default())],
        ];

        let encoded = to_ansi(&lines, ColorMode::Plain);

        assert_eq!(encoded, "a b\nc");
        assert!(!encoded.contains('\x1b'));
    }

    #[test]
    fn to_ansi_returns_an_empty_string_for_no_lines() {
        assert_eq!(to_ansi(&[], ColorMode::Plain), "");
        assert_eq!(to_ansi(&[], ColorMode::Ansi), "");
    }

    #[test]
    fn to_ansi_emits_nothing_for_default_style_spans() {
        let lines = vec![vec![span("plain", Style::default())]];

        assert_eq!(to_ansi(&lines, ColorMode::Ansi), "plain");
    }

    #[test]
    fn to_ansi_encodes_all_attributes_in_order_and_resets_at_line_end() {
        let every_attribute = Style {
            fg: Some(Rgb { r: 1, g: 2, b: 3 }),
            bg: None,
            bold: true,
            dim: true,
            italic: true,
            underline: true,
            strikethrough: true,
        };
        let lines = vec![vec![span("X", every_attribute)]];

        assert_eq!(to_ansi(&lines, ColorMode::Ansi), "\x1b[1;2;3;4;9;38;2;1;2;3mX\x1b[0m");
    }

    #[test]
    fn to_ansi_emits_one_sequence_for_adjacent_spans_with_equal_style() {
        let bold = Style { bold: true, ..Style::default() };
        let lines = vec![vec![span("a", bold), span("b", bold)]];

        assert_eq!(to_ansi(&lines, ColorMode::Ansi), "\x1b[1mab\x1b[0m");
    }

    #[test]
    fn to_ansi_switches_between_styles_with_a_single_sequence() {
        let bold = Style { bold: true, ..Style::default() };
        let italic = Style { italic: true, ..Style::default() };
        let lines = vec![vec![span("a", bold), span("b", italic), span("c", Style::default())]];

        assert_eq!(to_ansi(&lines, ColorMode::Ansi), "\x1b[1ma\x1b[0;3mb\x1b[0mc");
    }

    #[test]
    fn to_ansi_does_not_carry_style_across_lines() {
        let bold = Style { bold: true, ..Style::default() };
        let lines = vec![vec![span("a", bold)], vec![span("b", bold)]];

        assert_eq!(to_ansi(&lines, ColorMode::Ansi), "\x1b[1ma\x1b[0m\n\x1b[1mb\x1b[0m");
    }

    #[test]
    fn to_ansi_encodes_a_foreground_only_style_without_a_leading_separator() {
        let colored = Style { fg: Some(Rgb { r: 1, g: 2, b: 3 }), ..Style::default() };
        let lines = vec![vec![span("X", colored)]];

        assert_eq!(to_ansi(&lines, ColorMode::Ansi), "\x1b[38;2;1;2;3mX\x1b[0m");
    }

    #[test]
    fn to_ansi_encodes_a_background_in_ansi_mode_only() {
        let painted = Style { bg: Some(Rgb { r: 1, g: 2, b: 3 }), ..Style::default() };
        let lines = vec![vec![span("X", painted)]];

        assert_eq!(to_ansi(&lines, ColorMode::Ansi), "\x1b[48;2;1;2;3mX\x1b[0m");
        assert_eq!(to_ansi(&lines, ColorMode::Plain), "X");
    }

    #[test]
    fn to_ansi_encodes_foreground_then_background() {
        let both = Style {
            fg: Some(Rgb { r: 1, g: 2, b: 3 }),
            bg: Some(Rgb { r: 4, g: 5, b: 6 }),
            ..Style::default()
        };
        let lines = vec![vec![span("X", both)]];

        assert_eq!(to_ansi(&lines, ColorMode::Ansi), "\x1b[38;2;1;2;3;48;2;4;5;6mX\x1b[0m");
    }

    #[test]
    fn to_ansi_keeps_empty_lines_between_lines() {
        let lines =
            vec![vec![span("a", Style::default())], vec![], vec![span("b", Style::default())]];

        assert_eq!(to_ansi(&lines, ColorMode::Plain), "a\n\nb");
    }
}
