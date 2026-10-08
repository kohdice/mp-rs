//! A grid of terminal cells that diagrams draw on before becoming lines.

use unicode_width::UnicodeWidthStr;

use crate::style::{Line, Span, Style};

/// Bits naming the neighbours a line cell connects to.
const UP: u8 = 1;
const DOWN: u8 = 2;
const LEFT: u8 = 4;
const RIGHT: u8 = 8;

// The first of each run of Unicode's box-drawing turns and junctions in which the light
// and heavy forms of one shape follow each other (U+250C to U+254B): the corners
// `┌ ┐ └ ┘`, the tees `├ ┤ ┬ ┴` and the cross `┼`.
const DOWN_RIGHT: u32 = 0x250C;
const DOWN_LEFT: u32 = 0x2510;
const UP_RIGHT: u32 = 0x2514;
const UP_LEFT: u32 = 0x2518;
const VERTICAL_RIGHT: u32 = 0x251C;
const VERTICAL_LEFT: u32 = 0x2524;
const DOWN_HORIZONTAL: u32 = 0x252C;
const UP_HORIZONTAL: u32 = 0x2534;
const CROSS: u32 = 0x253C;

/// How far into the run from `┼` (U+253C) to `╋` (U+254B) the cross whose arms are heavy
/// as the connection bits of the index say lies: the run lists its sixteen mixes in an
/// order of its own (`┽` left, `┾` right, `┿` both horizontal arms, `╀` up, …).
const CROSS_OFFSETS: [u32; 16] = [0, 4, 5, 6, 1, 7, 9, 13, 2, 8, 10, 14, 3, 11, 12, 15];

#[derive(Debug, Clone, Copy, Default)]
enum Cell<'a> {
    #[default]
    Blank,
    Text(&'a str, Style),
    /// A column covered by the wider text to its left.
    Covered,
    /// Part of one or more lines, connecting towards the neighbours in `connections`, of
    /// which those in `heavy` by a heavy line. `glyphs` and `style` are those of the
    /// line drawn through the cell last, which shows over the earlier ones.
    Line {
        connections: u8,
        heavy: u8,
        glyphs: LineGlyphs,
        style: Style,
    },
}

/// The glyphs of a straight horizontal and a straight vertical run of one line style,
/// and whether its turns and junctions are heavy.
#[derive(Debug, Clone, Copy)]
pub(super) struct LineGlyphs {
    pub horizontal: char,
    pub vertical: char,
    pub heavy: bool,
}

#[derive(Debug, Default)]
pub(super) struct Canvas<'a> {
    rows: Vec<Vec<Cell<'a>>>,
}

impl<'a> Canvas<'a> {
    /// Writes `text` starting at `col`, covering as many columns as its display width.
    /// Text is placed as one unit rather than per character, so that its columns always
    /// add up to `text.width()`, the same measure the boxes around labels are sized by.
    /// Earlier text cut by either end of the new text is blanked, since what is left of
    /// it would no longer take as many columns as it covers.
    pub(super) fn put(&mut self, row: usize, col: usize, text: &'a str, style: Style) {
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
    /// the line meets another line the cell becomes a junction, light or heavy towards
    /// each neighbour as the line drawn that way is, in this line's style; cells holding
    /// text are left untouched.
    pub(super) fn line(&mut self, points: &[(usize, usize)], glyphs: LineGlyphs, style: Style) {
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
        let heavy_towards = if glyphs.heavy { towards } else { 0 };
        match cell {
            Cell::Blank => {
                *cell = Cell::Line { connections: towards, heavy: heavy_towards, glyphs, style };
            }
            Cell::Line { connections, heavy, glyphs: shown, style: shown_style } => {
                *connections |= towards;
                *heavy |= heavy_towards;
                *shown = glyphs;
                *shown_style = style;
            }
            Cell::Text(..) | Cell::Covered => {}
        }
    }

    /// Grows the canvas to at least `row + 1` rows, so that a row left blank, such as a
    /// text block's border, still becomes a line.
    pub(super) fn reach_row(&mut self, row: usize) {
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
    pub(super) fn into_lines(self) -> Vec<Line> {
        self.rows
            .into_iter()
            .map(|cells| {
                let mut line = Line::new();
                for cell in cells {
                    match cell {
                        Cell::Blank => push(&mut line, " ", Style::default()),
                        Cell::Text(text, style) => push(&mut line, text, style),
                        Cell::Covered => {}
                        Cell::Line { connections, heavy, glyphs, style } => {
                            let glyph = line_glyph(connections, heavy, glyphs);
                            push(&mut line, glyph.encode_utf8(&mut [0; 4]), style);
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

/// The glyph of a line cell connecting towards `connections`, heavy towards those in
/// `heavy`: the straight run of `glyphs` when the cell connects along one axis only,
/// otherwise the turn or junction with each arm light or heavy as asked.
fn line_glyph(connections: u8, heavy: u8, glyphs: LineGlyphs) -> char {
    if connections & (UP | DOWN) == 0 {
        return glyphs.horizontal;
    }
    if connections & (LEFT | RIGHT) == 0 {
        return glyphs.vertical;
    }
    let is_heavy = |side: u8| u32::from(heavy & side != 0);
    let (up, down, left, right) = (is_heavy(UP), is_heavy(DOWN), is_heavy(LEFT), is_heavy(RIGHT));
    // Within each run, a corner adds 1 for a heavy horizontal arm and 2 for a heavy
    // vertical one, and a `┬` or `┴` tee adds 1, 2 and 4 for a heavy left, right and
    // vertical arm. A `├` or `┤` tee lists its mixes as: the side arm heavy, the up arm,
    // the down arm, both vertical arms, up and side, down and side, all three.
    let side_tee = |side: u32| match (up, down, side) {
        (0, 0, 0) => 0,
        (0, 0, _) => 1,
        (_, 0, 0) => 2,
        (0, _, 0) => 3,
        (_, _, 0) => 4,
        (_, 0, _) => 5,
        (0, _, _) => 6,
        _ => 7,
    };
    let code = match connections {
        c if c == DOWN | RIGHT => DOWN_RIGHT + right + 2 * down,
        c if c == DOWN | LEFT => DOWN_LEFT + left + 2 * down,
        c if c == UP | RIGHT => UP_RIGHT + right + 2 * up,
        c if c == UP | LEFT => UP_LEFT + left + 2 * up,
        c if c == UP | DOWN | RIGHT => VERTICAL_RIGHT + side_tee(right),
        c if c == UP | DOWN | LEFT => VERTICAL_LEFT + side_tee(left),
        c if c == DOWN | LEFT | RIGHT => DOWN_HORIZONTAL + left + 2 * right + 4 * down,
        c if c == UP | LEFT | RIGHT => UP_HORIZONTAL + left + 2 * right + 4 * up,
        _ => CROSS + CROSS_OFFSETS.get(usize::from(heavy & 0xF)).copied().unwrap_or(0),
    };
    char::from_u32(code).unwrap_or(' ')
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
