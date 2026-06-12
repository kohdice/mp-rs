use std::io::{self, Write};

use mp_ast::{Alignment, Inline, Table};

use crate::writer::{write_repeated_str, write_spaces};

#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) struct TableLayout {
    pub(crate) widths: Vec<usize>,
}

/// Columns never shrink below this content width, even when the table then
/// overflows the available width.
const MIN_COLUMN_WIDTH: usize = 3;

pub(crate) fn table_layout(
    table: &Table<'_>,
    measure_cell: &dyn Fn(&[Inline<'_>]) -> usize,
    available: Option<usize>,
) -> TableLayout {
    // Parser-produced tables are already normalized to the header width (GFM), so the
    // header decides the column count; cells beyond it in hand-built rows are ignored.
    let column_count = table.alignments.len().max(table.header.len());

    let mut widths = vec![MIN_COLUMN_WIDTH; column_count];
    update_widths(&mut widths, &table.header, measure_cell);

    for row in &table.rows {
        update_widths(&mut widths, row, measure_cell);
    }

    if let Some(available) = available {
        shrink_to_fit(&mut widths, available);
    }

    TableLayout { widths }
}

/// Total display width of a table whose columns have the given content
/// widths: per column one leading border and two padding spaces, plus the
/// closing border.
fn table_total_width(widths: &[usize]) -> usize {
    widths.iter().sum::<usize>() + 3 * widths.len() + 1
}

/// Shrinks the widest column one display column at a time until the table
/// fits in `available`, stopping once every column is at the minimum width.
fn shrink_to_fit(widths: &mut [usize], available: usize) {
    while table_total_width(widths) > available {
        let Some(widest) = widths
            .iter_mut()
            .filter(|width| **width > MIN_COLUMN_WIDTH)
            .max_by_key(|width| **width)
        else {
            return;
        };
        *widest -= 1;
    }
}

fn update_widths(
    widths: &mut [usize],
    row: &[Vec<Inline<'_>>],
    measure_cell: &dyn Fn(&[Inline<'_>]) -> usize,
) {
    for (target, cell) in widths.iter_mut().zip(row) {
        *target = (*target).max(measure_cell(cell));
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
    cells: &[Vec<Vec<u8>>],
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
        let line = cells
            .get(column)
            .and_then(|lines| lines.get(line_index))
            .map_or([].as_slice(), Vec::as_slice);
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
    use crate::list::str_width;
    use mp_ast::Text;

    #[test]
    fn table_layout_ignores_cells_beyond_the_header_columns() {
        let table = Table {
            header: vec![cell("H")],
            alignments: vec![Alignment::Left],
            rows: vec![vec![cell("a"), cell("ignored extra cell")]],
        };

        let layout = table_layout(&table, &measure_text, None);

        assert_eq!(layout.widths.len(), 1, "column count follows the header, not ragged rows");
    }

    #[test]
    fn table_layout_preserves_measured_unicode_cell_widths() {
        let table = Table {
            header: vec![cell("H")],
            alignments: vec![Alignment::Left],
            rows: vec![vec![cell("日本語")], vec![cell("e\u{301}")], vec![cell("👩\u{200d}💻")]],
        };

        let layout = table_layout(&table, &measure_text, None);

        assert_eq!(layout.widths, vec![6]);
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
        let layout = table_layout(&table, &measure_text, Some(19));

        assert_eq!(layout.widths, vec![4, 8]);
    }

    #[test]
    fn columns_never_shrink_below_the_minimum_width() {
        let table = Table {
            header: vec![cell("HHHHHHH"), cell("HHHHHHH")],
            alignments: vec![Alignment::Left, Alignment::Left],
            rows: vec![],
        };

        // Even a 5-column budget cannot push content widths below 3.
        let layout = table_layout(&table, &measure_text, Some(5));

        assert_eq!(layout.widths, vec![3, 3]);
    }

    fn measure_text(cell: &[Inline<'_>]) -> usize {
        cell.iter()
            .map(|inline| match inline {
                Inline::Text(text) => str_width(text),
                _ => 0,
            })
            .sum()
    }

    fn cell(text: &'static str) -> Vec<Inline<'static>> {
        vec![Inline::Text(Text::borrowed(text))]
    }
}
