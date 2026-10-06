//! Orienting a [`Scene`] on screen and drawing it with box-drawing glyphs.

use unicode_width::UnicodeWidthStr;

use crate::diagram::canvas::{Canvas, HEAVY_JUNCTIONS, LIGHT_JUNCTIONS, LineGlyphs};
use crate::style::{Line, Span, Style};
use crate::theme::Rgb;
use crate::theme::solarized::DARK_PALETTE;

use super::label::{Label, Run, row_width};
use super::layout::{Axis, box_width, label_reach};
use super::outline::{EndsOn, MarkAt, Outline, Row, outline};
use super::parse::{Body, Direction, Edge, Marker, Node, Stroke};
use super::route::Scene;
use super::styling::Styling;

const LINE: Style = plain_style(DARK_PALETTE.muted);
const TEXT: Style = plain_style(DARK_PALETTE.body);

const fn plain_style(fg: Rgb) -> Style {
    Style {
        fg: Some(fg),
        bg: None,
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

/// Display cells the title row of `title` takes.
pub(super) fn title_width(title: &str) -> usize {
    title.width()
}

/// `drawing` under a row showing `title`, centred over the drawing's widest line with the
/// odd spare cell after it, or from the first column when wider, and a blank row.
pub(super) fn titled(drawing: Vec<Line>, title: &str) -> Vec<Line> {
    let drawing_width = drawing
        .iter()
        .map(|line| line.iter().map(|span| span.text.width()).sum::<usize>())
        .max()
        .unwrap_or(0);
    let indent = drawing_width.saturating_sub(title_width(title)) / 2;
    let title_row = vec![Span { text: format!("{}{title}", " ".repeat(indent)), style: TEXT }];
    [title_row, Vec::new()].into_iter().chain(drawing).collect()
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
        draw_box(&mut canvas, top, left, placed.node, axis);
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
            placed.styling,
        );
    }
    for link in &scene.links {
        let points: Vec<_> =
            link.points.iter().map(|&(main, cross)| frame.cell(main, cross)).collect();
        let style = Style { fg: link.edge.styling.stroke.rgb().or(LINE.fg), ..LINE };
        canvas.line(&points, link_glyphs(link.edge), style);
        if let (Some(head), Some((row, col, heading))) =
            (link.edge.head, arrival(points.iter().rev().copied()))
        {
            canvas.put(row, col, marker_glyph(head, heading), style);
        }
        if let (Some(tail), Some((row, col, heading))) =
            (link.edge.tail, arrival(points.iter().copied()))
        {
            canvas.put(row, col, marker_glyph(tail, heading), style);
        }
    }
    for placed in &scene.labels {
        let (label_main_size, _) = axis.label_size(placed.label);
        let (top, left) = frame.top_left(placed.main, placed.cross, label_main_size);
        let align = match axis {
            Axis::Horizontal => RowAlign::Centred,
            Axis::Vertical => RowAlign::OnLine,
        };
        let text = scene.links.get(placed.link).map_or(TEXT, |link| text_style(link.edge.styling));
        draw_label(&mut canvas, (top, left), placed.label, align, text);
    }
    canvas.into_lines()
}

/// Where a label row narrower than the widest goes.
#[derive(Clone, Copy)]
enum RowAlign {
    /// Centred within the widest row, the odd spare cell after it.
    Centred,
    /// Centred on the column of the vertical line the label lies on, leaning left as
    /// [`label_reach`] does, so that every row covers the line.
    OnLine,
}

/// Draws `label` with its top-left cell at `(top, left)`, one row under another, each
/// placed as `align` says.
fn draw_label<'a>(
    canvas: &mut Canvas<'a>,
    (top, left): (usize, usize),
    label: &'a Label,
    align: RowAlign,
    text: Style,
) {
    let width = label.width();
    for (row, runs) in (top..).zip(label.rows()) {
        let row_width = row_width(runs);
        let offset = match align {
            RowAlign::Centred => (width - row_width) / 2,
            RowAlign::OnLine => label_reach(width).0 - label_reach(row_width).0,
        };
        draw_runs(canvas, (row, left + offset), runs, text);
    }
}

