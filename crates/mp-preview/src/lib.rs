//! Renders Markdown as styled terminal text.
//!
//! [`preview`] is the whole public surface: callers own reading the input so they can
//! attach their own path-bearing error context, and choose the width and [`ColorMode`]
//! through [`Options`].

mod ansi;
mod highlight;
mod layout;
mod markdown;
mod model;
mod style;
mod theme;

use std::io::{self, Write};

pub use ansi::ColorMode;

use layout::{LayoutOptions, layout};

/// How [`preview`] lays out and encodes its output.
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq)]
pub struct Options {
    /// Maximum display width in terminal columns; `None` disables wrapping.
    pub width: Option<usize>,
    /// Whether the output carries ANSI styling.
    pub color: ColorMode,
}

/// Renders `markdown` as a terminal preview into `out`.
///
/// Empty input writes nothing; any other output ends with exactly one newline.
/// Top-level blocks are separated by one blank line, and each is written to `out`
/// before the next is laid out.
///
/// # Errors
///
/// Returns the first error reported by `out`, leaving the blocks written before it in
/// place. Parsing never fails.
pub fn preview<W: Write>(markdown: &str, options: &Options, out: &mut W) -> io::Result<()> {
    let layout_options = LayoutOptions { width: options.width, color: options.color };
    for (index, block) in markdown::parse(markdown).iter().enumerate() {
        let mut encoded = if index == 0 { String::new() } else { String::from("\n") };
        encoded.push_str(&ansi::to_ansi(&layout(block, &layout_options), options.color));
        encoded.push('\n');
        out.write_all(encoded.as_bytes())?;
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    use std::io;

    use super::{Options, preview};

    #[test]
    fn preview_writes_nothing_for_empty_input() -> io::Result<()> {
        let mut output = Vec::new();

        preview("", &Options::default(), &mut output)?;

        assert!(output.is_empty(), "expected no bytes, got {output:?}");
        Ok(())
    }

    #[test]
    fn preview_separates_top_level_blocks_with_one_blank_line() -> io::Result<()> {
        assert_eq!(plain("# Title\n\n\n\nHello\n")?, "Title\n\nHello\n");
        Ok(())
    }

    #[test]
    fn preview_spacing_ignores_source_blank_lines() -> io::Result<()> {
        assert_eq!(plain("# H\ntext")?, plain("# H\n\ntext")?);
        assert_eq!(plain("```\nx\n```\nb\n")?, "```\nx\n```\n\nb\n");
        Ok(())
    }

    #[test]
    fn preview_ends_non_empty_output_with_exactly_one_newline() -> io::Result<()> {
        assert_eq!(plain("Hello")?, "Hello\n");
        assert_eq!(plain("Hello\n\n\n")?, "Hello\n");
        Ok(())
    }

    #[test]
    fn preview_visualizes_control_characters_from_references() -> io::Result<()> {
        assert_eq!(plain("&#27;[2J&#27;[HHello\n")?, "\u{241b}[2J\u{241b}[HHello\n");
        Ok(())
    }

    #[test]
    fn preview_keeps_inline_html_spanning_lines_on_one_quoted_line() -> io::Result<()> {
        assert_eq!(
            plain("> a <span\nclass=\"x\">b</span> c\n")?,
            "\u{2502} a <span class=\"x\">b</span> c\n"
        );
        Ok(())
    }

    #[test]
    fn preview_drops_trailing_blank_lines_of_an_unclosed_html_block() -> io::Result<()> {
        assert_eq!(plain("<!--\nfoo\n\n\n")?, "<!--\nfoo\n");
        Ok(())
    }

    #[test]
    fn preview_drops_whitespace_only_trailing_lines_of_an_unclosed_html_block() -> io::Result<()> {
        assert_eq!(plain("<!--\nfoo\n  \n")?, "<!--\nfoo\n");
        Ok(())
    }

    #[test]
    fn preview_visualizes_control_characters_in_code_block_info_strings() -> io::Result<()> {
        assert_eq!(plain("```&#27;[31mrust\nx\n```\n")?, "```\u{241b}[31mrust\nx\n```\n");
        Ok(())
    }

    #[test]
    fn preview_reports_writer_failures() {
        let result = preview("Hello\n", &Options::default(), &mut FailingWriter);

        assert!(result.is_err());
    }

    #[test]
    fn preview_writes_earlier_blocks_before_later_ones_fail() {
        let mut writer = FirstWriteOnly { bytes: Vec::new(), writes: 0 };

        let result = preview("a\n\nb\n", &Options::default(), &mut writer);

        assert!(result.is_err());
        assert_eq!(writer.bytes, b"a\n");
    }

    /// Accepts its first `write` call in full and fails every later one.
    struct FirstWriteOnly {
        bytes: Vec<u8>,
        writes: usize,
    }

    impl io::Write for FirstWriteOnly {
        fn write(&mut self, buffer: &[u8]) -> io::Result<usize> {
            self.writes += 1;
            if self.writes > 1 {
                return Err(io::Error::other("closed writer"));
            }
            self.bytes.extend_from_slice(buffer);
            Ok(buffer.len())
        }

        fn flush(&mut self) -> io::Result<()> {
            Ok(())
        }
    }

    struct FailingWriter;

    impl io::Write for FailingWriter {
        fn write(&mut self, _buffer: &[u8]) -> io::Result<usize> {
            Err(io::Error::other("closed writer"))
        }

        fn flush(&mut self) -> io::Result<()> {
            Ok(())
        }
    }

    fn plain(markdown: &str) -> io::Result<String> {
        let mut output = Vec::new();
        preview(markdown, &Options::default(), &mut output)?;
        String::from_utf8(output).map_err(|error| io::Error::new(io::ErrorKind::InvalidData, error))
    }
}
