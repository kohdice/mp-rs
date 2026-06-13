use std::io::{self, Write};

use mp_ast::{List, ListKind, TaskState};

use crate::wrap::display_width;

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

pub(crate) fn task_marker(task: TaskState) -> &'static str {
    match task {
        TaskState::Checked => "☑",
        TaskState::Unchecked => "☐",
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
    let task_width = task.map_or(0, |state| display_width(task_marker(state)) + 1);
    marker_text_width(marker) + 1 + task_width
}

fn marker_text_width(marker: ListMarkerDisplay<'_>) -> usize {
    match marker {
        ListMarkerDisplay::Text(text) => display_width(text),
        ListMarkerDisplay::OrderedGenerated(value) => decimal_width(value) + 1,
    }
}

fn decimal_width(value: u64) -> usize {
    value.checked_ilog10().map_or(1, |log10| log10 as usize + 1)
}

#[cfg(test)]
mod tests {
    use super::{ListMarkerDisplay, marker_text_width, write_list_marker};
    use crate::wrap::display_width;

    #[test]
    fn ordered_marker_text_width_matches_the_written_marker() -> std::io::Result<()> {
        for value in [1, 9, 10, 999, u64::MAX] {
            let marker = ListMarkerDisplay::OrderedGenerated(value);
            let mut buffer = Vec::new();
            write_list_marker(&mut buffer, marker)?;
            let written = String::from_utf8(buffer)
                .map_err(|error| std::io::Error::new(std::io::ErrorKind::InvalidData, error))?;

            assert_eq!(display_width(&written), marker_text_width(marker), "value {value}");
        }
        Ok(())
    }
}
