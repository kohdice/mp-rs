//! Text drawings of diagrams written in a ```` ```mermaid ```` code block.

mod canvas;
mod flowchart;

use crate::style::Line;

/// Why a Mermaid block is shown as its source instead of a drawing.
#[derive(Debug)]
pub(crate) enum Failure {
    /// Valid Mermaid that is not drawn: a feature without a text drawing, or a drawing
    /// that does not fit the width or the size limit. There is no syntax to fix.
    Unsupported,
    /// Input that is not valid Mermaid.
    Syntax(SyntaxError),
}

/// Input that is not valid Mermaid. Displays as `line N: message`, or as the message
/// alone when no line is to blame.
#[derive(Debug, thiserror::Error)]
#[error("{}", located(*.line, .message))]
pub(crate) struct SyntaxError {
    /// 1-based line within the diagram source.
    pub line: Option<usize>,
    /// What is wrong, shown after the line number.
    pub message: String,
}

fn located(line: Option<usize>, message: &str) -> String {
    match line {
        Some(line) => format!("line {line}: {message}"),
        None => message.to_owned(),
    }
}

/// Draws the Mermaid `source` as lines without trailing spaces, in at most `width`
/// columns when given.
///
/// A source longer than Mermaid's `maxTextSize` as Mermaid counts it is drawn as the
/// one-box chart Mermaid draws in its place. Returns [`Failure::Syntax`] when the source
/// is not valid Mermaid or has more edges than Mermaid allows, and
/// [`Failure::Unsupported`] for valid input that has no text drawing: another
/// diagram type, a feature that is not drawn, an empty chart, subgraph frames that would
/// cover a box or frame outside them, long links passing through too many layers, or a
/// drawing that fits neither `width` nor the cell limit at any spacing.
pub(crate) fn render_mermaid(source: &str, width: Option<usize>) -> Result<Vec<Line>, Failure> {
    flowchart::render(source, width)
}

#[cfg(test)]
mod tests {
    use super::render_mermaid;
    use crate::ansi::to_ansi;
    use crate::style::{ColorMode, Line, Span, Style};
    use crate::theme::Rgb;
    use crate::theme::solarized::DARK_PALETTE;

    use std::ops::Range;

    use unicode_width::UnicodeWidthStr;

    fn mermaid(body: &str) -> String {
        to_ansi(&mermaid_lines(body), ColorMode::Plain)
    }

    /// The row of the box whose label row is `│ label │`, with one blank or more on
    /// either side of the label in a box grown wider than it, and the display columns of
    /// its left and right borders.
    fn box_of(output: &str, label: &str) -> Option<(usize, usize, usize)> {
        output.lines().enumerate().find_map(|(row, line)| {
            line.match_indices(label).find_map(|(at, _)| {
                let before = line.get(..at)?;
                let after = line.get(at + label.len()..)?;
                let (open, close) = (before.trim_end_matches(' '), after.trim_start_matches(' '));
                let padded = open.len() < before.len() && close.len() < after.len();
                (padded && open.ends_with('│') && close.starts_with('│')).then(|| {
                    let left = open.width() - 1;
                    (row, left, before.width() + label.width() + after.len() - close.len())
                })
            })
        })
    }

    const RED: Rgb = Rgb { r: 255, g: 0, b: 0 };
    const GREEN: Rgb = Rgb { r: 0, g: 255, b: 0 };
    const BLUE: Rgb = Rgb { r: 0, g: 0, b: 255 };

    /// The drawing of the flowchart `body`, as styled lines.
    fn mermaid_lines(body: &str) -> Vec<Line> {
        render_mermaid(body, None).unwrap_or_else(|failure| panic!("{failure:?}"))
    }

