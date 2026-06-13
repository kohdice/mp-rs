//! The preview use case: composes parsing and rendering.
//!
//! This is the only crate the `mp` binary depends on directly. [`preview`] renders
//! Markdown text to a writer; callers own the file-system read so they can attach their
//! own path-bearing error context.

use std::fmt;
use std::io::{self, Write};

pub use mp_parser::ParseError;
pub use mp_renderer::{CodeTheme, ColorMode, HEADING_LEVEL_COUNT, Palette, RenderOptions, Rgb};

/// Failure modes of the preview use case.
#[derive(Debug)]
pub enum PreviewError {
    /// The Markdown event stream was structurally inconsistent.
    Parse(mp_parser::ParseError),
    /// The writer reported an I/O error while rendering.
    Write(io::Error),
}

impl fmt::Display for PreviewError {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::Parse(_) => formatter.write_str("unable to parse the Markdown document"),
            Self::Write(_) => formatter.write_str("unable to write the rendered preview"),
        }
    }
}

impl std::error::Error for PreviewError {
    fn source(&self) -> Option<&(dyn std::error::Error + 'static)> {
        match self {
            Self::Write(source) => Some(source),
            Self::Parse(source) => Some(source),
        }
    }
}

/// Renders Markdown text as a terminal preview.
///
/// This is the pure core of the use case: it performs no file-system access, so callers
/// own the input text and the writer.
///
/// # Errors
///
/// Returns [`PreviewError::Parse`] if the Markdown event stream is structurally
/// inconsistent, or [`PreviewError::Write`] if the writer reports an I/O error.
pub fn preview<W>(
    markdown: &str,
    options: RenderOptions,
    writer: &mut W,
) -> Result<(), PreviewError>
where
    W: Write,
{
    let renderer = mp_renderer::Renderer::new(options);
    let blocks = mp_parser::blocks(markdown).map(|result| result.map_err(PreviewError::Parse));
    render_stream(&renderer, blocks, writer)
}

/// Renders a stream of already-mapped blocks, terminating the output with a trailing
/// newline.
///
/// Errors yielded by the iterator are propagated as-is; the parser→`PreviewError`
/// mapping is the caller's responsibility, which lets tests drive the error path with
/// constructible [`PreviewError`] variants.
fn render_stream<'a, W, I>(
    renderer: &mp_renderer::Renderer,
    blocks: I,
    writer: &mut W,
) -> Result<(), PreviewError>
where
    W: Write,
    I: Iterator<Item = Result<mp_ast::Block<'a>, PreviewError>>,
{
    let mut state = mp_renderer::RenderState::default();
    for block in blocks {
        let block = match block {
            Ok(block) => block,
            Err(error) => {
                // Terminate any partial output so the trailing-newline invariant holds on the
                // error path too. The stream error is the root cause and outranks a secondary
                // write failure from `finish`, so its result is deliberately discarded.
                let _ = renderer.finish(writer, &state);
                return Err(error);
            }
        };
        renderer.render_block(writer, &block, &mut state).map_err(PreviewError::Write)?;
    }
    renderer.finish(writer, &state).map_err(PreviewError::Write)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn reexports_let_callers_build_a_palette_without_reaching_into_mp_renderer() {
        let color = Rgb { r: 1, g: 2, b: 3 };
        let palette =
            Palette { heading_colors: [color; HEADING_LEVEL_COUNT], ..Palette::default() };

        assert!(palette.heading_colors.iter().all(|&value| value == color));
    }

    #[test]
    fn preview_renders_markdown_text_to_the_writer() -> Result<(), PreviewError> {
        let mut output = Vec::new();
        preview("# Title\n\nHello\n", RenderOptions::default(), &mut output)?;

        let rendered = String::from_utf8(output).map_err(|error| {
            PreviewError::Write(std::io::Error::new(std::io::ErrorKind::InvalidData, error))
        })?;
        assert_eq!(rendered, "Title\n\nHello\n");
        Ok(())
    }

    #[test]
    fn preview_reports_writer_failures_as_write_errors() {
        let mut writer = FailingWriter;
        let result = preview("Hello\n", RenderOptions::default(), &mut writer);

        assert!(matches!(result, Err(PreviewError::Write(_))));
    }

    #[test]
    fn preview_error_exposes_the_underlying_cause_via_source() {
        use std::error::Error;

        let error = PreviewError::Write(io::Error::other("disk full"));

        assert!(!error.to_string().is_empty());
        let source = error.source();
        assert!(source.is_some_and(|cause| cause.to_string().contains("disk full")));
    }

    #[test]
    fn stream_errors_after_partial_output_still_terminate_with_a_newline()
    -> Result<(), PreviewError> {
        let renderer = mp_renderer::Renderer::new(RenderOptions::default());
        let blocks =
            vec![Ok(paragraph("hello")), Err(PreviewError::Write(io::Error::other("boom")))];
        let mut output = Vec::new();
        let result = render_stream(&renderer, blocks.into_iter(), &mut output);

        assert!(matches!(result, Err(PreviewError::Write(_))));
        assert_eq!(utf8(output)?, "hello\n");
        Ok(())
    }

    #[test]
    fn stream_errors_before_any_output_write_nothing() -> Result<(), PreviewError> {
        let renderer = mp_renderer::Renderer::new(RenderOptions::default());
        let blocks: Vec<Result<mp_ast::Block<'_>, PreviewError>> =
            vec![Err(PreviewError::Write(io::Error::other("boom")))];
        let mut output = Vec::new();
        let result = render_stream(&renderer, blocks.into_iter(), &mut output);

        assert!(matches!(result, Err(PreviewError::Write(_))));
        assert_eq!(utf8(output)?, "");
        Ok(())
    }

    #[test]
    fn stream_errors_outrank_finish_write_failures() {
        let renderer = mp_renderer::Renderer::new(RenderOptions::default());
        let blocks =
            vec![Ok(paragraph("hello")), Err(PreviewError::Write(io::Error::other("boom")))];
        let mut writer = LimitedWriter { remaining_writes: 1, bytes: Vec::new() };
        let result = render_stream(&renderer, blocks.into_iter(), &mut writer);

        assert!(matches!(result, Err(PreviewError::Write(_))));
    }

    fn paragraph(text: &'static str) -> mp_ast::Block<'static> {
        mp_ast::Block::Paragraph(vec![mp_ast::Inline::Text(mp_ast::Text::borrowed(text))])
    }

    fn utf8(bytes: Vec<u8>) -> Result<String, PreviewError> {
        String::from_utf8(bytes)
            .map_err(|error| PreviewError::Write(io::Error::new(io::ErrorKind::InvalidData, error)))
    }

    struct FailingWriter;

    impl Write for FailingWriter {
        fn write(&mut self, _buffer: &[u8]) -> io::Result<usize> {
            Err(io::Error::other("closed writer"))
        }

        fn flush(&mut self) -> io::Result<()> {
            Ok(())
        }
    }

    /// A writer that accepts a bounded number of writes, then fails every later write.
    struct LimitedWriter {
        remaining_writes: usize,
        bytes: Vec<u8>,
    }

    impl Write for LimitedWriter {
        fn write(&mut self, buffer: &[u8]) -> io::Result<usize> {
            if self.remaining_writes == 0 {
                return Err(io::Error::other("exhausted writer"));
            }
            self.remaining_writes -= 1;
            self.bytes.write(buffer)
        }

        fn flush(&mut self) -> io::Result<()> {
            self.bytes.flush()
        }
    }
}
