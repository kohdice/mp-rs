//! Text drawings of diagrams written in a ```` ```mermaid ```` code block.

mod canvas;
mod flowchart;

use crate::style::Line;

/// Why a Mermaid block is shown as its source instead of a drawing.
#[derive(Debug)]
pub(crate) enum Failure {
    /// Valid Mermaid that is not drawn: a feature without a text drawing, or a drawing
    /// that does not fit the width or the size limit. There is no syntax to fix.
    Unsupported,
    /// Input that is not valid Mermaid.
    Syntax(SyntaxError),
}

/// Input that is not valid Mermaid. Displays as `line N: message`, or as the message
/// alone when no line is to blame.
#[derive(Debug, thiserror::Error)]
#[error("{}", located(*.line, .message))]
pub(crate) struct SyntaxError {
    /// 1-based line within the diagram source.
    pub line: Option<usize>,
    /// What is wrong, shown after the line number.
    pub message: String,
}

/// `message` prefixed with `line N: ` when `line` is known.
fn located(line: Option<usize>, message: &str) -> String {
    match line {
        Some(line) => format!("line {line}: {message}"),
        None => message.to_owned(),
    }
}

/// Mermaid's default `maxTextSize`
/// (<https://mermaid.js.org/config/schema-docs/config.html>), in characters.
const MAX_TEXT_SIZE: usize = 50_000;

/// Draws the Mermaid `source` as lines without trailing spaces, in at most `width`
/// columns when given.
///
/// Returns [`Failure::Syntax`] when the source is not valid Mermaid, exceeds
/// [`MAX_TEXT_SIZE`] characters, or has more edges than Mermaid allows, and
/// [`Failure::Unsupported`] for valid input that has no text drawing: another diagram
/// type, a feature that is not drawn, an empty chart, subgraph frames that would cover
/// a box or frame outside them, long links passing through too many layers, or a
/// drawing that fits neither `width` nor the cell limit at any spacing.
pub(crate) fn render_mermaid(source: &str, width: Option<usize>) -> Result<Vec<Line>, Failure> {
    // A string has at least as many bytes as characters, so only a long one is counted.
    if source.len() > MAX_TEXT_SIZE && source.chars().count() > MAX_TEXT_SIZE {
        return Err(Failure::Syntax(SyntaxError {
            line: None,
            message: format!("diagram text exceeds {MAX_TEXT_SIZE} characters"),
        }));
    }
    flowchart::render(source, width)
}

#[cfg(test)]
mod tests;
