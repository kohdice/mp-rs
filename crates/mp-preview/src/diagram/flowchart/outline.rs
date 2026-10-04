//! The glyphs each node shape is drawn with, and the cells they take around the label.

use unicode_width::UnicodeWidthStr;

use super::parse::Shape;

/// One row of a box: the blank cells before and after it, the glyphs at its two ends,
/// and between them `─` on a border row or the label, one blank either side, on the
/// label row.
#[derive(Debug, Clone, Copy)]
pub(super) struct Row {
    pub inset: [usize; 2],
    pub ends: [&'static str; 2],
}

/// A box drawn as a border row, the label row and a border row.
#[derive(Debug, Clone, Copy)]
pub(super) struct Outline {
    pub top: Row,
    pub label: Row,
    pub bottom: Row,
}

impl Outline {
    /// Rows the box takes.
    pub(super) fn height(&self) -> usize {
        3
    }

    /// Rows from the box's top row to its label row.
    pub(super) fn label_row(&self) -> usize {
        1
    }

    /// Cells from the box's left edge to its label.
    pub(super) fn label_offset(&self) -> usize {
        self.label.inset[0] + self.label.ends[0].width() + 1
    }

    /// Cells the box takes beyond its label's width.
    pub(super) fn padding(&self) -> usize {
        self.label_offset() + 1 + self.label.ends[1].width() + self.label.inset[1]
    }
}

/// The rows of a box of `shape`. A shape's feature lies in its end glyphs and in rows
/// inset from the box's sides, so that every shape keeps the rectangle's three rows.
pub(super) fn outline(shape: Shape) -> Outline {
    const fn row(inset: [usize; 2], ends: [&'static str; 2]) -> Row {
        Row { inset, ends }
    }
    const FLUSH: [usize; 2] = [0, 0];
    const INSET: [usize; 2] = [1, 1];
    let [top, label, bottom] = match shape {
        Shape::Rectangle => {
            [row(FLUSH, ["┌", "┐"]), row(FLUSH, ["│", "│"]), row(FLUSH, ["└", "┘"])]
        }
        Shape::Rounded => [row(FLUSH, ["╭", "╮"]), row(FLUSH, ["│", "│"]), row(FLUSH, ["╰", "╯"])],
        Shape::Diamond => [row(FLUSH, ["╱", "╲"]), row(FLUSH, ["│", "│"]), row(FLUSH, ["╲", "╱"])],
        Shape::Stadium => [row(FLUSH, ["╭", "╮"]), row(FLUSH, ["(", ")"]), row(FLUSH, ["╰", "╯"])],
        Shape::Subroutine => {
            [row(FLUSH, ["┌┬", "┬┐"]), row(FLUSH, ["││", "││"]), row(FLUSH, ["└┴", "┴┘"])]
        }
        Shape::Hexagon => [row(INSET, ["╱", "╲"]), row(FLUSH, ["<", ">"]), row(INSET, ["╲", "╱"])],
        // The notch's mouth leaves the label row's first cell blank.
        Shape::Asymmetric => {
            [row(FLUSH, ["╲", "┐"]), row([1, 0], [">", "│"]), row(FLUSH, ["╱", "┘"])]
        }
        Shape::LeanRight => {
            [row([1, 0], ["┌", "┐"]), row(FLUSH, ["╱", "╱"]), row([0, 1], ["└", "┘"])]
        }
        Shape::LeanLeft => {
            [row([0, 1], ["┌", "┐"]), row(FLUSH, ["╲", "╲"]), row([1, 0], ["└", "┘"])]
        }
        Shape::Trapezoid => {
            [row(INSET, ["┌", "┐"]), row(FLUSH, ["╱", "╲"]), row(FLUSH, ["└", "┘"])]
        }
        Shape::InvTrapezoid => {
            [row(FLUSH, ["┌", "┐"]), row(FLUSH, ["╲", "╱"]), row(INSET, ["└", "┘"])]
        }
        Shape::Circle => [row(INSET, ["╭", "╮"]), row(FLUSH, ["(", ")"]), row(INSET, ["╰", "╯"])],
        Shape::DoubleCircle => {
            [row(INSET, ["╭╭", "╮╮"]), row(FLUSH, ["((", "))"]), row(INSET, ["╰╰", "╯╯"])]
        }
    };
    Outline { top, label, bottom }
}
