//! Blockquote bars.

use unicode_width::UnicodeWidthStr;

use crate::model::{AlertKind, Block, Inline};
use crate::style::{ColorMode, Line, Span, Style};
use crate::theme::solarized::DARK_PALETTE;

/// Prefix of quoted content lines; blank quoted lines carry the bar without the space.
const BAR: &str = "│ ";

pub(super) fn lay_out_quote(
    blocks: &[Block],
    width: Option<usize>,
    color: ColorMode,
    depth: usize,
) -> Vec<Line> {
    let lines =
        super::lay_out_blocks(blocks, true, super::narrow(width, BAR.width()), color, depth);
    with_bar(lines, Style { fg: Some(DARK_PALETTE.muted), dim: true, ..Style::default() })
}

/// Lays out an alert as a quote headed by the bold `title`, separated from the body by a
/// blank line when both are non-empty; the bar and the title take the color of `kind`.
pub(super) fn lay_out_alert(
    kind: AlertKind,
    title: &str,
    blocks: &[Block],
    width: Option<usize>,
    color: ColorMode,
    depth: usize,
) -> Vec<Line> {
    let alert = DARK_PALETTE.alert;
    let kind_color = match kind {
        AlertKind::Note => alert.note,
        AlertKind::Tip => alert.tip,
        AlertKind::Important => alert.important,
        AlertKind::Warning => alert.warning,
        AlertKind::Caution => alert.caution,
    };
    let inner = super::narrow(width, BAR.width());
    let title_style = Style { fg: Some(kind_color), bold: true, ..Style::default() };
    let mut lines =
        super::text::lay_out_inlines(&[Inline::Text(title.to_owned())], title_style, inner);
    let body = super::lay_out_blocks(blocks, true, inner, color, depth);
    if !lines.is_empty() && !body.is_empty() {
        lines.push(Line::new());
    }
    lines.extend(body);
    with_bar(lines, Style { fg: Some(kind_color), ..Style::default() })
}

/// Prefixes every line with a bar in `style`; with no lines, gives a single bare bar.
fn with_bar(mut lines: Vec<Line>, style: Style) -> Vec<Line> {
    let bar = Span { text: BAR.to_owned(), style };
    let bare_bar = Span { text: BAR.trim_end().to_owned(), style };
    if lines.is_empty() {
        lines.push(Line::new());
    }
    for line in &mut lines {
        let prefix = if line.is_empty() { &bare_bar } else { &bar };
        line.insert(0, prefix.clone());
    }
    lines
}