/// Draws `runs` one after another from `(row, col)`, each in `text` with its own bold
/// and italic added.
fn draw_runs<'a>(
    canvas: &mut Canvas<'a>,
    (row, mut col): (usize, usize),
    runs: &'a [Run],
    text: Style,
) {
    for run in runs {
        let emphasis =
            Style { bold: text.bold || run.bold, italic: text.italic || run.italic, ..text };
        canvas.put(row, col, &run.text, emphasis);
        col += run.text.width();
    }
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

/// The glyphs of `edge`'s line: heavy for a thick link and dotted for a dotted one,
/// unless its `stroke-width` or `stroke-dasharray` says otherwise, as upstream's inline
/// style overrides the stroke its link syntax gives.
fn link_glyphs(edge: &Edge) -> LineGlyphs {
    let heavy = edge.styling.heavy.unwrap_or(edge.stroke == Stroke::Thick);
    let dotted = edge.styling.dotted.unwrap_or(edge.stroke == Stroke::Dotted);
    line_glyphs(heavy, dotted)
}

/// The glyphs of a line, heavy or dotted as asked. A heavy line turns and joins in
/// heavy glyphs too, as upstream draws a thick stroke along its whole path and a heavy
/// run meeting a light corner shows a break; the dotted forms exist for straight runs
/// only, so a dotted line turns in solid glyphs.
fn line_glyphs(heavy: bool, dotted: bool) -> LineGlyphs {
    let (horizontal, vertical) = match (heavy, dotted) {
        (false, false) => ("─", "│"),
        (false, true) => ("┄", "┆"),
        (true, false) => ("━", "┃"),
        (true, true) => ("┅", "┇"),
    };
    let junctions = if heavy { &HEAVY_JUNCTIONS } else { &LIGHT_JUNCTIONS };
    LineGlyphs { horizontal, vertical, junctions }
}

/// Draws a frame with the corners `top_left` and `bottom_right` and `title` in its top
/// border, between blanks starting `title_offset` cells right of the corner.
fn draw_subgraph_frame<'a>(
    canvas: &mut Canvas<'a>,
    (top, left): (usize, usize),
    (bottom, right): (usize, usize),
    title: &'a Label,
    title_offset: usize,
    styling: Styling,
) {
    let corners = [(top, left), (top, right), (bottom, right), (bottom, left), (top, left)];
    // Upstream paints `fill` behind a subgraph's members; here the cells inside the frame
    // are those members' boxes, lines and labels, which a background would recolor, so
    // a frame takes no `fill`.
    let border = Style { fg: styling.stroke.rgb().or(LINE.fg), ..LINE };
    let text = text_style(styling);
    let glyphs = line_glyphs(styling.heavy == Some(true), styling.dotted == Some(true));
    canvas.line(&corners, glyphs, border);
    let start = left + title_offset;
    canvas.put(top, start, " ", text);
    // A title is one row (see `Label::joined`).
    for runs in title.rows() {
        draw_runs(canvas, (top, start + 1), runs, text);
    }
    canvas.put(top, start + 1 + title.width(), " ", text);
}

