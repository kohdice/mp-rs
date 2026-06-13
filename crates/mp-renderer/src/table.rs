use std::io::{self, Write};
use std::ops::Range;

use mp_ast::{Alignment, Inline, Table};

use crate::writer::{write_repeated_str, write_spaces};

/// Columns never shrink below this content width, even when the table then
/// overflows the available width.
const MIN_COLUMN_WIDTH: usize = 3;

pub(crate) fn table_layout(
    table: &Table<'_>,
    measure_cell: impl Fn(&[Inline<'_>]) -> usize,
    available: Option<usize>,
) -> Vec<usize> {
    // The wider of the header and the alignment row decides the column count;
    // parser-produced rows are normalized to it, and cells beyond it in
    // hand-built rows are ignored.
    let column_count = table.alignments.len().max(table.header.len());

    let mut widths = vec![MIN_COLUMN_WIDTH; column_count];
    update_widths(&mut widths, &table.header, &measure_cell);

    for row in &table.rows {
        update_widths(&mut widths, row, &measure_cell);
    }

    if let Some(available) = available {
        shrink_to_fit(&mut widths, available);
    }

    widths
}

/// Total display width of a table whose columns have the given content
/// widths: per column one leading border and two padding spaces, plus the
/// closing border.
fn table_total_width(widths: &[usize]) -> usize {
    widths.iter().sum::<usize>() + 3 * widths.len() + 1
}

/// Shrinks the tallest columns toward the next-tallest level until the table
/// fits in `available`, never dropping a column below the minimum width.
///
/// Each step lowers a whole front of equally-tall columns at once, so the loop
/// runs in time proportional to the number of columns rather than to the amount
/// of overflow (which a single wide cell can make arbitrarily large).
fn shrink_to_fit(widths: &mut [usize], available: usize) {
    loop {
        let Some(overflow) = table_total_width(widths).checked_sub(available) else {
            return;
        };
        if overflow == 0 {
            return;
        }
        let Some(max) = widths.iter().copied().filter(|width| *width > MIN_COLUMN_WIDTH).max()
        else {
            return; // every column already sits at the minimum width
        };
        // The next level to lower the tallest columns down to: the next-tallest
        // column, but never below the minimum width.
        let next_level = widths
            .iter()
            .copied()
            .filter(|width| *width < max)
            .max()
            .unwrap_or(MIN_COLUMN_WIDTH)
            .max(MIN_COLUMN_WIDTH);
        let tallest = widths.iter().filter(|width| **width == max).count();
        let headroom = max - next_level;
        let batch = overflow.min(tallest.saturating_mul(headroom));
        let base_step = batch / tallest;
        let mut extra_steps = batch % tallest;
        for width in widths.iter_mut() {
            if *width == max {
                let extra_step = if extra_steps > 0 {
                    extra_steps -= 1;
                    1
                } else {
                    0
                };
                *width -= base_step + extra_step;
            }
        }
    }
}

fn update_widths(
    widths: &mut [usize],
    row: &[Vec<Inline<'_>>],
    measure_cell: impl Fn(&[Inline<'_>]) -> usize,
) {
    for (target, cell) in widths.iter_mut().zip(row) {
        *target = (*target).max(measure_cell(cell));
    }
}

/// A table cell rendered into a single buffer, with its newline-separated output
/// lines kept as ranges into that buffer so no per-line copies are made.
pub(crate) struct WrappedCell {
    bytes: Vec<u8>,
    lines: Vec<Range<usize>>,
}

impl WrappedCell {
    pub(crate) fn new(bytes: Vec<u8>) -> Self {
        let mut lines = Vec::new();
        let mut start = 0;
        for (index, byte) in bytes.iter().enumerate() {
            if *byte == b'\n' {
                lines.push(start..index);
                start = index + 1;
            }
        }
        lines.push(start..bytes.len());
        Self { bytes, lines }
    }

    pub(crate) fn height(&self) -> usize {
        self.lines.len()
    }

    fn line(&self, index: usize) -> &[u8] {
        self.lines.get(index).map_or(&[][..], |range| &self.bytes[range.clone()])
    }
}

/// Writes one bordered output line of a (possibly multi-line) table row.
///
/// `cells` holds each column's pre-rendered wrapped lines; columns whose cell
/// has no content on `line_index` render as padding so the borders stay
/// aligned. Cell bytes may contain ANSI escape sequences; padding is computed
/// from their visible width.
pub(crate) fn write_table_row_line(
    writer: &mut dyn Write,
    cells: &[WrappedCell],
    line_index: usize,
    widths: &[usize],
    alignments: &[Alignment],
) -> io::Result<()> {
    writer.write_all("│".as_bytes())?;
    for (column, width) in widths.iter().enumerate() {
        if column > 0 {
            writer.write_all("│".as_bytes())?;
        }
        writer.write_all(b" ")?;
        let line = cells.get(column).map_or([].as_slice(), |cell| cell.line(line_index));
        write_aligned_line(
            writer,
            line,
            *width,
            alignments.get(column).copied().unwrap_or(Alignment::None),
        )?;
        writer.write_all(b" ")?;
    }
    writer.write_all("│".as_bytes())
}

fn write_aligned_line(
    writer: &mut dyn Write,
    line: &[u8],
    width: usize,
    alignment: Alignment,
) -> io::Result<()> {
    let padding = width.saturating_sub(crate::wrap::visible_width(line));
    match alignment {
        Alignment::Right => {
            write_spaces(writer, padding)?;
            writer.write_all(line)
        }
        Alignment::Center => {
            let left = padding / 2;
            let right = padding - left;
            write_spaces(writer, left)?;
            writer.write_all(line)?;
            write_spaces(writer, right)
        }
        Alignment::None | Alignment::Left => {
            writer.write_all(line)?;
            write_spaces(writer, padding)
        }
    }
}

#[derive(Debug, Clone, Copy)]
pub(crate) enum BorderKind {
    Top,
    Middle,
    Bottom,
}

pub(crate) fn write_border_line<W>(
    writer: &mut W,
    widths: &[usize],
    kind: BorderKind,
) -> io::Result<()>
where
    W: Write + ?Sized,
{
    let (left, join, right) = match kind {
        BorderKind::Top => ("┌", "┬", "┐"),
        BorderKind::Middle => ("├", "┼", "┤"),
        BorderKind::Bottom => ("└", "┴", "┘"),
    };

    writer.write_all(left.as_bytes())?;
    for (index, width) in widths.iter().enumerate() {
        if index > 0 {
            writer.write_all(join.as_bytes())?;
        }
        write_repeated_str(writer, "─", width + 2)?;
    }
    writer.write_all(right.as_bytes())
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::wrap::display_width;
    use mp_ast::Text;

    #[test]
    fn table_layout_ignores_cells_beyond_the_header_columns() {
        let table = Table {
            header: vec![cell("H")],
            alignments: vec![Alignment::Left],
            rows: vec![vec![cell("a"), cell("ignored extra cell")]],
        };

        let widths = table_layout(&table, measure_text, None);

        assert_eq!(widths.len(), 1, "column count follows the header, not ragged rows");
    }

    #[test]
    fn table_layout_preserves_measured_unicode_cell_widths() {
        let table = Table {
            header: vec![cell("H")],
            alignments: vec![Alignment::Left],
            rows: vec![vec![cell("日本語")], vec![cell("e\u{301}")], vec![cell("👩\u{200d}💻")]],
        };

        let widths = table_layout(&table, measure_text, None);

        assert_eq!(widths, vec![6]);
    }

    #[test]
    fn shrinks_the_widest_column_first_to_fit_the_available_width() {
        let table = Table {
            header: vec![cell("HH"), cell("HHHH")],
            alignments: vec![Alignment::Left, Alignment::Left],
            rows: vec![vec![cell("aaaa"), cell("bbbbbbbbbbbb")]],
        };

        // Natural widths are [4, 12]; borders and padding add 7, so the table
        // is 23 columns wide. Fitting into 19 must take 4 from the widest.
        let widths = table_layout(&table, measure_text, Some(19));

        assert_eq!(widths, vec![4, 8]);
    }

    #[test]
    fn tied_columns_shrink_only_by_the_remaining_overflow() {
        let table = Table {
            header: vec![cell("aaaaaaaaaa"), cell("bbbbbbbbbb")],
            alignments: vec![Alignment::Left, Alignment::Left],
            rows: vec![],
        };

        // Natural widths are [10, 10]; borders and padding add 7, so the
        // table is 27 columns wide. Fitting into 26 must shrink only one
        // content column, not every tied widest column.
        let widths = table_layout(&table, measure_text, Some(26));

        assert_eq!(table_total_width(&widths), 26);
        assert_eq!(
            widths.iter().filter(|width| **width == 10).count(),
            1,
            "one tied column should keep its full width: {widths:?}"
        );
    }

    #[test]
    fn columns_never_shrink_below_the_minimum_width() {
        let table = Table {
            header: vec![cell("HHHHHHH"), cell("HHHHHHH")],
            alignments: vec![Alignment::Left, Alignment::Left],
            rows: vec![],
        };

        // Even a 5-column budget cannot push content widths below 3.
        let widths = table_layout(&table, measure_text, Some(5));

        assert_eq!(widths, vec![3, 3]);
    }

    fn measure_text(cell: &[Inline<'_>]) -> usize {
        cell.iter()
            .map(|inline| match inline {
                Inline::Text(text) => display_width(text),
                _ => 0,
            })
            .sum()
    }

    fn cell(text: &'static str) -> Vec<Inline<'static>> {
        vec![Inline::Text(Text::borrowed(text))]
    }
}
