use std::io::{self, Write};

use mp_ast::{Alignment, Inline, Table};

use crate::writer::{write_repeated_str, write_spaces};

/// Writes a cell's inline content to the row writer.
///
/// The width passed to [`write_table_row`] is measured by rendering the same cell through
/// a width-counting writer (see [`table_layout`]), so a writer must emit exactly that
/// visible text; styling is only allowed through zero-width ANSI sequences, which keeps
/// padding and borders aligned.
pub(crate) type CellWriter<'a> = dyn Fn(&mut dyn Write, &[Inline<'_>]) -> io::Result<()> + 'a;

#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) struct TableLayout {
    pub(crate) widths: Vec<usize>,
    pub(crate) header: RowLayout,
    pub(crate) rows: Vec<RowLayout>,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) struct RowLayout {
    cell_widths: Vec<usize>,
}

pub(crate) fn table_layout(
    table: &Table<'_>,
    measure_cell: &dyn Fn(&[Inline<'_>]) -> usize,
) -> TableLayout {
    // Parser-produced tables are already normalized to the header width (GFM), so the
    // header decides the column count; cells beyond it in hand-built rows are ignored.
    let column_count = table.alignments.len().max(table.header.len());

    let header = row_layout(&table.header, measure_cell);
    let rows: Vec<_> = table.rows.iter().map(|row| row_layout(row, measure_cell)).collect();

    let mut widths = vec![3; column_count];
    update_widths(&mut widths, &header);

    for row in &rows {
        update_widths(&mut widths, row);
    }

    TableLayout { widths, header, rows }
}

fn row_layout(row: &[Vec<Inline<'_>>], measure_cell: &dyn Fn(&[Inline<'_>]) -> usize) -> RowLayout {
    RowLayout { cell_widths: row.iter().map(|cell| measure_cell(cell)).collect() }
}

fn update_widths(widths: &mut [usize], row: &RowLayout) {
    for (target, width) in widths.iter_mut().zip(&row.cell_widths) {
        *target = (*target).max(*width);
    }
}

pub(crate) fn write_table_row(
    writer: &mut dyn Write,
    row: &[Vec<Inline<'_>>],
    row_layout: &RowLayout,
    widths: &[usize],
    alignments: &[Alignment],
    write_cell: &CellWriter<'_>,
) -> io::Result<()> {
    writer.write_all("│".as_bytes())?;
    for (index, width) in widths.iter().enumerate() {
        if index > 0 {
            writer.write_all("│".as_bytes())?;
        }
        writer.write_all(b" ")?;
        let cell = row.get(index).map_or([].as_slice(), Vec::as_slice);
        let cell_width = row_layout.cell_widths.get(index).copied().unwrap_or_default();
        write_padded_cell(
            writer,
            cell,
            cell_width,
            *width,
            alignments.get(index).copied().unwrap_or(Alignment::None),
            write_cell,
        )?;
        writer.write_all(b" ")?;
    }
    writer.write_all("│".as_bytes())
}

fn write_padded_cell(
    writer: &mut dyn Write,
    cell: &[Inline<'_>],
    cell_width: usize,
    width: usize,
    alignment: Alignment,
    write_cell: &CellWriter<'_>,
) -> io::Result<()> {
    let padding = width.saturating_sub(cell_width);
    match alignment {
        Alignment::Right => {
            write_spaces(writer, padding)?;
            write_cell(writer, cell)
        }
        Alignment::Center => {
            let left = padding / 2;
            let right = padding - left;
            write_spaces(writer, left)?;
            write_cell(writer, cell)?;
            write_spaces(writer, right)
        }
        Alignment::None | Alignment::Left => {
            write_cell(writer, cell)?;
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

        let layout = table_layout(&table, &measure_text);

        assert_eq!(layout.widths.len(), 1, "column count follows the header, not ragged rows");
    }

    #[test]
    fn table_layout_preserves_measured_unicode_cell_widths() {
        let table = Table {
            header: vec![cell("H")],
            alignments: vec![Alignment::Left],
            rows: vec![vec![cell("日本語")], vec![cell("e\u{301}")], vec![cell("👩\u{200d}💻")]],
        };

        let layout = table_layout(&table, &measure_text);

        assert_eq!(layout.widths, vec![6]);
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
