//! List item markers and the content column they define.

use unicode_width::UnicodeWidthStr;

use crate::ansi::ColorMode;
use crate::model::ListItem;
use crate::style::{Line, Span, Style};
use crate::theme::solarized::DARK_PALETTE;

/// A loose list separates items, and the direct child blocks of each item, with one
/// blank line; a tight list uses none.
pub(super) fn lay_out_list(
    start: Option<u64>,
    tight: bool,
    items: &[ListItem],
    width: Option<usize>,
    color: ColorMode,
    depth: usize,
) -> Vec<Line> {
    let mut lines = Vec::new();
    for (index, item) in items.iter().enumerate() {
        if index > 0 && !tight {
            lines.push(Line::new());
        }
        let marker = list_marker(start, index, depth);
        let column = content_column(&marker, item.task);
        let indent = " ".repeat(column);
        let item_lines = super::lay_out_blocks(
            &item.blocks,
            !tight,
            super::narrow(width, column),
            color,
            depth + 1,
        );
        let mut head = item_head(marker, item.task);
        if item_lines.is_empty() {
            // An item without content ends with its marker (or box), not with the
            // separating space.
            head.pop();
            lines.push(head);
            continue;
        }
        for (line_index, mut line) in item_lines.into_iter().enumerate() {
            if line_index == 0 {
                line.splice(0..0, head.iter().cloned());
            } else if !line.is_empty() {
                line.insert(0, Span { text: indent.clone(), style: Style::default() });
            }
            lines.push(line);
        }
    }
    lines
}

/// Styled marker and task box, each followed by one unstyled space.
fn item_head(marker: String, task: Option<bool>) -> Line {
    let marker_style = Style { fg: Some(DARK_PALETTE.list_marker), bold: true, ..Style::default() };
    let space = Span { text: " ".to_owned(), style: Style::default() };
    let mut head = vec![Span { text: marker, style: marker_style }, space.clone()];
    if let Some(checked) = task {
        head.extend([
            Span { text: task_box(checked).to_owned(), style: task_style(checked) },
            space,
        ]);
    }
    head
}

/// Returns the marker of the item at `index`: `N.` for ordered lists (`start` is
/// `Some`), otherwise a bullet chosen by the list nesting `depth`.
fn list_marker(start: Option<u64>, index: usize, depth: usize) -> String {
    match start {
        Some(start) => {
            // CommonMark limits a list start to 9 digits, so parsed input cannot overflow;
            // saturating still guards a `Block::List` built with a larger `start`.
            let offset = u64::try_from(index).unwrap_or(u64::MAX);
            format!("{}.", start.saturating_add(offset))
        }
        None => unordered_marker(depth).to_owned(),
    }
}

fn unordered_marker(depth: usize) -> &'static str {
    match depth % 3 {
        0 => "•",
        1 => "◦",
        _ => "▪",
    }
}

fn task_box(checked: bool) -> &'static str {
    if checked { "☑" } else { "☐" }
}

fn task_style(checked: bool) -> Style {
    if checked {
        Style { fg: Some(DARK_PALETTE.list_marker), ..Style::default() }
    } else {
        Style { fg: Some(DARK_PALETTE.muted), dim: true, ..Style::default() }
    }
}

/// Column where item content starts: each present part (marker, task box) is
/// followed by one separating space.
fn content_column(marker: &str, task: Option<bool>) -> usize {
    let task_width = task.map_or(0, |checked| task_box(checked).width() + 1);
    marker.width() + 1 + task_width
}
