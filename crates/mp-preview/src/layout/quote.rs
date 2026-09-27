//! Blockquote bars.

use unicode_width::UnicodeWidthStr;

use crate::ansi::ColorMode;
use crate::model::Block;
use crate::style::{Line, Span, Style};
use crate::theme::solarized::DARK_PALETTE;

/// Prefix of quoted content lines; blank quoted lines carry the bar without the space.
const BAR: &str = "│ ";

pub(super) fn lay_out_quote(
    blocks: &[Block],
    width: Option<usize>,
    color: ColorMode,
    depth: usize,
) -> Vec<Line> {
    let style = Style { fg: Some(DARK_PALETTE.muted), dim: true, ..Style::default() };
    let bar = Span { text: BAR.to_owned(), style };
    let bare_bar = Span { text: BAR.trim_end().to_owned(), style };
    let mut lines =
        super::lay_out_blocks(blocks, true, super::narrow(width, BAR.width()), color, depth);
    if lines.is_empty() {
        lines.push(Line::new());
    }
    for line in &mut lines {
        let prefix = if line.is_empty() { &bare_bar } else { &bar };
        line.insert(0, prefix.clone());
    }
    lines
}
