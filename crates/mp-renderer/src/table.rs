use std::io::{self, Write};

use mp_ast::{Alignment, Inline, Table};

use crate::plain::{plain_inlines_width, write_plain_inlines};
use crate::writer::{write_repeated_str, write_spaces};

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

pub(crate) fn table_layout(table: &Table<'_>) -> TableLayout {
    let mut column_count = table.alignments.len().max(table.header.len());
    for row in &table.rows {
        column_count = column_count.max(row.len());
    }

    let header = row_layout(&table.header);
    let rows: Vec<_> = table.rows.iter().map(Vec::as_slice).map(row_layout).collect();

    let mut widths = vec![3; column_count];
    update_widths(&mut widths, &header);

    for row in &rows {
        update_widths(&mut widths, row);
    }

    TableLayout { widths, header, rows }
}

pub(crate) fn row_layout(row: &[Vec<Inline<'_>>]) -> RowLayout {
    RowLayout { cell_widths: row.iter().map(Vec::as_slice).map(plain_inlines_width).collect() }
}

fn update_widths(widths: &mut [usize], row: &RowLayout) {
    for (index, width) in row.cell_widths.iter().enumerate() {
        widths[index] = widths[index].max(*width);
    }
}

pub(crate) fn write_table_row<W>(
    writer: &mut W,
    row: &[Vec<Inline<'_>>],
    row_layout: &RowLayout,
    widths: &[usize],
    alignments: &[Alignment],
) -> io::Result<()>
where
    W: Write + ?Sized,
{
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
        )?;
        writer.write_all(b" ")?;
    }
    writer.write_all("│".as_bytes())
}

fn write_padded_cell<W>(
    writer: &mut W,
    cell: &[Inline<'_>],
    cell_width: usize,
    width: usize,
    alignment: Alignment,
) -> io::Result<()>
where
    W: Write + ?Sized,
{
    let padding = width.saturating_sub(cell_width);
    match alignment {
        Alignment::Right => {
            write_spaces(writer, padding)?;
            write_plain_inlines(writer, cell)
        }
        Alignment::Center => {
            let left = padding / 2;
            let right = padding - left;
            write_spaces(writer, left)?;
            write_plain_inlines(writer, cell)?;
            write_spaces(writer, right)
        }
        Alignment::None | Alignment::Left => {
            write_plain_inlines(writer, cell)?;
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
    use mp_ast::Text;

    #[test]
    fn table_layout_preserves_measured_unicode_cell_widths() {
        let table = Table {
            header: vec![cell("H")],
            alignments: vec![Alignment::Left],
            rows: vec![vec![cell("日本語")], vec![cell("e\u{301}")], vec![cell("👩\u{200d}💻")]],
        };

        let layout = table_layout(&table);

        assert_eq!(layout.widths, vec![6]);
    }

    fn cell(text: &'static str) -> Vec<Inline<'static>> {
        vec![Inline::Text(Text::borrowed(text))]
    }
}