    /// The first span of `lines` whose text contains `text`.
    fn span_containing<'a>(lines: &'a [Line], text: &str) -> Option<&'a Span> {
        lines.iter().flatten().find(|span| span.text.contains(text))
    }

    /// Each row of `lines` as its characters with the style of the span holding each.
    fn cells(lines: &[Line]) -> Vec<Vec<(char, Style)>> {
        lines
            .iter()
            .map(|line| {
                line.iter().flat_map(|span| span.text.chars().map(|c| (c, span.style))).collect()
            })
            .collect()
    }

    /// The row, column (counted in characters), character and style of each character of
    /// `lines` from the box-drawing, block and geometric-shape blocks: the glyphs of the
    /// boxes and lines.
    fn glyph_cells(lines: &[Line]) -> Vec<(usize, usize, char, Style)> {
        cells(lines)
            .into_iter()
            .enumerate()
            .flat_map(|(row, cells)| {
                cells
                    .into_iter()
                    .enumerate()
                    .filter(|&(_, (c, _))| ('\u{2500}'..='\u{25ff}').contains(&c))
                    .map(move |(col, (c, style))| (row, col, c, style))
            })
            .collect()
    }

    /// The styles of the glyphs of `lines` (see [`glyph_cells`]) in `rows` and `cols`.
    fn glyph_styles(lines: &[Line], rows: Range<usize>, cols: Range<usize>) -> Vec<Style> {
        glyph_cells(lines)
            .into_iter()
            .filter(|(row, col, ..)| rows.contains(row) && cols.contains(col))
            .map(|(.., style)| style)
            .collect()
    }

    /// Whether `styles` is not empty and every one has the foreground `fg`.
    fn all_fg(styles: &[Style], fg: Rgb) -> bool {
        !styles.is_empty() && styles.iter().all(|style| style.fg == Some(fg))
    }

    /// The rows the box showing `label` spans in `output`, its borders included.
    fn box_rows(output: &str, label: &str) -> Option<Range<usize>> {
        let (row, left, _) = box_of(output, label)?;
        let rows: Vec<&str> = output.lines().collect();
        let side = |row: usize| rows.get(row).and_then(|line| line.chars().nth(left));
        let top = (0..=row).rev().find(|&row| side(row) == Some('┌'))?;
        let bottom = (row..rows.len()).find(|&row| side(row) == Some('└'))?;
        Some(top..bottom + 1)
    }

    /// The rows and columns of the box showing `label` in `output`, its borders included.
    fn box_cells(output: &str, label: &str) -> (Range<usize>, Range<usize>) {
        let cols = box_of(output, label).map(|(_, left, right)| left..right + 1);
        box_rows(output, label).zip(cols).unwrap_or_else(|| panic!("no box {label} in\n{output}"))
    }

    /// The styles of the glyphs of A's box, of the link between the boxes on A's rows and
    /// of B's box in the drawing of `body`, a chart of `A --> B` laid out left to right.
    fn a_to_b_glyph_styles(body: &str) -> [Vec<Style>; 3] {
        let (output, lines) = (mermaid(body), mermaid_lines(body));
        let ((a_rows, a_cols), (b_rows, b_cols)) =
            (box_cells(&output, "A"), box_cells(&output, "B"));
        [
            glyph_styles(&lines, a_rows.clone(), a_cols.clone()),
            glyph_styles(&lines, a_rows, a_cols.end..b_cols.start),
            glyph_styles(&lines, b_rows, b_cols),
        ]
    }

    #[test]
    fn mermaid_style_stroke_colors_the_box_border() {
        let [a, link, b] =
            a_to_b_glyph_styles("flowchart LR\n    A --> B\n    style A stroke:#ff0000\n");

        assert!(all_fg(&a, RED), "{a:?}");
        assert!(all_fg(&link, DARK_PALETTE.muted), "{link:?}");
        assert!(all_fg(&b, DARK_PALETTE.muted), "{b:?}");
    }

    #[test]
    fn mermaid_style_color_colors_the_label() {
        let lines = mermaid_lines("flowchart LR\n    A --> B\n    style A color:#00ff00\n");

        assert_eq!(span_containing(&lines, "A").map(|span| span.style.fg), Some(Some(GREEN)));
        assert_eq!(
            span_containing(&lines, "B").map(|span| span.style.fg),
            Some(Some(DARK_PALETTE.body))
        );
    }

    /// The styles of every character of `lines` in `rows` and `cols`, blanks included.
    fn styles_in(lines: &[Line], rows: Range<usize>, cols: Range<usize>) -> Vec<Style> {
        cells(lines)
            .iter()
            .skip(rows.start)
            .take(rows.len())
            .flat_map(|row| row.iter().skip(cols.start).take(cols.len()).map(|&(_, style)| style))
            .collect()
    }

    #[test]
    fn mermaid_style_fill_paints_the_box_interior() {
        let body = "flowchart LR\n    A --> B\n    style A fill:#0000ff\n";
        let (output, lines) = (mermaid(body), mermaid_lines(body));
        let Some((row, left, right)) = box_of(&output, "A") else { panic!("{output}") };
        let (rows, cols) = box_cells(&output, "A");

        let interior = styles_in(&lines, row..row + 1, left + 1..right);
        assert_eq!(interior.len(), " A ".len(), "{lines:?}");
        assert!(interior.iter().all(|style| style.bg == Some(BLUE)), "{lines:?}");
        let border = glyph_styles(&lines, rows.clone(), cols.clone());
        assert!(!border.is_empty() && border.iter().all(|style| style.bg.is_none()));
        let beyond = styles_in(&lines, rows, cols.end..usize::MAX);
        assert!(!beyond.is_empty() && beyond.iter().all(|style| style.bg.is_none()));
    }

    #[test]
    fn mermaid_hex_color_with_zero_alpha_is_transparent() {
        let box_styles = |fill: &str| {
            let body = format!(
                "flowchart LR\n    A:::c\n    classDef c fill:#f00\n    style A fill:{fill}\n"
            );
            let (rows, cols) = box_cells(&mermaid(&body), "A");
            styles_in(&mermaid_lines(&body), rows, cols)
        };

        assert!(box_styles("#f00f").iter().any(|style| style.bg.is_some()));
        assert!(box_styles("#f000").iter().all(|style| style.bg.is_none()));
    }

    #[test]
    fn mermaid_style_id_is_one_node_declared_when_new() {
        let body = "flowchart LR\n    A\n    style A,B stroke:#ff0000\n";
        let output = mermaid(body);
        let lines = mermaid_lines(body);

        let (row, left, right) = box_of(&output, "A,B").unwrap_or_else(|| panic!("{output}"));
        let styles = glyph_styles(&lines, row - 1..row + 2, left..right + 1);
        assert!(all_fg(&styles, RED), "{styles:?}");
        let (row, left, right) = box_of(&output, "A").unwrap_or_else(|| panic!("{output}"));
        let a_border = glyph_styles(&lines, row - 1..row + 2, left..right + 1);
        assert!(all_fg(&a_border, DARK_PALETTE.muted), "{output}");
    }

    #[test]
    fn mermaid_later_style_overrides_earlier_property_by_property() {
        let body = "flowchart LR\n    A --> B\n    style A stroke:#ff0000,color:#00ff00\n    style A stroke:#0000ff\n";
        let [a, _, _] = a_to_b_glyph_styles(body);

        assert!(all_fg(&a, BLUE), "{a:?}");
        let label = span_containing(&mermaid_lines(body), "A").map(|span| span.style.fg);
        assert_eq!(label, Some(Some(GREEN)));
    }

    #[test]
    fn mermaid_style_on_a_shaped_node_colors_its_glyphs_but_keeps_their_form() {
        let lines = mermaid_lines(
            "flowchart LR\n    A([x])\n    style A stroke:#ff0000,stroke-width:4px\n",
        );

        assert!(all_fg(&glyph_styles(&lines, 0..lines.len(), 0..usize::MAX), RED), "{lines:?}");
    }

    #[test]
    fn mermaid_triple_colon_attaches_a_class() {
        let [a, _, b] = a_to_b_glyph_styles(
            "flowchart LR\n    A:::hot --> B\n    classDef hot stroke:#ff0000\n",
        );
        assert!(all_fg(&a, RED));
        assert!(all_fg(&b, DARK_PALETTE.muted));

        let [a, _, b] = a_to_b_glyph_styles(
            "flowchart LR\n    A --> B:::hot\n    classDef hot stroke:#ff0000\n",
        );
        assert!(all_fg(&a, DARK_PALETTE.muted));
        assert!(all_fg(&b, RED));
    }

    #[test]
    fn mermaid_class_def_may_name_several_classes_and_class_several_nodes() {
        for (body, b_fg) in [
            (
                "flowchart LR\n    A --> B\n    classDef hot stroke:#ff0000\n    class A hot\n",
                DARK_PALETTE.muted,
            ),
            (
                "flowchart LR\n    A --> B\n    classDef hot,warm stroke:#ff0000\n    class A,B warm\n",
                RED,
            ),
        ] {
            let [a, _, b] = a_to_b_glyph_styles(body);

            assert!(all_fg(&a, RED), "{body}");
            assert!(all_fg(&b, b_fg), "{body}");
        }
    }

    #[test]
    fn mermaid_class_def_may_follow_the_class_statement() {
        let [a, _, _] = a_to_b_glyph_styles(
            "flowchart LR\n    A --> B\n    class A hot\n    classDef hot stroke:#ff0000\n",
        );

        assert!(all_fg(&a, RED));
    }

    #[test]
    fn mermaid_default_class_applies_to_every_node_before_its_classes() {
        let body = "flowchart LR\n    A --> B\n    classDef default stroke:#ff0000,color:#00ff00\n    classDef hot stroke:#0000ff\n    class B hot\n";
        let [a, _, b] = a_to_b_glyph_styles(body);
        let lines = mermaid_lines(body);

        assert!(all_fg(&a, RED));
        assert!(all_fg(&b, BLUE));
        for label in ["A", "B"] {
            let fg = span_containing(&lines, label).map(|span| span.style.fg);
            assert_eq!(fg, Some(Some(GREEN)), "{label}");
        }
    }

    #[test]
    fn mermaid_class_def_node_applies_to_every_node() {
        let [a, _, b] =
            a_to_b_glyph_styles("flowchart LR\n    A --> B\n    classDef node stroke:#ff0000\n");

        assert!(all_fg(&a, RED));
        assert!(all_fg(&b, RED));
    }

    #[test]
    fn mermaid_class_before_its_node_is_declared_attaches_nothing() {
        let [a, _, _] = a_to_b_glyph_styles(
            "flowchart LR\n    class A hot\n    A --> B\n    classDef hot stroke:#ff0000\n",
        );

        assert!(all_fg(&a, DARK_PALETTE.muted));
    }

    #[test]
    fn mermaid_style_overrides_class_property_by_property() {
        let body = "flowchart LR\n    A --> B\n    classDef hot stroke:#ff0000,color:#00ff00\n    class A hot\n    style A stroke:#0000ff\n";
        let [a, _, _] = a_to_b_glyph_styles(body);

        assert!(all_fg(&a, BLUE));
        let label = span_containing(&mermaid_lines(body), "A").map(|span| span.style.fg);
        assert_eq!(label, Some(Some(GREEN)));
    }

    #[test]
    fn mermaid_undefined_class_contributes_nothing() {
        for body in [
            "flowchart LR\n    A --> B\n    class A nosuch\n",
            "flowchart LR\n    A:::nosuch --> B\n",
        ] {
            let [a, _, _] = a_to_b_glyph_styles(body);
            assert!(all_fg(&a, DARK_PALETTE.muted), "{body}");
            let label = span_containing(&mermaid_lines(body), "A").map(|span| span.style.fg);
            assert_eq!(label, Some(Some(DARK_PALETTE.body)), "{body}");
        }
    }

    /// The row, column and style of each `►` of `lines`.
    fn arrowhead_cells(lines: &[Line]) -> Vec<(usize, usize, Style)> {
        glyph_cells(lines)
            .into_iter()
            .filter(|&(.., c, _)| c == '►')
            .map(|(row, col, _, style)| (row, col, style))
            .collect()
    }

    /// The styles of the `►` of `body`'s drawing on the rows of the box showing `label`.
    fn arrowheads_into(body: &str, label: &str) -> Vec<Style> {
        let (rows, _) = box_cells(&mermaid(body), label);
        arrowhead_cells(&mermaid_lines(body))
            .into_iter()
            .filter(|(row, ..)| rows.contains(row))
            .map(|(.., style)| style)
            .collect()
    }

    /// The styles of the glyphs on the row of the `►` into the box showing `to`, from the
    /// right border of the box showing `from` to that arrowhead included: the last leg of
    /// the link between them in `output`, drawn as `lines`.
    fn last_leg(output: &str, lines: &[Line], from: &str, to: &str) -> Vec<Style> {
        let ((_, from_cols), (to_rows, _)) = (box_cells(output, from), box_cells(output, to));
        let Some((row, col, _)) =
            arrowhead_cells(lines).into_iter().find(|(row, ..)| to_rows.contains(row))
        else {
            panic!("no arrowhead into {to} in\n{output}");
        };
        glyph_styles(lines, row..row + 1, from_cols.end..col + 1)
    }

    #[test]
    fn mermaid_link_style_colors_the_indexed_link() {
        let body = "flowchart LR\n    A --> B\n    A --> C\n    linkStyle 1 stroke:#ff0000\n";
        let (output, lines) = (mermaid(body), mermaid_lines(body));

        assert!(all_fg(&last_leg(&output, &lines, "A", "C"), RED), "{lines:?}");
        assert!(all_fg(&last_leg(&output, &lines, "A", "B"), DARK_PALETTE.muted), "{lines:?}");
    }

    #[test]
    fn mermaid_link_style_indexes_follow_group_expansion_order() {
        // The links are A→C, A→D, B→C and B→D, in this order.
        let body = "flowchart LR\n    A & B --> C & D\n    linkStyle 1 stroke:#ff0000\n";
        let red: Vec<(usize, usize, Style)> = arrowhead_cells(&mermaid_lines(body))
            .into_iter()
            .filter(|(.., style)| style.fg == Some(RED))
            .collect();

        assert_eq!(red.len(), 1, "{red:?}");
        let (d_rows, _) = box_cells(&mermaid(body), "D");
        assert!(red.iter().all(|(row, ..)| d_rows.contains(row)), "{red:?}");
        assert!(all_fg(&arrowheads_into(body, "C"), DARK_PALETTE.muted));
    }

    #[test]
    fn mermaid_link_style_accepts_a_list_and_default() {
        let listed = "flowchart LR\n    A --> B\n    A --> C\n    linkStyle 0,1 stroke:#ff0000\n";
        assert!(all_fg(&arrowheads_into(listed, "B"), RED));
        assert!(all_fg(&arrowheads_into(listed, "C"), RED));

        let defaulted = "flowchart LR\n    A --> B\n    A --> C\n    linkStyle default stroke:#ff0000\n    linkStyle 1 stroke:#0000ff\n";
        assert!(all_fg(&arrowheads_into(defaulted, "B"), RED));
        assert!(all_fg(&arrowheads_into(defaulted, "C"), BLUE));
    }

    #[test]
    fn mermaid_later_link_style_replaces_the_earlier_one() {
        let body = "flowchart LR\n    A --> B\n    linkStyle 0 stroke:#ff0000\n    linkStyle 0 stroke-width:1px\n";

        assert!(all_fg(&arrowheads_into(body, "B"), DARK_PALETTE.muted));
    }

    #[test]
    fn mermaid_link_style_color_colors_the_edge_label() {
        let lines =
            mermaid_lines("flowchart LR\n    A -->|yes| B\n    linkStyle 0 color:#00ff00\n");

        let yes = span_containing(&lines, "yes").map(|span| span.style.fg);
        assert_eq!(yes, Some(Some(GREEN)), "{lines:?}");
    }

    #[test]
    fn mermaid_link_style_interpolate_is_ignored() {
        let curve_only = "flowchart LR\n    A --> B\n    linkStyle 0 interpolate basis\n";
        assert!(all_fg(&arrowheads_into(curve_only, "B"), DARK_PALETTE.muted));

        let styled =
            "flowchart LR\n    A --> B\n    linkStyle 0 interpolate basis stroke:#ff0000\n";
        assert!(all_fg(&arrowheads_into(styled, "B"), RED));

        // A curve alone leaves the link's style in place.
        let after_style = "flowchart LR\n    A --> B\n    linkStyle 0 stroke:#ff0000\n    linkStyle 0 interpolate basis\n";
        assert!(all_fg(&arrowheads_into(after_style, "B"), RED));
    }

    #[test]
    fn mermaid_class_on_an_edge_id_colors_the_link() {
        let body =
            "flowchart LR\n    A e1@--> B\n    classDef hot stroke:#ff0000\n    class e1 hot\n";
        let [_, link, _] = a_to_b_glyph_styles(body);

        assert!(all_fg(&link, RED));
    }

    #[test]
    fn mermaid_edge_id_in_a_group_link_names_the_last_source_to_first_target_link() {
        // The links are A→C, A→D, B→C and B→D, in this order.
        let body = "flowchart LR\n    A & B e1@--> C & D\n    class e1 hot\n    classDef hot stroke:#ff0000\n";
        let (output, lines) = (mermaid(body), mermaid_lines(body));
        let red: Vec<(usize, usize)> = glyph_cells(&lines)
            .into_iter()
            .filter(|&(.., style)| style.fg == Some(RED))
            .map(|(row, col, ..)| (row, col))
            .collect();
        // Whether a red line leaves the box showing `label`, from the column right of it.
        let leaves = |label: &str| {
            let (rows, cols) = box_cells(&output, label);
            red.iter().any(|&(row, col)| rows.contains(&row) && col == cols.end)
        };
        let heads = arrowhead_cells(&lines);
        let (into_c, others): (Vec<&(usize, usize, Style)>, Vec<_>) =
            heads.iter().partition(|&&(.., style)| style.fg == Some(RED));
        let (c_rows, _) = box_cells(&output, "C");

        assert!(leaves("B") && !leaves("A"), "{lines:?}");
        assert_eq!(into_c.len(), 1, "{heads:?}");
        assert!(into_c.iter().all(|(row, ..)| c_rows.contains(row)), "{heads:?}");
        assert_eq!(others.len(), 3, "{heads:?}");
        assert!(others.iter().all(|(.., style)| style.fg == Some(DARK_PALETTE.muted)), "{heads:?}");
    }

    #[test]
    fn mermaid_repeated_edge_id_names_the_first_link_only() {
        let body = "flowchart LR\n    A e1@--> B\n    A e1@--> C\n    class e1 hot\n    classDef hot stroke:#ff0000\n";
        let (output, lines) = (mermaid(body), mermaid_lines(body));

        assert!(all_fg(&last_leg(&output, &lines, "A", "B"), RED), "{lines:?}");
        assert!(all_fg(&last_leg(&output, &lines, "A", "C"), DARK_PALETTE.muted), "{lines:?}");
    }

    #[test]
    fn mermaid_link_style_default_overrides_an_edge_class() {
        let body = "flowchart LR\n    A e1@--> B\n    classDef hot stroke:#ff0000\n    class e1 hot\n    linkStyle default stroke:#0000ff\n";
        let [_, link, _] = a_to_b_glyph_styles(body);

        assert!(all_fg(&link, BLUE));
    }

    /// The styles of the glyphs of the frame and of A's box in the drawing of `body`, a
    /// subgraph around the one node `A`: every glyph outside A's box belongs to the frame.
    fn frame_and_box_styles(body: &str) -> (Vec<Style>, Vec<Style>) {
        let (rows, cols) = box_cells(&mermaid(body), "A");
        let (a, frame): (Vec<_>, Vec<_>) = glyph_cells(&mermaid_lines(body))
            .into_iter()
            .partition(|&(row, col, ..)| rows.contains(&row) && cols.contains(&col));
        let styles = |cells: Vec<(usize, usize, char, Style)>| {
            cells.into_iter().map(|(.., style)| style).collect()
        };
        (styles(frame), styles(a))
    }

    #[test]
    fn mermaid_style_on_a_subgraph_colors_its_frame_and_title() {
        let body = "flowchart LR\n    subgraph s [Lib]\n    A\n    end\n    style s stroke:#ff0000,color:#00ff00\n";
        let (frame, a) = frame_and_box_styles(body);

        assert!(all_fg(&frame, RED), "{frame:?}");
        let title = span_containing(&mermaid_lines(body), "Lib").map(|span| span.style.fg);
        assert_eq!(title, Some(Some(GREEN)));
        assert!(all_fg(&a, DARK_PALETTE.muted));
    }

    #[test]
    fn mermaid_class_on_a_subgraph_applies_a_class_def() {
        let (frame, a) = frame_and_box_styles(
            "flowchart LR\n    subgraph s [Lib]\n    A\n    end\n    classDef hot stroke:#ff0000\n    class s hot\n",
        );

        assert!(all_fg(&frame, RED));
        assert!(all_fg(&a, DARK_PALETTE.muted));
    }

    #[test]
    fn mermaid_class_inside_its_own_subgraph_attaches_nothing() {
        let body = "flowchart LR\n    subgraph s [Lib]\n    A\n    class s hot\n    end\n    classDef hot stroke:#ff0000\n";
        let (frame, _) = frame_and_box_styles(body);

        assert!(all_fg(&frame, DARK_PALETTE.muted));
        let title = span_containing(&mermaid_lines(body), "Lib").map(|span| span.style.fg);
        assert_eq!(title, Some(Some(DARK_PALETTE.body)));
    }

    #[test]
    fn mermaid_fill_on_a_subgraph_is_ignored() {
        let body = "flowchart LR\n    subgraph s [Lib]\n    A\n    end\n    style s fill:#0000ff\n";

        assert!(mermaid_lines(body).iter().flatten().all(|span| span.style.bg.is_none()));
    }

    #[test]
    fn mermaid_style_on_a_collapsed_subgraph_styles_its_box() {
        let body = "flowchart LR\n    subgraph one\n    A --> B\n    end\n    one@{ view: collapsed }\n    style one stroke:#ff0000,fill:#0000ff\n";
        let (output, lines) = (mermaid(body), mermaid_lines(body));
        let Some((row, left, right)) = box_of(&output, "one") else { panic!("{output}") };
        let (rows, cols) = box_cells(&output, "one");

        assert!(all_fg(&glyph_styles(&lines, rows, cols), RED), "{lines:?}");
        let interior = styles_in(&lines, row..row + 1, left + 1..right);
        assert!(!interior.is_empty() && interior.iter().all(|style| style.bg == Some(BLUE)));
    }

    #[test]
    fn mermaid_markdown_edge_label_and_subgraph_title_render_bold() {
        let lines = mermaid_lines(
            "flowchart LR\n    subgraph \"`**Two**`\"\n    c(\"`The **cat**\n    in the hat`\") -- \"`Bold **edge label**`\" --> d(\"The dog in the hog\")\n    end\n",
        );

        for bold in ["Two", "cat", "edge label"] {
            let span = lines.iter().flatten().find(|span| span.text == bold);
            assert!(span.is_some_and(|span| span.style.bold), "{bold}: {lines:?}");
        }
    }

    #[test]
    fn mermaid_markdown_node_label_renders_nested_emphasis() {
        let lines = mermaid_lines("flowchart LR\n    A[\"`*a **b** c*`\"]\n");
        let rows = cells(&lines);
        let Some(row) = rows
            .iter()
            .find(|row| row.iter().map(|&(c, _)| c).collect::<String>().contains("a b c"))
        else {
            panic!("missing label in {lines:?}");
        };
        let style_of =
            |glyph: char| row.iter().find(|&&(c, _)| c == glyph).map(|&(_, style)| style);

        for glyph in ['a', 'c'] {
            assert!(
                style_of(glyph).is_some_and(|style| style.italic && !style.bold),
                "{glyph}: {lines:?}"
            );
        }
        assert!(style_of('b').is_some_and(|style| style.italic && style.bold), "{lines:?}");
        for glyph in ['a', 'b', 'c'] {
            assert_eq!(style_of(glyph).map(|style| style.fg), Some(Some(DARK_PALETTE.body)));
        }
    }

    #[test]
    fn mermaid_style_font_weight_and_style_render_the_label_bold_and_italic() {
        let lines = mermaid_lines(
            "flowchart LR\n    A[Note]\n    style A font-weight:bold,font-style:italic\n",
        );
        let span = span_containing(&lines, "Note");

        assert!(span.is_some_and(|span| span.style.bold && span.style.italic), "{lines:?}");
    }

    #[test]
    fn mermaid_style_ending_in_a_hex_color_and_a_semicolon_is_applied() {
        for (with_semicolon, without) in [
            (
                "flowchart LR\n    A\n    style A fill:#f9f;\n",
                "flowchart LR\n    A\n    style A fill:#f9f\n",
            ),
            (
                "flowchart LR\n    A:::c\n    classDef c fill:#f9f;\n",
                "flowchart LR\n    A:::c\n    classDef c fill:#f9f\n",
            ),
        ] {
            assert_eq!(mermaid_lines(with_semicolon), mermaid_lines(without), "{with_semicolon}");
            assert_ne!(
                mermaid_lines(with_semicolon),
                mermaid_lines("flowchart LR\n    A\n"),
                "{with_semicolon}"
            );
        }
    }

    /// The junction glyphs of `lines` where a line crosses straight over another, with
    /// their styles.
    fn crossings(lines: &[Line]) -> Vec<(char, Style)> {
        cells(lines)
            .into_iter()
            .flatten()
            .filter(|(c, _)| "┼┽┾┿╀╁╂╃╄╅╆╇╈╉╊╋".contains(*c))
            .collect()
    }

    #[test]
    fn mermaid_link_crossing_a_frame_keeps_its_weight_and_color_at_the_crossing() {
        // Link 1 is `A ==> C`, which crosses the border of `S` to reach `C`.
        let lines = mermaid_lines(
            "flowchart LR\n    subgraph S\n    B --> C\n    end\n    A ==> C\n    A --> B\n    linkStyle 1 stroke:#f00\n",
        );

        let crossed = crossings(&lines);
        assert!(!crossed.is_empty(), "{lines:?}");
        assert!(
            crossed.iter().any(|&(c, style)| "┿╂".contains(c) && style.fg == Some(RED)),
            "{crossed:?}"
        );
    }

    #[test]
    fn mermaid_filled_shape_open_on_the_right_paints_only_the_label_characters() {
        const YELLOW: Rgb = Rgb { r: 255, g: 255, b: 0 };
        const LABEL: &str = "hello";
        for shape in ["brace", "text", "datastore"] {
            let body = format!(
                "flowchart LR\n    A@{{ shape: {shape}, label: \"{LABEL}\" }}\n    style A fill:#ff0\n"
            );
            let lines =
                super::render_mermaid(&body, None).unwrap_or_else(|error| panic!("{error:?}"));
            let rows = cells(&lines);
            let Some((row, start)) = rows.iter().find_map(|row| {
                let text: String = row.iter().map(|&(c, _)| c).collect();
                text.find(LABEL).map(|at| (row, text[..at].chars().count()))
            }) else {
                panic!("{shape}: missing label in {lines:?}");
            };
            let end = start + LABEL.chars().count();

            assert!(start > 0, "{shape}: no cell left of the label in {lines:?}");
            assert!(
                row[start..end].iter().all(|(_, style)| style.bg == Some(YELLOW)),
                "{shape}: {lines:?}"
            );
            assert!(row[..start].iter().all(|(_, style)| style.bg.is_none()), "{shape}: {lines:?}");
            assert!(row[end..].iter().all(|(_, style)| style.bg.is_none()), "{shape}: {lines:?}");
        }
    }

    #[test]
    fn mermaid_filled_shape_open_on_the_right_leaves_an_empty_label_row_unpainted() {
        let lines = mermaid_lines(
            "flowchart LR\n    B@{ shape: brace, label: \"a<br><br>b\" }\n    classDef f fill:#333\n    class B f\n",
        );
        let rows = cells(&lines);
        let row_of = |label: char| rows.iter().position(|row| row.iter().any(|&(c, _)| c == label));
        let (Some(a), Some(b)) = (row_of('a'), row_of('b')) else {
            panic!("missing label row in {lines:?}");
        };
        // The cells of `row` that are painted, as the characters they hold.
        let painted = |row: usize| -> String {
            let row = rows.get(row).map_or(&[][..], Vec::as_slice);
            row.iter().filter(|(_, style)| style.bg.is_some()).map(|&(c, _)| c).collect()
        };

        assert_eq!(b, a + 2, "{lines:?}");
        assert_eq!(painted(a), "a", "{lines:?}");
        assert_eq!(painted(a + 1), "", "{lines:?}");
        assert_eq!(painted(b), "b", "{lines:?}");
    }
}
