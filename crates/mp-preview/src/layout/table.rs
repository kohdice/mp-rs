//! Tables: column widths, cell padding, and box-drawing borders.

use unicode_width::UnicodeWidthStr;

use super::text::{flatten, push_span, wrap};
use crate::model::{Align, Inline};
use crate::style::{Line, Span, Style};
use crate::theme::solarized::DARK_PALETTE;

/// Lays out a table whose rows all have the header's column count. Each cell is
/// flattened once; that flattened text is both measured and wrapped.
pub(super) fn lay_out_table(
    align: &[Align],
    header: &[Vec<Inline>],
    rows: &[Vec<Vec<Inline>>],
    width: Option<usize>,
) -> Vec<Line> {
    let body = Style { fg: Some(DARK_PALETTE.body), ..Style::default() };
    let mut cells: Vec<Vec<Vec<Line>>> = std::iter::once(header)
        .chain(rows.iter().map(Vec::as_slice))
        .map(|row| row.iter().map(|cell| flatten(cell, body, false)).collect())
        .collect();

    let mut widths = vec![MIN_COLUMN_WIDTH; header.len()];
    for row in &cells {
        for (width, cell) in widths.iter_mut().zip(row) {
            *width = cell.iter().map(|line| line_width(line)).fold(*width, usize::max);
        }
    }
    if let Some(available) = width {
        shrink_to_fit(&mut widths, available);
        for row in &mut cells {
            for (cell, width) in row.iter_mut().zip(&widths) {
                *cell = wrap(cell, *width);
            }
        }
    }

    let mut lines = vec![border_line(&widths, Border::Top)];
    for (index, row) in cells.iter().enumerate() {
        if index > 0 {
            lines.push(border_line(&widths, Border::Middle));
        }
        lines.extend(row_lines(row, &widths, align));
    }
    lines.push(border_line(&widths, Border::Bottom));
    lines
}

/// Display width of the line's complete visible text.
fn line_width(line: &[Span]) -> usize {
    match line {
        [span] => span.text.width(),
        spans => spans.iter().map(|span| span.text.as_str()).collect::<String>().width(),
    }
}

fn row_lines(cells: &[Vec<Line>], widths: &[usize], align: &[Align]) -> Vec<Line> {
    let height = cells.iter().map(Vec::len).max().unwrap_or(0);
    (0..height)
        .map(|index| {
            let mut line = vec![border_span("│")];
            for (column, ((cell, width), align)) in cells.iter().zip(widths).zip(align).enumerate()
            {
                if column > 0 {
                    line.push(border_span("│"));
                }
                let content = cell.get(index).map_or(&[][..], Vec::as_slice);
                let padding = width.saturating_sub(line_width(content));
                let (left, right) = match align {
                    Align::Right => (padding, 0),
                    Align::Center => (padding / 2, padding - padding / 2),
                    Align::None | Align::Left => (0, padding),
                };
                push_span(&mut line, &" ".repeat(left + 1), Style::default());
                for span in content {
                    push_span(&mut line, &span.text, span.style);
                }
                push_span(&mut line, &" ".repeat(right + 1), Style::default());
            }
            line.push(border_span("│"));
            line
        })
        .collect()
}

#[derive(Clone, Copy)]
enum Border {
    Top,
    Middle,
    Bottom,
}

fn border_line(widths: &[usize], border: Border) -> Line {
    let (left, join, right) = match border {
        Border::Top => ("┌", "┬", "┐"),
        Border::Middle => ("├", "┼", "┤"),
        Border::Bottom => ("└", "┴", "┘"),
    };
    let mut text = left.to_owned();
    for (index, width) in widths.iter().enumerate() {
        if index > 0 {
            text.push_str(join);
        }
        text.push_str(&"─".repeat(width + 2));
    }
    text.push_str(right);
    vec![border_span(&text)]
}

fn border_span(text: &str) -> Span {
    Span {
        text: text.to_owned(),
        style: Style { fg: Some(DARK_PALETTE.muted), ..Style::default() },
    }
}

/// Columns never shrink below this content width, even when the table then
/// overflows the available width.
const MIN_COLUMN_WIDTH: usize = 3;

/// Total display width of a table whose columns have the given content
/// widths: per column one leading border and two padding spaces, plus the
/// closing border.
fn table_total_width(widths: &[usize]) -> usize {
    widths.iter().sum::<usize>() + 3 * widths.len() + 1
}

/// Shrinks the tallest columns toward the next-tallest level until the table
/// fits in `available`, never dropping a column below the minimum width.
///
/// Lowering tied columns together bounds the number of iterations by the column
/// count, rather than by an arbitrarily wide cell's overflow. For n columns this
/// takes O(n²) time and O(1) additional space because each iteration scans them.
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
            return;
        };
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

#[cfg(test)]
mod tests {
    use super::{shrink_to_fit, table_total_width};

    #[test]
    fn shrinks_the_widest_column_first_to_fit_the_available_width() {
        // Natural widths are [4, 12]; borders and padding add 7, so the table
        // is 23 columns wide. Fitting into 19 must take 4 from the widest.
        let mut widths = [4, 12];

        shrink_to_fit(&mut widths, 19);

        assert_eq!(widths, [4, 8]);
    }

    #[test]
    fn tied_columns_shrink_only_by_the_remaining_overflow() {
        // Natural widths are [10, 10]; borders and padding add 7, so the
        // table is 27 columns wide. Fitting into 26 must shrink only one
        // content column, not every tied widest column.
        let mut widths = [10, 10];

        shrink_to_fit(&mut widths, 26);

        assert_eq!(table_total_width(&widths), 26);
        assert_eq!(
            widths.iter().filter(|width| **width == 10).count(),
            1,
            "one tied column should keep its full width: {widths:?}"
        );
    }

    #[test]
    fn columns_never_shrink_below_the_minimum_width() {
        // Even a 5-column budget cannot push content widths below 3.
        let mut widths = [7, 7];

        shrink_to_fit(&mut widths, 5);

        assert_eq!(widths, [3, 3]);
    }
}