/// Draws `node`'s box with its top-left cell at `(top, left)`, grown by its spread on
/// both sides of the label across the flow of `axis`: extra rows of sides above and
/// below the label rows in a horizontal layout, extra columns of border either side of
/// the label in a vertical one. A hidden node draws nothing, and a drawing is copied
/// in whole.
fn draw_box<'a>(canvas: &mut Canvas<'a>, top: usize, left: usize, node: &'a Node, axis: Axis) {
    let (label, shape) = match &node.body {
        Body::Box { label, shape } => (label, *shape),
        Body::Hidden => return,
        Body::Drawing(drawing) => {
            // Its cells become text, which Canvas::line never overwrites, so no link is
            // drawn through the subgraph.
            for (row, line) in (top..).zip(drawing) {
                let mut col = left;
                for span in line {
                    canvas.put(row, col, &span.text, span.style);
                    col += span.text.width();
                }
            }
            return;
        }
    };
    let (extra_rows, extra_cols) = match axis {
        Axis::Horizontal => (node.spread, 0),
        Axis::Vertical => (0, node.spread),
    };
    let width = box_width(node) + 2 * extra_cols;
    let outline = outline(shape);
    let pen = Pen {
        border: Style { fg: node.styling.stroke.rgb().or(LINE.fg), ..LINE },
        interior: node.styling.fill.rgb().map(|fill| Style { bg: Some(fill), ..Style::default() }),
        heavy: node.styling.heavy == Some(true) && has_heavy_form(&outline),
        dotted: node.styling.dotted == Some(true),
    };
    let text = Style { bg: node.styling.fill.rgb(), ..text_style(node.styling) };
    let rows = outline.label_height(label);
    let first_label_row = top + outline.label_row() + extra_rows;
    let label_rows = first_label_row..first_label_row + rows;
    let below_top = label_rows.end + extra_rows;
    for (row, outline_row) in (top..).zip(outline.above).chain((below_top..).zip(outline.below)) {
        draw_row(canvas, row, left, width, *outline_row, Pen { interior: None, ..pen });
    }
    // The middle rows: the label rows and the rows the box grew by around them. Those
    // that do not carry the label row's own side glyphs continue its sides.
    let middle = first_label_row - extra_rows..below_top;
    let continued = Row {
        ends: outline.continued.unwrap_or(outline.label.ends.map(plain_side)),
        mark: None,
        ..outline.label
    };
    for row in middle.clone() {
        let own = match outline.label_ends_on {
            EndsOn::LabelRows => label_rows.contains(&row),
            EndsOn::LastRow => row + 1 == middle.end,
            EndsOn::PortRow => row == top + outline.middle_label_row(label) + extra_rows,
        };
        draw_row(canvas, row, left, width, if own { outline.label } else { continued }, pen);
    }
    if outline.shows_label() {
        let label_left = left + outline.label_offset() + extra_cols;
        draw_label(canvas, (first_label_row, label_left), label, RowAlign::Centred, text);
    }
}

