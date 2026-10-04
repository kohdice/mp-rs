//! Orienting a [`Scene`] on screen and drawing it with box-drawing glyphs.

use unicode_width::UnicodeWidthStr;

use crate::diagram::canvas::{Canvas, LineGlyphs};
use crate::style::{Line, Style};
use crate::theme::Rgb;
use crate::theme::solarized::DARK_PALETTE;

use super::layout::{Axis, box_width};
use super::outline::{Row, outline};
use super::parse::{Direction, Marker, Node, Stroke};
use super::route::Scene;

const LINE: Style = plain_style(DARK_PALETTE.muted);
const TEXT: Style = plain_style(DARK_PALETTE.body);

const fn plain_style(fg: Rgb) -> Style {
    Style {
        fg: Some(fg),
        bold: false,
        dim: false,
        italic: false,
        underline: false,
        strikethrough: false,
    }
}

/// Maps `(main, cross)` scene cells to `(row, col)` screen cells for `direction`.
struct Frame {
    direction: Direction,
    /// Cells the scene spans along the flow, mirrored for `RL` and `BT`.
    main_extent: usize,
}

impl Frame {
    fn cell(&self, main: usize, cross: usize) -> (usize, usize) {
        let main = if self.direction.points_backward() {
            self.main_extent.saturating_sub(main + 1)
        } else {
            main
        };
        match self.direction.axis() {
            Axis::Horizontal => (cross, main),
            Axis::Vertical => (main, cross),
        }
    }

    /// The top-left screen cell of a rectangle given by its first scene cell and its
    /// size along and across the flow.
    fn top_left(&self, main: usize, cross: usize, main_size: usize) -> (usize, usize) {
        let near_end = if self.direction.points_backward() {
            (main + main_size).saturating_sub(1)
        } else {
            main
        };
        self.cell(near_end, cross)
    }
}

pub(super) fn draw(scene: &Scene<'_>, direction: Direction) -> Vec<Line> {
    let axis = direction.axis();
    let (main_extent, _) = scene.extent(axis);
    let frame = Frame { direction, main_extent };

    // Boxes and frames go down before links: `Canvas::line` never overwrites text
    // cells, so boxes and frame titles stay whole where a link would cross them.
    let mut canvas = Canvas::default();
    for placed in &scene.boxes {
        let (top, left) =
            frame.top_left(placed.main, placed.cross, axis.box_main_size(placed.node));
        draw_box(&mut canvas, top, left, placed.node);
    }
    for placed in &scene.frames {
        let (row_a, col_a) = frame.cell(*placed.main.start(), *placed.cross.start());
        let (row_b, col_b) = frame.cell(*placed.main.end(), *placed.cross.end());
        draw_subgraph_frame(
            &mut canvas,
            (row_a.min(row_b), col_a.min(col_b)),
            (row_a.max(row_b), col_a.max(col_b)),
            placed.title,
            placed.title_offset,
        );
    }
    for link in &scene.links {
        let points: Vec<_> =
            link.points.iter().map(|&(main, cross)| frame.cell(main, cross)).collect();
        canvas.line(&points, line_glyphs(link.edge.stroke), LINE);
        if let (Some(head), Some((row, col, heading))) =
            (link.edge.head, arrival(points.iter().rev().copied()))
        {
            canvas.put(row, col, marker_glyph(head, heading), LINE);
        }
        if let (Some(tail), Some((row, col, heading))) =
            (link.edge.tail, arrival(points.iter().copied()))
        {
            canvas.put(row, col, marker_glyph(tail, heading), LINE);
        }
    }
    for label in &scene.labels {
        let (label_main_size, _) = axis.label_size(label.text);
        let (row, col) = frame.top_left(label.main, label.cross, label_main_size);
        canvas.put(row, col, label.text, TEXT);
    }
    canvas.into_lines()
}

/// The end cell of a line whose points are listed from that end, and the direction the
/// line arrives at it in.
fn arrival(mut points: impl Iterator<Item = (usize, usize)>) -> Option<(usize, usize, Direction)> {
    let (row, col) = points.next()?;
    let (from_row, from_col) = points.find(|&point| point != (row, col))?;
    let heading = if col > from_col {
        Direction::LeftToRight
    } else if col < from_col {
        Direction::RightToLeft
    } else if row > from_row {
        Direction::TopDown
    } else {
        Direction::BottomUp
    };
    Some((row, col, heading))
}

/// The glyph of `marker` at a link end, with an arrowhead pointing `heading`.
fn marker_glyph(marker: Marker, heading: Direction) -> &'static str {
    match marker {
        Marker::Arrow => match heading {
            Direction::LeftToRight => "►",
            Direction::RightToLeft => "◄",
            Direction::TopDown => "▼",
            Direction::BottomUp => "▲",
        },
        Marker::Circle => "○",
        Marker::Cross => "×",
    }
}

fn line_glyphs(stroke: Stroke) -> LineGlyphs {
    let (horizontal, vertical) = match stroke {
        Stroke::Solid => ("─", "│"),
        Stroke::Dotted => ("┄", "┆"),
        Stroke::Thick => ("━", "┃"),
    };
    LineGlyphs { horizontal, vertical }
}

/// Draws a frame with the corners `top_left` and `bottom_right` and `title` in its top
/// border, between blanks starting `title_offset` cells right of the corner.
fn draw_subgraph_frame<'a>(
    canvas: &mut Canvas<'a>,
    (top, left): (usize, usize),
    (bottom, right): (usize, usize),
    title: &'a str,
    title_offset: usize,
) {
    let corners = [(top, left), (top, right), (bottom, right), (bottom, left), (top, left)];
    canvas.line(&corners, line_glyphs(Stroke::Solid), LINE);
    let start = left + title_offset;
    canvas.put(top, start, " ", TEXT);
    canvas.put(top, start + 1, title, TEXT);
    canvas.put(top, start + 1 + title.width(), " ", TEXT);
}

fn draw_box<'a>(canvas: &mut Canvas<'a>, top: usize, left: usize, node: &'a Node) {
    let width = box_width(node);
    let outline = outline(node.shape);
    let bottom = top + outline.height() - 1;
    for (row, outline_row) in [(top, outline.top), (bottom, outline.bottom)] {
        if let Some((start, end)) = draw_row_ends(canvas, row, left, width, outline_row) {
            for col in start..end {
                canvas.put(row, col, "─", LINE);
            }
        }
    }
    let label_row = top + outline.label_row();
    draw_row_ends(canvas, label_row, left, width, outline.label);
    canvas.put(label_row, left + outline.label_offset(), &node.label, TEXT);
}

/// Draws the end glyphs of `row` on a box `width` cells wide from `left`, and returns
/// the cells between them.
fn draw_row_ends(
    canvas: &mut Canvas<'_>,
    row: usize,
    left: usize,
    width: usize,
    outline_row: Row,
) -> Option<(usize, usize)> {
    let [inset_left, inset_right] = outline_row.inset;
    let [end_left, end_right] = outline_row.ends;
    let start = left + inset_left;
    let right_start = (left + width).checked_sub(inset_right + end_right.width())?;
    canvas.put(row, start, end_left, LINE);
    canvas.put(row, right_start, end_right, LINE);
    Some((start + end_left.width(), right_start))
}
