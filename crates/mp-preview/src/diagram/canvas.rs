//! A grid of terminal cells that diagrams draw on before becoming lines.

use unicode_width::UnicodeWidthStr;

use crate::style::{Line, Span, Style};

/// Bits naming the neighbours a line cell connects to.
const UP: u8 = 1;
const DOWN: u8 = 2;
const LEFT: u8 = 4;
const RIGHT: u8 = 8;

/// Glyphs indexed by connection bits. A bare run in one direction is replaced by the
/// line's own straight glyph, so only turns and junctions come from this table.
const JUNCTIONS: [&str; 16] =
    [" ", "│", "│", "│", "─", "┘", "┐", "┤", "─", "└", "┌", "├", "─", "┴", "┬", "┼"];

#[derive(Debug, Clone, Copy, Default)]
enum Cell<'a> {
    #[default]
    Blank,
    Text(&'a str, Style),
    /// A column covered by the wider text to its left.
    Covered,
    /// Part of one or more lines, connecting towards the neighbours in `connections`.
    Line {
        connections: u8,
        glyphs: LineGlyphs,
        style: Style,
    },
}

/// The glyphs of a straight horizontal and a straight vertical run of one line style.
#[derive(Debug, Clone, Copy)]
pub(crate) struct LineGlyphs {
    pub horizontal: &'static str,
    pub vertical: &'static str,
}

#[derive(Debug, Default)]
pub(crate) struct Canvas<'a> {
    rows: Vec<Vec<Cell<'a>>>,
}

impl<'a> Canvas<'a> {
    /// Writes `text` starting at `col`, covering as many columns as its display width.
    /// Text is placed as one unit rather than per character, so that its columns always
    /// add up to `text.width()`, the same measure the boxes around labels are sized by.
    /// Earlier text cut by either end of the new text is blanked, since what is left of
    /// it would no longer take as many columns as it covers.
    pub(crate) fn put(&mut self, row: usize, col: usize, text: &'a str, style: Style) {
        let width = text.width();
        if width == 0 {
            return;
        }
        let end = col + width;
        let Some(cells) = self.cells(row, end) else { return };
        if let Some(Cell::Covered) = cells.get(col)
            && let Some(before) = cells.get_mut(..col)
        {
            for cell in before.iter_mut().rev() {
                match cell {
                    Cell::Covered => *cell = Cell::Blank,
                    Cell::Text(..) => {
                        *cell = Cell::Blank;
                        break;
                    }
                    Cell::Blank | Cell::Line { .. } => break,
                }
            }
        }
        if let Some(after) = cells.get_mut(end..) {
            for cell in after.iter_mut().take_while(|cell| matches!(cell, Cell::Covered)) {
                *cell = Cell::Blank;
            }
        }
        if let Some(covered) = cells.get_mut(col..end)
            && let Some((first, rest)) = covered.split_first_mut()
        {
            *first = Cell::Text(text, style);
            rest.fill(Cell::Covered);
        }
    }

    /// Draws a line through `points`, each `(row, col)`; between consecutive points it
    /// moves vertically to the target row first, then horizontally to its column. Where
    /// the line meets another line the cell becomes a junction; cells holding text are
    /// left untouched.
    pub(crate) fn line(&mut self, points: &[(usize, usize)], glyphs: LineGlyphs, style: Style) {
        for pair in points.windows(2) {
            let &[mut from, to] = pair else { continue };
            while from != to {
                let (next, towards, back) = step(from, to);
                self.connect(from, towards, glyphs, style);
                self.connect(next, back, glyphs, style);
                from = next;
            }
        }
    }

    fn connect(
        &mut self,
        (row, col): (usize, usize),
        towards: u8,
        glyphs: LineGlyphs,
        style: Style,
    ) {
        let Some(cell) = self.cells(row, col + 1).and_then(|cells| cells.get_mut(col)) else {
            return;
        };
        match cell {
            Cell::Blank => *cell = Cell::Line { connections: towards, glyphs, style },
            Cell::Line { connections, .. } => *connections |= towards,
            Cell::Text(..) | Cell::Covered => {}
        }
    }

    /// Grows the canvas to at least `row + 1` rows, so that a row left blank, such as a
    /// text block's border, still becomes a line.
    pub(crate) fn reach_row(&mut self, row: usize) {
        self.cells(row, 0);
    }

    /// Row `row`, grown to at least `len` cells.
    fn cells(&mut self, row: usize, len: usize) -> Option<&mut Vec<Cell<'a>>> {
        if self.rows.len() <= row {
            self.rows.resize_with(row + 1, Vec::new);
        }
        let cells = self.rows.get_mut(row)?;
        if cells.len() < len {
            cells.resize(len, Cell::Blank);
        }
        Some(cells)
    }

    /// The rows as lines without trailing blanks.
    pub(crate) fn into_lines(self) -> Vec<Line> {
        self.rows
            .into_iter()
            .map(|cells| {
                let mut line = Line::new();
                for cell in cells {
                    match cell {
                        Cell::Blank => push(&mut line, " ", Style::default()),
                        Cell::Text(text, style) => push(&mut line, text, style),
                        Cell::Covered => {}
                        Cell::Line { connections, glyphs, style } => {
                            push(&mut line, line_glyph(connections, glyphs), style);
                        }
                    }
                }
                line
            })
            .collect()
    }
}

/// The cell one step from `from` towards `to`, and the connection bits from `from` to
/// it and back.
fn step((row, col): (usize, usize), (to_row, to_col): (usize, usize)) -> ((usize, usize), u8, u8) {
    if row < to_row {
        ((row + 1, col), DOWN, UP)
    } else if row > to_row {
        ((row - 1, col), UP, DOWN)
    } else if col < to_col {
        ((row, col + 1), RIGHT, LEFT)
    } else {
        ((row, col.saturating_sub(1)), LEFT, RIGHT)
    }
}

fn line_glyph(connections: u8, glyphs: LineGlyphs) -> &'static str {
    if connections & (UP | DOWN) == 0 {
        glyphs.horizontal
    } else if connections & (LEFT | RIGHT) == 0 {
        glyphs.vertical
    } else {
        JUNCTIONS.get(usize::from(connections)).copied().unwrap_or(" ")
    }
}

/// Appends `text`, extending the last span when it has the same style.
fn push(line: &mut Line, text: &str, style: Style) {
    match line.last_mut() {
        Some(last) if last.style == style => last.text.push_str(text),
        _ => line.push(Span { text: text.to_owned(), style }),
    }
}

#[cfg(test)]
mod tests {
    use unicode_width::UnicodeWidthStr;

    use super::Canvas;
    use crate::style::Style;

    /// The display width of each line `canvas` turns into.
    fn canvas_line_widths(canvas: Canvas<'_>) -> Vec<usize> {
        canvas
            .into_lines()
            .iter()
            .map(|line| line.iter().map(|span| span.text.width()).sum())
            .collect()
    }

    #[test]
    fn canvas_text_starting_inside_wide_text_keeps_the_row_as_wide_as_its_columns() {
        let mut canvas = Canvas::default();
        canvas.put(0, 0, "あ", Style::default());
        canvas.put(0, 1, "x", Style::default());

        assert_eq!(canvas_line_widths(canvas), [2]);
    }

    #[test]
    fn canvas_text_over_the_first_column_of_wide_text_keeps_the_row_as_wide_as_its_columns() {
        let mut canvas = Canvas::default();
        canvas.put(0, 0, "あ", Style::default());
        canvas.put(0, 0, "x", Style::default());

        assert_eq!(canvas_line_widths(canvas), [2]);
    }
}
