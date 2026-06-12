use std::io::{self, Write};

use mp_ast::{List, ListKind, TaskState};
use unicode_width::UnicodeWidthStr;

#[derive(Debug, Clone, Copy)]
pub(crate) enum ListMarkerDisplay<'a> {
    Text(&'a str),
    OrderedGenerated(u64),
}

pub(crate) fn list_marker<'a>(
    list: &List<'_>,
    index: usize,
    depth: usize,
) -> ListMarkerDisplay<'a> {
    match list.kind {
        ListKind::Ordered { start } => {
            // CommonMark caps ordered-list starts at 9 digits, so this saturation is
            // unobservable in practice; it only replaces a misclassified overflow error.
            let offset = u64::try_from(index).unwrap_or(u64::MAX);
            ListMarkerDisplay::OrderedGenerated(start.saturating_add(offset))
        }
        ListKind::Unordered => ListMarkerDisplay::Text(unordered_marker(depth)),
    }
}

pub(crate) fn write_list_marker<W>(writer: &mut W, marker: ListMarkerDisplay<'_>) -> io::Result<()>
where
    W: Write + ?Sized,
{
    match marker {
        ListMarkerDisplay::Text(text) => writer.write_all(text.as_bytes()),
        ListMarkerDisplay::OrderedGenerated(value) => write!(writer, "{value}."),
    }
}

pub(crate) fn task_marker(task: Option<TaskState>) -> Option<&'static str> {
    match task {
        Some(TaskState::Checked) => Some("☑"),
        Some(TaskState::Unchecked) => Some("☐"),
        None => None,
    }
}

fn unordered_marker(depth: usize) -> &'static str {
    match depth % 3 {
        0 => "•",
        1 => "◦",
        _ => "▪",
    }
}

pub(crate) fn marker_width(marker: ListMarkerDisplay<'_>, task: Option<TaskState>) -> usize {
    // Each present part is followed by one separating space before the content column.
    let task_width = task_marker(task).map_or(0, |glyph| str_width(glyph) + 1);
    marker_text_width(marker) + 1 + task_width
}

fn marker_text_width(marker: ListMarkerDisplay<'_>) -> usize {
    match marker {
        ListMarkerDisplay::Text(text) => str_width(text),
        ListMarkerDisplay::OrderedGenerated(value) => decimal_width(value) + 1,
    }
}

fn decimal_width(value: u64) -> usize {
    value.checked_ilog10().map_or(1, |log10| log10 as usize + 1)
}

pub(crate) fn str_width(text: &str) -> usize {
    text.width()
}

#[cfg(test)]
mod tests {
    use super::{ListMarkerDisplay, marker_text_width, str_width, write_list_marker};

    #[test]
    fn measures_fullwidth_and_combining_text_widths() {
        assert_eq!(str_width("abc"), 3);
        assert_eq!(str_width("日本語"), 6);
        assert_eq!(str_width("e\u{301}"), 1);
        assert_eq!(str_width("👩\u{200d}💻"), 2);
    }

    #[test]
    fn ordered_marker_text_width_matches_the_written_marker() -> std::io::Result<()> {
        for value in [1, 9, 10, 999, u64::MAX] {
            let marker = ListMarkerDisplay::OrderedGenerated(value);
            let mut buffer = Vec::new();
            write_list_marker(&mut buffer, marker)?;
            let written = String::from_utf8(buffer)
                .map_err(|error| std::io::Error::new(std::io::ErrorKind::InvalidData, error))?;

            assert_eq!(str_width(&written), marker_text_width(marker), "value {value}");
        }
        Ok(())
    }
}
