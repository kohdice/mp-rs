//! Renders Markdown as styled terminal text.
//!
//! [`preview`] is the whole public surface: callers own reading the input so they can
//! attach their own path-bearing error context, and choose the width and [`ColorMode`]
//! through [`Options`].

mod ansi;
mod control;
mod diagram;
mod highlight;
mod layout;
mod markdown;
mod model;
mod style;
mod theme;

use std::io::{self, Write};

pub use style::ColorMode;

use layout::lay_out_block;

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
/// before the next is laid out. A block that lays out to no lines, such as an empty
/// heading, is skipped along with its separator.
///
/// # Errors
///
/// Returns the first error reported by `out`, leaving the blocks written before it in
/// place. Parsing never fails.
pub fn preview<W: Write>(markdown: &str, options: &Options, mut out: W) -> io::Result<()> {
    let mut wrote_block = false;
    for block in &markdown::parse(markdown) {
        let lines = lay_out_block(block, options.width, options.color, 0);
        if lines.is_empty() {
            continue;
        }
        let mut encoded = if wrote_block { String::from("\n") } else { String::new() };
        wrote_block = true;
        encoded.push_str(&ansi::to_ansi(&lines, options.color));
        encoded.push('\n');
        out.write_all(encoded.as_bytes())?;
    }
    Ok(())
}
