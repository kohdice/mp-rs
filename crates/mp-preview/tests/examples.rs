//! End-to-end previews of the bundled example documents.

use std::io;

use mp_preview::{ColorMode, Options, preview};
use unicode_width::UnicodeWidthStr;

const WIDTH: usize = 80;

#[test]
fn renders_example_documents_within_the_width() -> io::Result<()> {
    for markdown in [
        include_str!("../../../examples/EXAMPLE.md"),
        include_str!("../../../examples/EXAMPLE_ja.md"),
    ] {
        let options = Options { width: Some(WIDTH), color: ColorMode::Plain };
        let mut output = Vec::new();
        preview(markdown, &options, &mut output)?;
        let output = String::from_utf8(output)
            .map_err(|error| io::Error::new(io::ErrorKind::InvalidData, error))?;

        assert!(!output.contains('\x1b'), "plain output must not contain ESC");
        assert!(
            output.ends_with('\n') && !output.ends_with("\n\n"),
            "output must end with exactly one newline"
        );
        for line in lines_outside_code_blocks(&output) {
            assert!(line.width() <= WIDTH, "line wider than {WIDTH} columns: {line:?}");
        }
    }
    Ok(())
}

/// Drops fenced code blocks, fences included, since code is never wrapped. A fence may sit
/// behind list indentation or quote bars. The examples contain no HTML blocks.
fn lines_outside_code_blocks(output: &str) -> Vec<&str> {
    let mut in_code = false;
    let mut kept = Vec::new();
    for line in output.lines() {
        let content = line.trim_start_matches([' ', '│']);
        if content.starts_with("```") {
            in_code = !in_code;
        } else if !in_code {
            kept.push(line);
        }
    }
    kept
}