/// A run of `│` as wide as the side glyphs `end`, but a heavy side stays heavy and an
/// open side open.
fn plain_side(end: &'static str) -> &'static str {
    match end {
        "" | "┃" => end,
        _ if end.width() > 1 => "││",
        _ => "│",
    }
}

/// Draws `outline_row` on a box `width` cells wide from `left`: its end glyphs, its fill
/// between them, and its mark over the fill. Blanks are drawn only where `pen` paints
/// the interior, so an unpainted row leaves no trailing blanks behind.
fn draw_row(
    canvas: &mut Canvas<'_>,
    row: usize,
    left: usize,
    width: usize,
    outline_row: Row,
    pen: Pen,
) {
    let [inset_left, inset_right] = outline_row.inset;
    let [end_left, end_right] = outline_row.ends;
    let start = left + inset_left;
    // The cell after the row's last one, counted from the box's left side.
    let end = match outline_row.reach {
        Some(reach) => (inset_left + reach).min(width.saturating_sub(1)),
        None => width.saturating_sub(inset_right),
    };
    let Some(right_start) = (left + end).checked_sub(end_right.width()) else {
        return;
    };
    canvas.reach_row(row);
    put_glyphs(canvas, row, start, end_left, pen);
    put_glyphs(canvas, row, right_start, end_right, pen);
    for col in start + end_left.width()..right_start {
        put_glyphs(canvas, row, col, outline_row.fill, pen);
    }
    if let Some((at, mark)) = outline_row.mark {
        let col = match at {
            MarkAt::Column(col) => col.min(width.saturating_sub(2)),
            MarkAt::Centre => width.saturating_sub(mark.width()) / 2,
        };
        put_glyphs(canvas, row, left + col, mark, Pen { interior: None, ..pen });
    }
}

/// Draws `glyphs` from `(row, col)` one character at a time in `pen`'s border style, and
/// their blanks only when `pen` paints the interior.
fn put_glyphs(canvas: &mut Canvas<'_>, row: usize, mut col: usize, glyphs: &'static str, pen: Pen) {
    for (at, c) in glyphs.char_indices() {
        // The range comes from `char_indices`, so it lies on char boundaries.
        let glyph = &glyphs[at..at + c.len_utf8()];
        match (c, pen.interior) {
            (' ', Some(interior)) => canvas.put(row, col, glyph, interior),
            (' ', None) => {}
            _ => canvas.put(row, col, pen.glyph(c, glyph), pen.border),
        }
        col += glyph.width();
    }
}

/// How a box's outline is drawn.
#[derive(Debug, Clone, Copy)]
struct Pen {
    /// The style of its glyphs.
    border: Style,
    /// The style of the blanks inside it, which are left undrawn when `None`.
    interior: Option<Style>,
    /// Whether its glyphs are drawn in their heavy forms.
    heavy: bool,
    /// Whether its straight runs are drawn dotted.
    dotted: bool,
}

impl Pen {
    /// The glyph this pen draws for the character `c`, given as the string `glyph`.
    fn glyph(self, c: char, glyph: &'static str) -> &'static str {
        let weighed = if self.heavy { heavy(c).unwrap_or(glyph) } else { glyph };
        if self.dotted { dotted(weighed) } else { weighed }
    }
}

/// The dotted form of a straight line glyph, light or heavy; any other glyph is itself,
/// as the box-drawing glyphs have dotted forms of straight runs only.
fn dotted(glyph: &'static str) -> &'static str {
    match glyph {
        "─" => "┄",
        "━" => "┅",
        "│" => "┆",
        "┃" => "┇",
        _ => glyph,
    }
}

/// The heavy form of the box-drawing glyph `glyph`, which is itself when heavy already;
/// `None` for a glyph with no heavy form, such as a round corner or a diagonal.
fn heavy(glyph: char) -> Option<&'static str> {
    Some(match glyph {
        '─' | '━' => "━",
        '│' | '┃' => "┃",
        '┌' | '┏' => "┏",
        '┐' | '┓' => "┓",
        '└' | '┗' => "┗",
        '┘' | '┛' => "┛",
        '├' | '┣' => "┣",
        '┤' | '┫' => "┫",
        '┬' | '┳' => "┳",
        '┴' | '┻' => "┻",
        '┼' | '╋' => "╋",
        _ => return None,
    })
}

/// Whether every glyph of `outline` has a heavy form. Upstream thickens any outline's
/// stroke; Unicode has no heavy round corners, diagonals, brackets or waves, and a heavy
/// line meeting a light corner shows a break, so a shape drawn with any of them keeps
/// its light glyphs whatever its `stroke-width`.
fn has_heavy_form(outline: &Outline) -> bool {
    let rows = outline.above.iter().chain(outline.below).chain([&outline.label]);
    rows.flat_map(|row| {
        row.ends.into_iter().chain([row.fill]).chain(row.mark.map(|(_, mark)| mark))
    })
    .chain(outline.continued.into_iter().flatten())
    .flat_map(str::chars)
    .all(|c| c == ' ' || heavy(c).is_some())
}

/// The label text style with what `styling` sets: its `color`, bold and italic.
fn text_style(styling: Styling) -> Style {
    Style {
        fg: styling.color.rgb().or(TEXT.fg),
        bold: styling.bold == Some(true),
        italic: styling.italic == Some(true),
        ..TEXT
    }
}
