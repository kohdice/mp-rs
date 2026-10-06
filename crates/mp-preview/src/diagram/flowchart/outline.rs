//! The glyphs each node shape is drawn with, and the cells they take around the label.

use unicode_width::UnicodeWidthStr;

use super::label::Label;
use super::parse::Shape;

/// One row of a box: the blank cells before and after it, the glyphs at its two ends,
/// and the glyph repeated between them — on the label row, a blank, which leaves the
/// label one blank either side. An end may be several cells wide (`┌┬`, `├┘`), and an
/// empty end leaves that side of the row open.
#[derive(Debug, Clone, Copy)]
pub(super) struct Row {
    pub inset: [usize; 2],
    pub ends: [&'static str; 2],
    /// One cell wide; a blank is not drawn.
    pub fill: &'static str,
    /// Cells the row spans from its left inset when it stops short of the box's right
    /// side whatever the box's width, as a folder's tab does; never past the box's last
    /// cell but one. `inset[1]` is then unused.
    pub reach: Option<usize>,
    /// Glyphs drawn over the fill.
    pub mark: Option<(MarkAt, &'static str)>,
}

/// Where a row's mark starts.
#[derive(Debug, Clone, Copy)]
pub(super) enum MarkAt {
    /// A column counted from the box's left side, but never past its last cell but one.
    Column(usize),
    /// Centred on the box, the odd spare cell after it.
    Centre,
}

/// A box drawn as its border rows above the label, a label row for each row of the
/// label, and its border rows below the label.
#[derive(Debug, Clone, Copy)]
pub(super) struct Outline {
    pub above: &'static [Row],
    pub label: Row,
    pub below: &'static [Row],
    /// The box's width whatever its label, which it hides behind a single label row, as
    /// upstream draws no text in these shapes; `None` for a box sized to its label.
    pub fixed_width: Option<usize>,
    /// The side glyphs of the box's middle rows — its label rows and the rows it grew
    /// by — that do not carry the label row's own; `None` for a run of `│` as wide as
    /// each of them, a heavy side staying heavy and an open side open.
    pub continued: Option<[&'static str; 2]>,
    /// Which middle rows carry the label row's side glyphs.
    pub label_ends_on: EndsOn,
}

/// The middle rows of a box that carry its label row's side glyphs.
#[derive(Debug, Clone, Copy)]
pub(super) enum EndsOn {
    /// Every label row.
    LabelRows,
    /// The last middle row only: the stacked shapes' second sheet, one row higher than
    /// the front one, ends beside it.
    LastRow,
    /// The middle label row, which links meet: a brace's one point.
    PortRow,
}

impl Outline {
    /// Whether the box draws its label.
    pub(super) fn shows_label(&self) -> bool {
        self.fixed_width.is_none()
    }

    /// Label rows the box takes for `label`: one when it hides the label.
    pub(super) fn label_height(&self, label: &Label) -> usize {
        if self.shows_label() { label.height() } else { 1 }
    }

    /// Rows the box takes around `label`.
    pub(super) fn height(&self, label: &Label) -> usize {
        self.above.len() + self.label_height(label) + self.below.len()
    }

    /// Columns the box takes around `label`: its fixed width when it has one, whatever
    /// the label, and otherwise the label's width plus the cells its sides take.
    pub(super) fn width(&self, label: &Label) -> usize {
        self.fixed_width.unwrap_or_else(|| label.width() + self.padding())
    }

    /// Rows from the box's top row to its first label row.
    pub(super) fn label_row(&self) -> usize {
        self.above.len()
    }

    /// Rows from the box's top row to its middle label row (see [`Label::middle_row`]).
    pub(super) fn middle_label_row(&self, label: &Label) -> usize {
        self.label_row() + if self.shows_label() { label.middle_row() } else { 0 }
    }

    /// Cells from the box's left edge to its label.
    pub(super) fn label_offset(&self) -> usize {
        self.label.inset[0] + self.label.ends[0].width() + 1
    }

    /// Blank cells before the top border or the bottom one, whichever starts further in:
    /// the columns at either end of the box that links along a vertical flow do not meet.
    pub(super) fn border_inset(&self) -> usize {
        let top = self.above.first().map_or(0, |row| row.inset[0]);
        let bottom = self.below.last().map_or(0, |row| row.inset[0]);
        top.max(bottom)
    }

    /// Cells from the box's bounding edge on its right side when `right`, and on its left
    /// side otherwise, to the label row's side glyph there, where a link along a
    /// horizontal flow ends; none when that side is open, as the edge itself is then the
    /// box's side.
    pub(super) fn side_inset(&self, right: bool) -> usize {
        let [left_inset, right_inset] = self.label.inset;
        let [left_end, right_end] = self.label.ends;
        let (inset, end) = if right { (right_inset, right_end) } else { (left_inset, left_end) };
        if end.is_empty() { 0 } else { inset }
    }

    /// Cells the box takes beyond its label's width.
    fn padding(&self) -> usize {
        self.label_offset() + 1 + self.label.ends[1].width() + self.label.inset[1]
    }
}

const fn row(inset: [usize; 2], ends: [&'static str; 2], fill: &'static str) -> Row {
    Row { inset, ends, fill, reach: None, mark: None }
}

/// A border row of `─` between `ends`.
const fn border(inset: [usize; 2], ends: [&'static str; 2]) -> Row {
    row(inset, ends, "─")
}

/// A label row with `ends` at its sides, or a border row left blank between them.
const fn sides(inset: [usize; 2], ends: [&'static str; 2]) -> Row {
    row(inset, ends, " ")
}

/// A box sized to its label, whose label rows all carry the label row's side glyphs and
/// whose other middle rows continue them as plain lines.
const fn boxed(above: &'static [Row], label: Row, below: &'static [Row]) -> Outline {
    Outline {
        above,
        label,
        below,
        fixed_width: None,
        continued: None,
        label_ends_on: EndsOn::LabelRows,
    }
}

const FLUSH: [usize; 2] = [0, 0];
const INSET: [usize; 2] = [1, 1];

/// The rows of a box of `shape`. A shape's feature lies in its end glyphs, in rows
/// inset from the box's sides and in the glyphs between the ends. Upstream draws
/// curves, fills and exact angles, which cells of box-drawing glyphs cannot: a circle
/// differs from a stadium only by its inset top and bottom rows, and a double circle
/// from a circle by its doubled end glyphs.
pub(super) fn outline(shape: Shape) -> Outline {
    match shape {
        Shape::Rectangle => RECTANGLE,
        Shape::Rounded => ROUNDED,
        Shape::Diamond => DIAMOND,
        Shape::Stadium => STADIUM,
        Shape::Subroutine => SUBROUTINE,
        Shape::Hexagon => HEXAGON,
        Shape::Asymmetric => ASYMMETRIC,
        Shape::LeanRight => LEAN_RIGHT,
        Shape::LeanLeft => LEAN_LEFT,
        Shape::Trapezoid => TRAPEZOID,
        Shape::InvTrapezoid => INV_TRAPEZOID,
        Shape::Circle => CIRCLE,
        Shape::DoubleCircle => DOUBLE_CIRCLE,
        Shape::Ellipse => ELLIPSE,
        Shape::Cylinder => CYLINDER,
        Shape::NotchedRectangle => NOTCHED_RECTANGLE,
        Shape::LinedRectangle => LINED_RECTANGLE,
        Shape::DividedRectangle => DIVIDED_RECTANGLE,
        Shape::TaggedRectangle => TAGGED_RECTANGLE,
        Shape::NotchedPentagon => NOTCHED_PENTAGON,
        Shape::SlopedRectangle => SLOPED_RECTANGLE,
        Shape::Delay => DELAY,
        Shape::BowTieRectangle => BOW_TIE_RECTANGLE,
        Shape::CurvedTrapezoid => CURVED_TRAPEZOID,
        Shape::Console => CONSOLE,
        Shape::Browser => BROWSER,
        Shape::Bucket => BUCKET,
        Shape::Document => DOCUMENT,
        Shape::LinedDocument => LINED_DOCUMENT,
        Shape::TaggedDocument => TAGGED_DOCUMENT,
        Shape::Flag => FLAG,
        Shape::Documents => DOCUMENTS,
        Shape::StackedRectangle => STACKED_RECTANGLE,
        Shape::Folder => FOLDER,
        Shape::WindowPane => WINDOW_PANE,
        Shape::HorizontalCylinder => HORIZONTAL_CYLINDER,
        Shape::LinedCylinder => LINED_CYLINDER,
        Shape::Datastore => DATASTORE,
        Shape::Triangle => TRIANGLE,
        Shape::FlippedTriangle => FLIPPED_TRIANGLE,
        Shape::Hourglass => HOURGLASS,
        Shape::Brace => BRACE,
        Shape::BraceRight => BRACE_RIGHT,
        Shape::Braces => BRACES,
        Shape::Bang => BANG,
        Shape::Cloud => CLOUD,
        Shape::Bolt => BOLT,
        Shape::Person => PERSON,
        Shape::Text => TEXT,
        Shape::Fork => FORK,
        Shape::SmallCircle => SMALL_CIRCLE,
        Shape::FramedCircle => FRAMED_CIRCLE,
        Shape::FilledCircle => FILLED_CIRCLE,
        Shape::CrossedCircle => CROSSED_CIRCLE,
    }
}

const RECTANGLE: Outline =
    boxed(&[border(FLUSH, ["┌", "┐"])], sides(FLUSH, ["│", "│"]), &[border(FLUSH, ["└", "┘"])]);

const ROUNDED: Outline =
    boxed(&[border(FLUSH, ["╭", "╮"])], sides(FLUSH, ["│", "│"]), &[border(FLUSH, ["╰", "╯"])]);

const DIAMOND: Outline =
    boxed(&[border(FLUSH, ["╱", "╲"])], sides(FLUSH, ["│", "│"]), &[border(FLUSH, ["╲", "╱"])]);

const STADIUM: Outline =
    boxed(&[border(FLUSH, ["╭", "╮"])], sides(FLUSH, ["(", ")"]), &[border(FLUSH, ["╰", "╯"])]);

const SUBROUTINE: Outline = boxed(
    &[border(FLUSH, ["┌┬", "┬┐"])],
    sides(FLUSH, ["││", "││"]),
    &[border(FLUSH, ["└┴", "┴┘"])],
);

const HEXAGON: Outline =
    boxed(&[border(INSET, ["╱", "╲"])], sides(FLUSH, ["<", ">"]), &[border(INSET, ["╲", "╱"])]);

// The notch's mouth leaves the label row's first cell blank. Upstream's notch (`notch`
// in `rect_left_inv_arrow`) is a quarter of the box's height deep, so it deepens with
// the label's rows; a notch drawn with glyphs is one column deep whatever the height,
// as a deeper one would need diagonal rows of its own.
const ASYMMETRIC: Outline =
    boxed(&[border(FLUSH, ["╲", "┐"])], sides([1, 0], [">", "│"]), &[border(FLUSH, ["╱", "┘"])]);

const LEAN_RIGHT: Outline =
    boxed(&[border([1, 0], ["┌", "┐"])], sides(FLUSH, ["╱", "╱"]), &[border([0, 1], ["└", "┘"])]);

const LEAN_LEFT: Outline =
    boxed(&[border([0, 1], ["┌", "┐"])], sides(FLUSH, ["╲", "╲"]), &[border([1, 0], ["└", "┘"])]);

const TRAPEZOID: Outline =
    boxed(&[border(INSET, ["┌", "┐"])], sides(FLUSH, ["╱", "╲"]), &[border(FLUSH, ["└", "┘"])]);

const INV_TRAPEZOID: Outline =
    boxed(&[border(FLUSH, ["┌", "┐"])], sides(FLUSH, ["╲", "╱"]), &[border(INSET, ["└", "┘"])]);

const CIRCLE: Outline =
    boxed(&[border(INSET, ["╭", "╮"])], sides(FLUSH, ["(", ")"]), &[border(INSET, ["╰", "╯"])]);

const DOUBLE_CIRCLE: Outline = boxed(
    &[border(INSET, ["╭╭", "╮╮"])],
    sides(FLUSH, ["((", "))"]),
    &[border(INSET, ["╰╰", "╯╯"])],
);

// Inset one column further than the circle, so that the two differ.
const ELLIPSE: Outline =
    boxed(&[border([2, 2], ["╭", "╮"])], sides(INSET, ["(", ")"]), &[border([2, 2], ["╰", "╯"])]);

// The lower arc of the ellipse on top needs a row of its own, so a cylinder is one row
// taller than the other shapes and a layer mixing them is less even than upstream's,
// whose arcs take fractions of a text line.
const CYLINDER: Outline = boxed(
    &[border(FLUSH, ["╭", "╮"]), border(FLUSH, ["├", "┤"])],
    sides(FLUSH, ["│", "│"]),
    &[border(FLUSH, ["╰", "╯"])],
);

// Upstream cuts the card's top-left corner at an angle a few pixels deep; a cell holds
// one diagonal glyph, so the cut is one `╱` wide whatever the box's size.
const NOTCHED_RECTANGLE: Outline =
    boxed(&[border([1, 0], ["╱", "┐"])], sides(FLUSH, ["│", "│"]), &[border(FLUSH, ["└", "┘"])]);

/// The inner line along the left side, joining the top and bottom borders.
const LINED_RECTANGLE: Outline =
    boxed(&[border(FLUSH, ["┌┬", "┐"])], sides(FLUSH, ["││", "│"]), &[border(FLUSH, ["└┴", "┘"])]);

/// The inner line along the top as a row of its own under the top border.
const DIVIDED_RECTANGLE: Outline = boxed(
    &[border(FLUSH, ["┌", "┐"]), border(FLUSH, ["├", "┤"])],
    sides(FLUSH, ["│", "│"]),
    &[border(FLUSH, ["└", "┘"])],
);

/// The tag in the cell left of the bottom-right corner, which takes a column of its own
/// past the other rows' right side.
const TAGGED_RECTANGLE: Outline =
    boxed(&[border([0, 1], ["┌", "┐"])], sides([0, 1], ["│", "│"]), &[border(FLUSH, ["└", "╱┘"])]);

// Upstream cuts both top corners of the loop limit at an angle; a cell holds one
// diagonal glyph, so each cut is one `╱` or `╲` wide whatever the box's size.
const NOTCHED_PENTAGON: Outline =
    boxed(&[border(INSET, ["╱", "╲"])], sides(FLUSH, ["│", "│"]), &[border(FLUSH, ["└", "┘"])]);

// Upstream's manual input has a top edge rising across the whole width; cells cannot
// hold a shallow slope, so the rise shows as a left side leaning over two rows.
const SLOPED_RECTANGLE: Outline =
    boxed(&[border([2, 0], ["╱", "┐"])], sides([1, 0], ["╱", "│"]), &[border(FLUSH, ["└", "┘"])]);

// Upstream's half circle on the right is a curve as tall as the box; a column of
// glyphs can only show it as round corners and a `)` beside the label.
const DELAY: Outline =
    boxed(&[border(FLUSH, ["┌", "╮"])], sides(FLUSH, ["│", ")"]), &[border(FLUSH, ["└", "╯"])]);

// Upstream curves both sides the same way, a concave left and a convex right; a column
// of cells cannot curve, so each side shows as one `)` beside the label between round
// corners.
const BOW_TIE_RECTANGLE: Outline =
    boxed(&[border(FLUSH, ["╭", "╮"])], sides(FLUSH, [")", ")"]), &[border(FLUSH, ["╰", "╯"])]);

// Upstream's display has a pointed left side and a right side curved over the box's
// height; cells hold neither a slanted edge nor a curve, so the point shows as `<`
// between diagonals and the curve as `)` between round corners.
const CURVED_TRAPEZOID: Outline =
    boxed(&[border([1, 0], ["╱", "╮"])], sides(FLUSH, ["<", ")"]), &[border([1, 0], ["╲", "╯"])]);

// Upstream's console is a window with a dark title bar; a text row has no room for a
// bar of its own, so a heavy frame tells the console from a plain box.
const CONSOLE: Outline =
    boxed(&[row(FLUSH, ["┏", "┓"], "━")], sides(FLUSH, ["┃", "┃"]), &[row(FLUSH, ["┗", "┛"], "━")]);

// Upstream draws a title bar holding window controls; a text row has room only for the
// border, so one `○` in the top border stands for the controls.
const BROWSER: Outline =
    boxed(&[border(FLUSH, ["┌○", "┐"])], sides(FLUSH, ["│", "│"]), &[border(FLUSH, ["└", "┘"])]);

// Upstream's bucket narrows evenly from its rim to its base; cells cannot slope a side
// over several rows, so the narrowing shows only in the bottom row, inset between
// diagonals.
const BUCKET: Outline =
    boxed(&[border(FLUSH, ["╭", "╮"])], sides(FLUSH, ["│", "│"]), &[border(INSET, ["╲", "╱"])]);

// Upstream's document has a wavy bottom edge; a row of cells cannot curve, so `~` stands
// for the wave.
const DOCUMENT: Outline = Outline { below: &[row(FLUSH, ["└", "┘"], "~")], ..RECTANGLE };

/// The lined process's inner line on a document, whose wave is `~` as for `DOCUMENT`.
const LINED_DOCUMENT: Outline =
    Outline { below: &[row(FLUSH, ["└┴", "┘"], "~")], ..LINED_RECTANGLE };

/// The tagged process's tag on a document, whose wave is `~` as for `DOCUMENT`.
const TAGGED_DOCUMENT: Outline =
    Outline { below: &[row(FLUSH, ["└", "╱┘"], "~")], ..TAGGED_RECTANGLE };

/// A document's wave along the top as well as the bottom.
const FLAG: Outline =
    boxed(&[row(FLUSH, ["┌", "┐"], "~")], sides(FLUSH, ["│", "│"]), &[row(FLUSH, ["└", "┘"], "~")]);

// Upstream offsets the sheets behind a stack by a few pixels; a cell is the least step
// text has, so the second sheet sits one row up and one column right of the box, behind
// it, its right side running down to the last middle row.
const DOCUMENTS: Outline = Outline {
    continued: Some(["│", "││"]),
    label_ends_on: EndsOn::LastRow,
    ..boxed(
        &[border([1, 0], ["┌", "┐"]), border(FLUSH, ["┌┴", "┐│"])],
        sides(FLUSH, ["│", "├┘"]),
        &[row([0, 1], ["└", "┘"], "~")],
    )
};

// The documents' stack with a straight bottom, its sheets one cell apart for the reason
// given at `DOCUMENTS`.
const STACKED_RECTANGLE: Outline =
    Outline { below: &[border([0, 1], ["└", "┘"])], ..DOCUMENTS };

/// Columns from a folder's left side to its tab's right side, unless the box is too
/// narrow for it.
const TAB_RIGHT: usize = 4;

// Upstream's tab spans 38% of the folder's width but no less than 28 pixels, under a
// fraction of a text line; a tab in cells is a row of its own and keeps the five columns
// that make it read as a tab beside any label.
const FOLDER: Outline = boxed(
    &[
        Row { reach: Some(TAB_RIGHT + 1), ..border(FLUSH, ["┌", "┐"]) },
        Row { mark: Some((MarkAt::Column(TAB_RIGHT), "┴")), ..border(FLUSH, ["├", "┐"]) },
    ],
    sides(FLUSH, ["│", "│"]),
    &[border(FLUSH, ["└", "┘"])],
);

/// Internal storage: the lined process's inner line along the left side crossing a
/// divided process's line along the top.
const WINDOW_PANE: Outline = boxed(
    &[border(FLUSH, ["┌┬", "┐"]), border(FLUSH, ["├┼", "┤"])],
    sides(FLUSH, ["││", "│"]),
    &[border(FLUSH, ["└┴", "┘"])],
);

// Upstream lies the cylinder on its side, with an ellipse for its left end and a half
// one for its right; a column of cells cannot curve, so the ellipse shows as `(` beside
// an inner line and the half one as `)` between round corners.
const HORIZONTAL_CYLINDER: Outline =
    boxed(&[border(FLUSH, ["╭┬", "╮"])], sides(FLUSH, ["(│", ")"]), &[border(FLUSH, ["╰┴", "╯"])]);

// Upstream draws a second arc under the top ellipse; the cylinder's own arc row shows it
// as a double line instead, as two arcs would take two rows.
const LINED_CYLINDER: Outline = boxed(
    &[border(FLUSH, ["╭", "╮"]), row(FLUSH, ["╞", "╡"], "═")],
    sides(FLUSH, ["│", "│"]),
    &[border(FLUSH, ["╰", "╯"])],
);

// Upstream's data store is two thin horizontal lines with open sides; heavy lines keep
// the two rows apart from a link's light line running beside the box.
const DATASTORE: Outline =
    boxed(&[row(FLUSH, ["", ""], "━")], sides(INSET, ["", ""]), &[row(FLUSH, ["", ""], "━")]);

// Upstream's triangle narrows to a point over the box's height; three rows of cells
// show it as a flat top inset two columns over sides inset one, an apex no row is narrow
// enough for.
const TRIANGLE: Outline =
    boxed(&[border([2, 2], ["╱", "╲"])], sides(INSET, ["╱", "╲"]), &[border(FLUSH, ["└", "┘"])]);

// The triangle upside down, its point a flat bottom row for the reason given at
// `TRIANGLE`.
const FLIPPED_TRIANGLE: Outline =
    boxed(&[border(FLUSH, ["┌", "┐"])], sides(INSET, ["╲", "╱"]), &[border([2, 2], ["╲", "╱"])]);

// Upstream's collate is two triangles whose points meet at the box's centre, where the
// label sits; cells cannot hold that point beside a label, so the triangles meet in the
// row under the label: the upper one's sides beside the label, the lower one's below.
const HOURGLASS: Outline = boxed(
    &[border(FLUSH, ["┌", "┐"])],
    sides(FLUSH, ["╲", "╱"]),
    &[border(FLUSH, ["╱", "╲"]), border(FLUSH, ["└", "┘"])],
);

// Upstream's comment is a curly brace as tall as the label, with no box; a column of
// cells cannot curve, so the brace's two curls show as round corners and its one point
// as a junction beside the middle label row.
const BRACE: Outline = Outline {
    label_ends_on: EndsOn::PortRow,
    ..boxed(&[sides(FLUSH, ["╭", ""])], sides([0, 1], ["┤", ""]), &[sides(FLUSH, ["╰", ""])])
};

// The comment's brace on the right, drawn with glyphs for the reason given at `BRACE`.
const BRACE_RIGHT: Outline = Outline {
    label_ends_on: EndsOn::PortRow,
    ..boxed(&[sides(FLUSH, ["", "╮"])], sides([1, 0], ["", "├"]), &[sides(FLUSH, ["", "╯"])])
};

// The comment's brace on both sides, drawn with glyphs for the reason given at `BRACE`.
const BRACES: Outline = Outline {
    label_ends_on: EndsOn::PortRow,
    ..boxed(&[sides(FLUSH, ["╭", "╮"])], sides(FLUSH, ["┤", "├"]), &[sides(FLUSH, ["╰", "╯"])])
};

// Upstream's bang is an explosion of spikes all round; cells hold no spike at an
// arbitrary angle, so `^` and `v` point out of the top and bottom borders and `>` and
// `<` out of the sides.
const BANG: Outline =
    boxed(&[row(FLUSH, ["╲", "╱"], "^")], sides(FLUSH, [">", "<"]), &[row(FLUSH, ["╱", "╲"], "v")]);

// Upstream's cloud is a ring of arcs; a row of cells cannot curve, so the arcs show as
// waves along the top and bottom and as `(` and `)` at the sides.
const CLOUD: Outline =
    boxed(&[row(FLUSH, ["╭", "╮"], "~")], sides(FLUSH, ["(", ")"]), &[row(FLUSH, ["╰", "╯"], "~")]);

// Upstream's com link is a zigzag lightning bolt; cells hold no edge at its angles, so a
// box whose sides zigzag once, all leaning the same way, stands for it.
const BOLT: Outline =
    boxed(&[border([1, 0], ["╲", "╲"])], sides(FLUSH, ["╱", "╱"]), &[border([1, 0], ["╲", "╲"])]);

// Upstream's person is a round head over the curved shoulders of a body; cells cannot
// draw a figure, so a head two cells wide sits on a rounded box, joined to its top
// border.
const PERSON: Outline = boxed(
    &[
        Row { mark: Some((MarkAt::Centre, "╭╮")), ..sides(FLUSH, ["", ""]) },
        Row { mark: Some((MarkAt::Centre, "┴┴")), ..border(FLUSH, ["╭", "╮"]) },
    ],
    sides(FLUSH, ["│", "│"]),
    &[border(FLUSH, ["╰", "╯"])],
);

// Upstream's text block is the label alone, padded inside an invisible box; blank
// border rows keep that padding as cells, so links meet the block just outside them.
const TEXT: Outline =
    boxed(&[sides(FLUSH, ["", ""])], sides(FLUSH, ["", ""]), &[sides(FLUSH, ["", ""])]);

/// Columns of a fork or join bar.
const FORK_WIDTH: usize = 8;
/// Columns of a small circle.
const SMALL_CIRCLE_WIDTH: usize = 5;

// Upstream's fork or join is a filled bar 70 by 10 pixels, laid across the flow (upright
// in `LR` and `RL`); block glyphs fill a bar `FORK_WIDTH` cells long, about a box's
// width as 70 pixels is, and the half blocks above and below keep it thinner than a box
// three rows tall. It lies flat in every direction, as an upright bar cells wide enough
// to tell from a line would be as tall as several boxes.
const FORK: Outline = Outline {
    fixed_width: Some(FORK_WIDTH),
    ..boxed(&[row(FLUSH, ["", ""], "▄")], row(FLUSH, ["", ""], "█"), &[row(FLUSH, ["", ""], "▀")])
};

// Upstream's start is a circle 14 pixels across, about a text line; three rows are the
// fewest that draw a closed circle in cells, and `SMALL_CIRCLE_WIDTH` columns the fewest
// that leave a cell inside it.
const SMALL_CIRCLE: Outline = Outline {
    fixed_width: Some(SMALL_CIRCLE_WIDTH),
    ..boxed(&[border(INSET, ["╭", "╮"])], sides(FLUSH, ["(", ")"]), &[border(INSET, ["╰", "╯"])])
};

// Upstream's stop is a ring around a filled circle; `●`, one cell wide, stands for the
// inner circle.
const FRAMED_CIRCLE: Outline = Outline {
    label: Row { mark: Some((MarkAt::Centre, "●")), ..sides(FLUSH, ["(", ")"]) },
    ..SMALL_CIRCLE
};

// Upstream fills the junction's circle smoothly; a cell is filled whole or not at all, so
// block glyphs fill the cells inside the small circle.
const FILLED_CIRCLE: Outline = Outline { label: row(FLUSH, ["(", ")"], "█"), ..SMALL_CIRCLE };

// Upstream's summary crosses its circle with two diagonals reaching its edge; `╳`, one
// cell wide, stands for them.
const CROSSED_CIRCLE: Outline = Outline {
    label: Row { mark: Some((MarkAt::Centre, "╳")), ..sides(FLUSH, ["(", ")"]) },
    ..SMALL_CIRCLE
};
