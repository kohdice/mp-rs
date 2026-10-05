//! Text drawings of diagrams written in a ```` ```mermaid ```` code block.

mod canvas;
mod flowchart;

use std::fmt;

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

/// Displays as `line N: message`, or as the message alone when no line is to blame.
#[derive(Debug)]
pub(crate) struct SyntaxError {
    /// 1-based line within the diagram source.
    pub line: Option<usize>,
    pub message: String,
}

impl fmt::Display for SyntaxError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self.line {
            Some(line) => write!(f, "line {line}: {}", self.message),
            None => f.write_str(&self.message),
        }
    }
}

/// Mermaid's default `maxTextSize`
/// (<https://mermaid.js.org/config/schema-docs/config.html>), in characters.
const MAX_TEXT_SIZE: usize = 50_000;

/// Draws the Mermaid `source` as lines without trailing spaces, in at most `width`
/// columns when given.
///
/// Returns [`Failure::Syntax`] when the source is not valid Mermaid, exceeds
/// [`MAX_TEXT_SIZE`] characters, or has more edges than Mermaid allows, and
/// [`Failure::Unsupported`] for valid input that has no text drawing: another diagram
/// type, a feature that is not drawn, an empty chart, subgraph frames that would cover
/// a box or frame outside them, long links passing through too many layers, or a
/// drawing that fits neither `width` nor the cell limit at any spacing.
pub(crate) fn render_mermaid(source: &str, width: Option<usize>) -> Result<Vec<Line>, Failure> {
    // A string has at least as many bytes as characters, so only a long one is counted.
    if source.len() > MAX_TEXT_SIZE && source.chars().count() > MAX_TEXT_SIZE {
        return Err(Failure::Syntax(SyntaxError {
            line: None,
            message: format!("diagram text exceeds {MAX_TEXT_SIZE} characters"),
        }));
    }
    flowchart::render(source, width)
}

#[cfg(test)]
mod tests {
    use crate::ansi::{ColorMode, to_ansi};
    use crate::layout::lay_out_block;
    use crate::markdown::parse;
    use crate::style::{Line, Span, Style};
    use crate::theme::solarized::DARK_PALETTE;

    use std::ops::Range;

    use unicode_width::{UnicodeWidthChar, UnicodeWidthStr};

    const A_TO_B: &str = "┌───┐     ┌───┐\n│ A │────►│ B │\n└───┘     └───┘";
    const A_YES_B: &str = "┌───┐      ┌───┐\n│ A │─yes─►│ B │\n└───┘      └───┘";
    const A_ABOVE_B: &str = "┌───┐\n│ A │\n└───┘\n  │\n  │\n  ▼\n┌───┐\n│ B │\n└───┘";
    /// Canonical sources whose drawing tests of other spellings compare theirs with.
    const LR_A_TO_B: &str = "flowchart LR\n    A --> B\n";
    const TD_A_TO_B: &str = "flowchart TD\n    A --> B\n";
    const LR_A_YES_B: &str = "flowchart LR\n    A -->|yes| B\n";

    fn lay_out(markdown: &str, width: Option<usize>) -> Vec<Line> {
        parse(markdown)
            .iter()
            .flat_map(|block| lay_out_block(block, width, ColorMode::Plain, 0))
            .collect()
    }

    fn span(text: &str, style: Style) -> Span {
        Span { text: text.to_owned(), style }
    }

    fn plain(markdown: &str, width: Option<usize>) -> String {
        to_ansi(&lay_out(markdown, width), ColorMode::Plain)
    }

    fn mermaid(body: &str) -> String {
        mermaid_in(body, None)
    }

    fn mermaid_in(body: &str, width: Option<usize>) -> String {
        plain(&format!("```mermaid\n{body}```\n"), width)
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

    /// The top row, left column, bottom row and right column of the subgraph frame whose
    /// top border shows `title`.
    fn frame_of(output: &str, title: &str) -> Option<(usize, usize, usize, usize)> {
        let title_border = format!("┌─ {title} ─");
        let (top, line) =
            output.lines().enumerate().find(|(_, line)| line.contains(&title_border))?;
        let start = line.find(&title_border)?;
        let left = line.get(..start)?.width();
        let after = line.get(start..)?;
        let right = left + after.get(..after.find('┐')?)?.width();
        let bottom = (top + 1..output.lines().count())
            .find(|&row| glyph_at(output, row, left) == Some('└'))?;
        Some((top, left, bottom, right))
    }

    /// Like [`frame_of`], for a frame whose top border links may cross: `┌`, then `─` or
    /// `┼` up to `─ title ─`, then `┐`.
    fn crossed_frame_of(output: &str, title: &str) -> Option<(usize, usize, usize, usize)> {
        let title_border = format!("─ {title} ─");
        let (top, line, corner, at) = output.lines().enumerate().find_map(|(top, line)| {
            line.match_indices(&title_border).find_map(|(at, _)| {
                let before = line.get(..at)?;
                let corner = before.rfind('┌')?;
                let between = before.get(corner + '┌'.len_utf8()..)?;
                between.chars().all(|c| "─┼".contains(c)).then_some((top, line, corner, at))
            })
        })?;
        let left = line.get(..corner)?.width();
        let right = line.get(..at + line.get(at..)?.find('┐')?)?.width();
        let bottom = (top + 1..output.lines().count())
            .find(|&row| glyph_at(output, row, left) == Some('└'))?;
        Some((top, left, bottom, right))
    }

    /// The display columns of ` title ` in the top border of the frame whose top-left
    /// corner is at `(top, left)`.
    fn title_cells(output: &str, top: usize, left: usize, title: &str) -> Option<Range<usize>> {
        let line = output.lines().nth(top)?;
        let padded = format!(" {title} ");
        let start = line
            .match_indices(&padded)
            .filter_map(|(at, _)| line.get(..at).map(UnicodeWidthStr::width))
            .find(|&col| col > left)?;
        Some(start..start + padded.width())
    }

    /// The display columns and glyphs of every arrowhead in `output`.
    fn arrowheads(output: &str) -> Vec<(usize, usize, char)> {
        output
            .lines()
            .enumerate()
            .flat_map(|(row, line)| {
                let mut col = 0;
                line.chars()
                    .filter_map(move |c| {
                        let at = col;
                        col += c.width().unwrap_or(0);
                        "►◄▲▼".contains(c).then_some((row, at, c))
                    })
                    .collect::<Vec<_>>()
            })
            .collect()
    }

    /// The place of `word` in `output`, which must sit on a line: a line cell on either side.
    fn label_on_line(output: &str, word: &str) -> (usize, usize) {
        let Some((row, col)) = position_of_word(output, word) else {
            panic!("missing {word} in\n{output}");
        };
        assert_eq!(glyph_at(output, row, col - 1), Some('─'), "{word} in\n{output}");
        assert_eq!(glyph_at(output, row, col + word.width()), Some('─'), "{word} in\n{output}");
        (row, col)
    }

    /// The character whose first display column is `col` on line `row`.
    fn glyph_at(output: &str, row: usize, col: usize) -> Option<char> {
        let mut start = 0;
        for c in output.lines().nth(row)?.chars() {
            if start == col {
                return Some(c);
            }
            start += c.width().unwrap_or(0);
        }
        None
    }

    fn words(output: &str) -> impl Iterator<Item = &str> {
        output.split(|c: char| !c.is_alphanumeric()).filter(|word| !word.is_empty())
    }

    fn whole_words(output: &str, word: &str) -> usize {
        words(output).filter(|&found| found == word).count()
    }

    /// The row and first display column of the first whole-word occurrence of `word`.
    fn position_of_word(output: &str, word: &str) -> Option<(usize, usize)> {
        output.lines().enumerate().find_map(|(row, line)| {
            line.match_indices(word).find_map(|(at, _)| {
                let before = line.get(..at)?;
                let after = line.get(at + word.len()..)?;
                let bounded = !before.chars().next_back().is_some_and(char::is_alphanumeric)
                    && !after.chars().next().is_some_and(char::is_alphanumeric);
                bounded.then(|| (row, before.width()))
            })
        })
    }

    fn count_glyph(output: &str, glyph: char) -> usize {
        output.chars().filter(|&c| c == glyph).count()
    }

    /// The display width of the widest line of `output`.
    fn widest(output: &str) -> usize {
        output.lines().map(UnicodeWidthStr::width).max().unwrap_or(0)
    }

    fn is_line_glyph(c: Option<char>) -> bool {
        c.is_some_and(|c| "─│┄┆━┃┌┐└┘├┤┬┴┼".contains(c))
    }

    /// The top border row, left column, bottom border row and right column of the box
    /// whose label row is `│ label │`: the border rows are the nearest rows above and
    /// below the label row that do not hold `│` in both border columns, so a box grown
    /// to give its links rows of their own is measured whole.
    fn box_bounds(output: &str, label: &str) -> Option<(usize, usize, usize, usize)> {
        let (row, left, right) = box_of(output, label)?;
        let side = |row: usize| {
            glyph_at(output, row, left) == Some('│') && glyph_at(output, row, right) == Some('│')
        };
        let top = (0..row).rev().find(|&above| !side(above))?;
        let bottom = (row + 1..=output.lines().count()).find(|&below| !side(below))?;
        Some((top, left, bottom, right))
    }

    /// Whether each label appears exactly once, inside a box whose border rows are made
    /// of corner glyphs and `─` over the box's columns.
    fn boxes_intact(output: &str, labels: &[&str]) -> bool {
        labels.iter().all(|label| {
            let Some((top, left, bottom, right)) = box_bounds(output, label) else {
                return false;
            };
            let border = |row: usize, corners: [&str; 2]| {
                (left..=right).all(|col| {
                    let expected = if col == left {
                        corners[0]
                    } else if col == right {
                        corners[1]
                    } else {
                        "─"
                    };
                    glyph_at(output, row, col).is_some_and(|c| expected.contains(c))
                })
            };
            output.matches(label).count() == 1
                && border(top, ["┌╭╱", "┐╮╲"])
                && border(bottom, ["└╰╲", "┘╯╱"])
        })
    }

    /// Whether the boxes labelled `labels` are intact and side by side on one row, in
    /// that order from left to right, with an arrowhead just before each box after the
    /// first and no other arrowhead.
    fn is_lr_chain(output: &str, labels: &[&str]) -> bool {
        let Some(boxes) =
            labels.iter().map(|label| box_of(output, label)).collect::<Option<Vec<_>>>()
        else {
            return false;
        };
        boxes_intact(output, labels)
            && count_glyph(output, '►') == labels.len() - 1
            && boxes.windows(2).all(|pair| match pair {
                &[(row_a, _, right_a), (row_b, left_b, _)] => {
                    row_a == row_b
                        && right_a < left_b
                        && glyph_at(output, row_b, left_b - 1) == Some('►')
                }
                _ => false,
            })
    }

    /// Whether the box labelled `label` shares no cell with the frame `(top, left,
    /// bottom, right)`.
    fn box_outside_frame(
        output: &str,
        label: &str,
        (top, left, bottom, right): (usize, usize, usize, usize),
    ) -> bool {
        box_of(output, label).is_some_and(|(row, box_left, box_right)| {
            row + 1 < top || bottom < row - 1 || box_right < left || right < box_left
        })
    }

    /// Whether the box `(label row, left, right)`, as [`box_of`] gives it, lies strictly
    /// inside the frame `(top, left, bottom, right)`.
    fn box_inside_frame(
        (row, box_left, box_right): (usize, usize, usize),
        (top, left, bottom, right): (usize, usize, usize, usize),
    ) -> bool {
        top < row && row < bottom && left < box_left && box_right < right
    }

    /// Whether the frame `inner` lies strictly inside the frame `outer`, both given as
    /// `(top, left, bottom, right)`.
    fn frame_inside_frame(
        (top, left, bottom, right): (usize, usize, usize, usize),
        (outer_top, outer_left, outer_bottom, outer_right): (usize, usize, usize, usize),
    ) -> bool {
        outer_top < top && outer_left < left && bottom < outer_bottom && right < outer_right
    }

    /// The bounds of the frame titled `title` when all four borders are whole: `┌─ title ─`
    /// continued by `─` up to `┐`, `│` down both sides, or `┼` where a link crosses, and
    /// `└─…─┘` along the bottom.
    fn intact_frame(output: &str, title: &str) -> Option<(usize, usize, usize, usize)> {
        let (top, left, bottom, right) = frame_of(output, title)?;
        let title_end = left + format!("┌─ {title} ─").width();
        let side = |row: usize| {
            [left, right]
                .into_iter()
                .all(|col| glyph_at(output, row, col).is_some_and(|c| "│┼".contains(c)))
        };
        let intact = (title_end..right).all(|col| glyph_at(output, top, col) == Some('─'))
            && (left + 1..right).all(|col| glyph_at(output, bottom, col) == Some('─'))
            && glyph_at(output, bottom, right) == Some('┘')
            && (top + 1..bottom).all(side);
        intact.then_some((top, left, bottom, right))
    }

    /// Like [`intact_frame`], for a frame that links may cross on any border: ` title ` in
    /// a top border otherwise made of `─`, `│` down both sides, `─` along the bottom, `┐`
    /// and `┘` at the right corners, and `┼` wherever a link crosses a border.
    fn intact_crossed_frame(output: &str, title: &str) -> Option<(usize, usize, usize, usize)> {
        let (top, left, bottom, right) = crossed_frame_of(output, title)?;
        let title = title_cells(output, top, left, title)?;
        let border = |row: usize, col: usize, straight: char| {
            glyph_at(output, row, col).is_some_and(|c| c == straight || c == '┼')
        };
        let intact =
            (left + 1..right).filter(|col| !title.contains(col)).all(|col| border(top, col, '─'))
                && (left + 1..right).all(|col| border(bottom, col, '─'))
                && (top + 1..bottom).all(|row| border(row, left, '│') && border(row, right, '│'))
                && glyph_at(output, top, right) == Some('┐')
                && glyph_at(output, bottom, right) == Some('┘');
        intact.then_some((top, left, bottom, right))
    }

    /// A long label in front of a tall layer of boxes: about 2 000 columns by 1 000 rows.
    fn huge_drawing_body() -> String {
        let targets: Vec<String> = (1..=250).map(|index| format!("B{index}")).collect();
        format!("flowchart LR\n    A -->|{}| {}\n", "x".repeat(2_000), targets.join(" & "))
    }

    #[test]
    fn mermaid_single_node_renders_as_a_box() {
        assert_eq!(mermaid("flowchart LR\n    A\n"), "┌───┐\n│ A │\n└───┘");
    }

    #[test]
    fn mermaid_bracket_label_replaces_the_node_id() {
        assert_eq!(mermaid("flowchart LR\n    A[Start]\n"), "┌───────┐\n│ Start │\n└───────┘");
    }

    #[test]
    fn mermaid_box_width_uses_the_display_width_of_wide_characters() {
        assert_eq!(mermaid("flowchart LR\n    A[開始]\n"), "┌──────┐\n│ 開始 │\n└──────┘");
    }

    #[test]
    fn mermaid_quoted_label_keeps_bracket_characters() {
        let output = mermaid("flowchart LR\n    A[\"x]y\"]\n");

        assert_eq!(output.lines().nth(1), Some("│ x]y │"));
    }

    #[test]
    fn mermaid_node_ids_may_contain_hyphens_and_dots_like_mermaid() {
        let output = mermaid("flowchart LR\n    node-1 --> step.2\n");

        assert!(is_lr_chain(&output, &["node-1", "step.2"]), "{output}");
    }

    #[test]
    fn mermaid_node_ids_may_be_non_ascii() {
        let output = mermaid("flowchart LR\n    開始 --> 終了\n");

        assert!(is_lr_chain(&output, &["開始", "終了"]), "{output}");
    }

    #[test]
    fn mermaid_lr_edge_draws_an_arrow_between_boxes() {
        assert_eq!(mermaid("flowchart LR\n    A --> B\n"), A_TO_B);
    }

    #[test]
    fn mermaid_lr_chain_lays_out_three_layers_left_to_right() {
        let output = mermaid("flowchart LR\n    A --> B --> C\n");

        assert!(is_lr_chain(&output, &["A", "B", "C"]), "{output}");
    }

    #[test]
    fn mermaid_later_label_declaration_renames_the_node() {
        let output = mermaid("flowchart LR\n    A --> B\n    B[End]\n");

        assert!(is_lr_chain(&output, &["A", "End"]), "{output}");
        assert_eq!(whole_words(&output, "B"), 0, "{output}");
    }

    #[test]
    fn mermaid_semicolons_comments_and_missing_spaces_are_accepted() {
        assert_eq!(
            mermaid("graph LR; A-->B; %% note\n%% whole-line comment\n"),
            mermaid(LR_A_TO_B)
        );
    }

    #[test]
    fn mermaid_style_directives_do_not_change_the_drawing() {
        let body = "flowchart LR\n    A:::hot --> B\n    style A fill:#f9f\n    classDef hot fill:#f00\n    class B hot\n    linkStyle 0 stroke:red\n    click A href \"https://example.com\"\n";

        assert_eq!(mermaid(body), mermaid(LR_A_TO_B));
    }

    #[test]
    fn mermaid_lines_are_muted_and_labels_are_body_colored() {
        let lines = lay_out("```mermaid\nflowchart LR\n    A --> B\n```\n", None);
        let spans: Vec<&Span> = lines.iter().flatten().collect();
        let drawing: Vec<&&Span> =
            spans.iter().filter(|span| span.text.chars().any(|c| "┌┐└┘─│►".contains(c))).collect();
        let labels: Vec<&&Span> =
            spans.iter().filter(|span| span.text == "A" || span.text == "B").collect();

        assert!(!drawing.is_empty());
        assert!(drawing.iter().all(|span| span.style.fg == Some(DARK_PALETTE.muted)));
        assert_eq!(labels.len(), 2);
        assert!(labels.iter().all(|span| span.style.fg == Some(DARK_PALETTE.body)));
    }

    #[test]
    fn mermaid_info_string_with_extra_words_still_renders() {
        assert_eq!(
            plain("```mermaid title=\"x\"\nflowchart LR\n    A\n```\n", None),
            mermaid("flowchart LR\n    A\n")
        );
    }

    #[test]
    fn mermaid_unclosed_node_label_adds_a_reason_line_above_the_code_block() {
        let markdown = "```mermaid\nflowchart LR\n    A[Start\n```\n";
        let lines = lay_out(markdown, None);

        assert_eq!(
            to_ansi(&lines, ColorMode::Plain),
            "mermaid: line 2: unclosed node label\n```mermaid\nflowchart LR\n    A[Start\n```"
        );
        let fence_style =
            Style { fg: Some(DARK_PALETTE.code_fence), dim: true, ..Style::default() };
        assert_eq!(
            lines.first(),
            Some(&vec![span("mermaid: line 2: unclosed node label", fence_style)])
        );
    }

    #[test]
    fn mermaid_edge_without_target_is_a_syntax_error() {
        assert_eq!(
            mermaid("flowchart LR\n    A -->\n"),
            "mermaid: line 2: edge has no target\n```mermaid\nflowchart LR\n    A -->\n```"
        );
    }

    #[test]
    fn mermaid_statement_without_a_node_id_is_a_syntax_error() {
        assert_eq!(
            mermaid("flowchart LR\n    A --> B\n    }}}\n"),
            "mermaid: line 3: expected a node id\n```mermaid\nflowchart LR\n    A --> B\n    }}}\n```"
        );
    }

    #[test]
    fn mermaid_unknown_direction_is_a_syntax_error() {
        assert_eq!(
            mermaid("flowchart XY\n    A\n"),
            "mermaid: line 1: unknown direction \"XY\"\n```mermaid\nflowchart XY\n    A\n```"
        );
    }

    #[test]
    fn mermaid_statement_on_the_header_line_is_a_syntax_error() {
        let body = "flowchart LR A --> B\n    C --> D\n";

        assert_eq!(
            mermaid(body),
            format!(
                "mermaid: line 1: expected a new line or \";\" after the direction\n```mermaid\n{body}```"
            )
        );
    }

    #[test]
    fn mermaid_statement_after_a_semicolon_on_the_header_line_is_accepted() {
        assert_eq!(mermaid("graph LR; A-->B\n"), mermaid(LR_A_TO_B));
    }

    #[test]
    fn mermaid_unsupported_diagram_type_falls_back_without_a_reason_line() {
        for body in
            ["sequenceDiagram\n    A->>B: hi\n", "xychart-beta horizontal\n    x-axis [a, b]\n"]
        {
            assert_eq!(mermaid(body), format!("```mermaid\n{body}```"));
        }
    }

    #[test]
    fn mermaid_deferred_node_shapes_fall_back_without_a_reason_line() {
        for body in [
            "flowchart LR\n    A(-x-)\n",
            "flowchart LR\n    A@{ shape: cloud }\n",
            "flowchart LR\n    A@{ icon: \"fa:bell\" }\n",
            "flowchart LR\n    A@{\n    shape: rect\n    }\n",
        ] {
            assert_eq!(mermaid(body), format!("```mermaid\n{body}```"), "{body}");
        }
    }

    #[test]
    fn mermaid_unclosed_rounded_and_diamond_labels_are_syntax_errors() {
        for node in ["A(x", "A{y"] {
            let body = format!("flowchart LR\n    {node}\n");
            assert_eq!(
                mermaid(&body),
                format!("mermaid: line 2: unclosed node label\n```mermaid\n{body}```"),
                "{node}"
            );
        }
    }

    #[test]
    fn mermaid_empty_block_falls_back_without_a_reason_line() {
        assert_eq!(mermaid(""), "```mermaid\n```");
    }

    #[test]
    fn mermaid_td_edge_draws_a_downward_arrow() {
        assert_eq!(mermaid("flowchart TD\n    A --> B\n"), A_ABOVE_B);
    }

    #[test]
    fn mermaid_tb_and_omitted_direction_render_like_td() {
        assert_eq!(mermaid("flowchart TB\n    A --> B\n"), mermaid(TD_A_TO_B));
        assert_eq!(mermaid("flowchart\n    A --> B\n"), mermaid(TD_A_TO_B));
    }

    #[test]
    fn mermaid_td_child_box_is_centered_under_the_edge() {
        assert_eq!(
            mermaid("flowchart TD\n    A[Start] --> B\n"),
            "┌───────┐\n│ Start │\n└───────┘\n    │\n    │\n    ▼\n  ┌───┐\n  │ B │\n  └───┘"
        );
    }

    #[test]
    fn mermaid_bt_lays_out_bottom_to_top() {
        assert_eq!(
            mermaid("flowchart BT\n    A --> B\n"),
            "┌───┐\n│ B │\n└───┘\n  ▲\n  │\n  │\n┌───┐\n│ A │\n└───┘"
        );
    }

    #[test]
    fn mermaid_rl_lays_out_right_to_left() {
        assert_eq!(
            mermaid("flowchart RL\n    A --> B\n"),
            "┌───┐     ┌───┐\n│ B │◄────│ A │\n└───┘     └───┘"
        );
    }

    #[test]
    fn mermaid_rounded_node_uses_rounded_corners() {
        assert_eq!(mermaid("flowchart LR\n    A(Start)\n"), "╭───────╮\n│ Start │\n╰───────╯");
    }

    #[test]
    fn mermaid_diamond_node_uses_slanted_corners() {
        assert_eq!(mermaid("flowchart LR\n    A{Ok?}\n"), "╱─────╲\n│ Ok? │\n╲─────╱");
    }

    #[test]
    fn mermaid_stadium_node_has_round_ends() {
        assert_eq!(mermaid("flowchart LR\n    A([Done])\n"), "╭──────╮\n( Done )\n╰──────╯");
    }

    #[test]
    fn mermaid_lr_link_into_a_stadium_ends_before_its_round_end() {
        assert_eq!(
            mermaid("flowchart LR\n    A --> B([Done])\n"),
            "┌───┐     ╭──────╮\n│ A │────►( Done )\n└───┘     ╰──────╯"
        );
    }

    #[test]
    fn mermaid_subroutine_node_has_double_sides() {
        assert_eq!(mermaid("flowchart LR\n    A[[Done]]\n"), "┌┬──────┬┐\n││ Done ││\n└┴──────┴┘");
    }

    #[test]
    fn mermaid_hexagon_node_has_pointed_sides() {
        assert_eq!(mermaid("flowchart LR\n    A{{Done}}\n"), " ╱────╲\n< Done >\n ╲────╱");
    }

    #[test]
    fn mermaid_asymmetric_node_has_a_notched_left_side() {
        assert_eq!(mermaid("flowchart LR\n    A>Done]\n"), "╲───────┐\n > Done │\n╱───────┘");
    }

    #[test]
    fn mermaid_parallelogram_nodes_lean_the_way_their_slashes_do() {
        assert_eq!(mermaid("flowchart LR\n    A[/Done/]\n"), " ┌─────┐\n╱ Done ╱\n└─────┘");
        assert_eq!(mermaid("flowchart LR\n    A[\\Done\\]\n"), "┌─────┐\n╲ Done ╲\n └─────┘");
    }

    #[test]
    fn mermaid_trapezoid_nodes_widen_towards_their_base() {
        assert_eq!(mermaid("flowchart LR\n    A[/Done\\]\n"), " ┌────┐\n╱ Done ╲\n└──────┘");
        assert_eq!(mermaid("flowchart LR\n    A[\\Done/]\n"), "┌──────┐\n╲ Done ╱\n └────┘");
    }

    #[test]
    fn mermaid_circle_nodes_bulge_at_the_label_row() {
        assert_eq!(mermaid("flowchart LR\n    A((Done))\n"), " ╭────╮\n( Done )\n ╰────╯");
        assert_eq!(mermaid("flowchart LR\n    A(((Done)))\n"), " ╭╭────╮╮\n(( Done ))\n ╰╰────╯╯");
    }

    #[test]
    fn mermaid_cylinder_node_has_an_elliptical_top() {
        assert_eq!(
            mermaid("flowchart LR\n    A[(Done)]\n"),
            "╭──────╮\n├──────┤\n│ Done │\n╰──────╯"
        );
    }

    #[test]
    fn mermaid_lr_link_into_a_cylinder_enters_on_its_label_row() {
        assert_eq!(
            mermaid("flowchart LR\n    A --> B[(DB)]\n"),
            "          ╭────╮\n┌───┐     ├────┤\n│ A │────►│ DB │\n└───┘     ╰────╯"
        );
    }

    #[test]
    fn mermaid_td_link_into_a_cylinder_points_at_its_top_arc() {
        assert_eq!(
            mermaid("flowchart TD\n    A --> B[(D)]\n"),
            "┌───┐\n│ A │\n└───┘\n  │\n  │\n  ▼\n╭───╮\n├───┤\n│ D │\n╰───╯"
        );
    }

    #[test]
    fn mermaid_lr_cylinder_beside_a_box_in_one_layer_keeps_both_intact() {
        let output = mermaid("flowchart LR\n    A --> B\n    A --> C[(D)]\n");
        assert!(!output.starts_with("mermaid:"), "{output}");
        let (Some(_), Some((b_row, b_left, _)), Some((d_row, d_left, _))) =
            (box_of(&output, "A"), box_of(&output, "B"), box_of(&output, "D"))
        else {
            panic!("{output}");
        };
        assert!(boxes_intact(&output, &["A", "B"]), "{output}");
        assert_eq!(count_glyph(&output, '►'), 2, "{output}");
        assert_eq!(glyph_at(&output, b_row, b_left - 1), Some('►'), "{output}");
        assert_eq!(glyph_at(&output, d_row, d_left - 1), Some('►'), "{output}");
        let cylinder_rows = d_row - 2..=d_row + 1;
        assert!((b_row - 1..=b_row + 1).all(|row| !cylinder_rows.contains(&row)), "{output}");
    }

    #[test]
    fn mermaid_shape_data_sets_the_shape_and_keeps_the_id_as_label() {
        for body in
            ["flowchart LR\n    A@{ shape: stadium }\n", "flowchart LR\n    A@{shape:stadium}\n"]
        {
            assert_eq!(mermaid(body), "╭───╮\n( A )\n╰───╯", "{body}");
        }
    }

    #[test]
    fn mermaid_shape_data_label_overrides_the_bracket_label() {
        assert_eq!(
            mermaid("flowchart LR\n    A[x]@{ shape: hex, label: \"Hello, world\" }\n"),
            " ╱────────────╲\n< Hello, world >\n ╲────────────╱"
        );
        assert_eq!(mermaid("flowchart LR\n    A@{ label: y }\n"), "┌───┐\n│ y │\n└───┘");
        assert_eq!(
            mermaid("flowchart LR\n    A@{ label: \"x}y\" }\n"),
            "┌─────┐\n│ x}y │\n└─────┘"
        );
    }

    #[test]
    fn mermaid_shape_data_on_a_link_target_and_after_a_class_is_read() {
        assert_eq!(
            mermaid("flowchart LR\n    A --> B@{ shape: stadium }\n"),
            "┌───┐     ╭───╮\n│ A │────►( B )\n└───┘     ╰───╯"
        );
        assert_eq!(
            mermaid("flowchart LR\n    A:::cls@{ shape: stadium }\n"),
            "╭───╮\n( A )\n╰───╯"
        );
    }

    #[test]
    fn mermaid_shape_data_image_parameters_do_not_change_the_drawing() {
        assert_eq!(
            mermaid("flowchart LR\n    A@{ shape: rect, w: 100, h: 50, pos: t, constraint: on }\n"),
            "┌───┐\n│ A │\n└───┘"
        );
    }

    #[test]
    fn mermaid_unknown_shape_data_shape_is_a_syntax_error() {
        for name in ["blob", "Rect"] {
            let body = format!("flowchart LR\n    A@{{ shape: {name} }}\n");
            assert_eq!(
                mermaid(&body),
                format!("mermaid: line 2: no such shape \"{name}\"\n```mermaid\n{body}```"),
                "{name}"
            );
        }
    }

    #[test]
    fn mermaid_shape_data_names_and_aliases_render_like_their_bracket_forms() {
        let names: [(&[&str], &str); 14] = [
            (&["rect", "proc", "process", "rectangle"], "A[A]"),
            (&["rounded", "event"], "A(A)"),
            (&["diam", "decision", "diamond", "question"], "A{A}"),
            (&["stadium", "pill", "terminal"], "A([A])"),
            (&["fr-rect", "framed-rectangle", "subproc", "subprocess", "subroutine"], "A[[A]]"),
            (&["cyl", "cylinder", "database", "db"], "A[(A)]"),
            (&["circle", "circ"], "A((A))"),
            (&["dbl-circ", "double-circle"], "A(((A)))"),
            (&["hex", "hexagon", "prepare"], "A{{A}}"),
            (&["lean-r", "lean-right", "in-out"], "A[/A/]"),
            (&["lean-l", "lean-left", "out-in"], "A[\\A\\]"),
            (&["trap-b", "priority", "trapezoid", "trapezoid-bottom"], "A[/A\\]"),
            (&["trap-t", "inv-trapezoid", "manual", "trapezoid-top"], "A[\\A/]"),
            (&["odd"], "A>A]"),
        ];
        for (aliases, bracket_form) in names {
            let expected = mermaid(&format!("flowchart LR\n    {bracket_form}\n"));
            assert!(!expected.starts_with("mermaid:"), "{expected}");
            assert!(!expected.starts_with("```mermaid"), "{expected}");
            for name in aliases {
                let body = format!("flowchart LR\n    A@{{ shape: {name} }}\n");
                assert_eq!(mermaid(&body), expected, "{name}");
            }
        }
    }

    #[test]
    fn mermaid_quoted_labels_in_two_character_brackets_keep_bracket_characters() {
        let stadium = mermaid("flowchart LR\n    A([\"x])y\"])\n");
        assert!(stadium.lines().any(|line| line == "( x])y )"), "{stadium}");
        let hexagon = mermaid("flowchart LR\n    A{{\"a}}b\"}}\n");
        assert!(hexagon.lines().any(|line| line == "< a}}b >"), "{hexagon}");
    }

    #[test]
    fn mermaid_td_link_into_a_shaped_node_points_at_its_top_border() {
        assert_eq!(
            mermaid("flowchart TD\n    A --> B{{Done}}\n"),
            "  ┌───┐\n  │ A │\n  └───┘\n    │\n    │\n    ▼\n ╱────╲\n< Done >\n ╲────╱"
        );
    }

    #[test]
    fn mermaid_example_document_flowcharts_with_stadium_nodes_render() {
        let output = plain(include_str!("../../../examples/EXAMPLE.md"), None);
        assert!(output.lines().any(|line| line.contains("( Done )")), "{output}");
        assert!(output.lines().any(|line| line.contains("( ANSI output )")), "{output}");
        assert!(!output.contains("E --> G([Done])"), "{output}");
        assert!(!output.contains("Render --> Out([ANSI output])"), "{output}");
    }

    #[test]
    fn mermaid_unclosed_two_character_brackets_are_syntax_errors() {
        for node in [
            "A([x)", "A([x]", "A[[x]", "A[(x)", "A((x)", "A(((x))", "A{{x}", "A>x", "A[/x]",
            "A[\\x",
        ] {
            let body = format!("flowchart LR\n    {node}\n");
            assert_eq!(
                mermaid(&body),
                format!("mermaid: line 2: unclosed node label\n```mermaid\n{body}```"),
                "{node}"
            );
        }
    }

    #[test]
    fn mermaid_open_link_has_no_arrowhead() {
        assert_eq!(
            mermaid("flowchart LR\n    A --- B\n"),
            "┌───┐     ┌───┐\n│ A │─────│ B │\n└───┘     └───┘"
        );
    }

    #[test]
    fn mermaid_dotted_link_uses_dotted_glyphs() {
        let lr = mermaid("flowchart LR\n    A -.-> B\n");
        let td = mermaid("flowchart TD\n    A -.-> B\n");

        assert_eq!(lr.lines().nth(1), Some("│ A │┄┄┄┄►│ B │"));
        assert_eq!(td.lines().skip(3).take(3).collect::<Vec<_>>(), ["  ┆", "  ┆", "  ▼"]);
    }

    #[test]
    fn mermaid_thick_link_uses_heavy_glyphs() {
        let lr = mermaid("flowchart LR\n    A ==> B\n");
        let td = mermaid("flowchart TD\n    A ==> B\n");

        assert_eq!(lr.lines().nth(1), Some("│ A │━━━━►│ B │"));
        assert_eq!(td.lines().skip(3).take(3).collect::<Vec<_>>(), ["  ┃", "  ┃", "  ▼"]);
    }

    #[test]
    fn mermaid_circle_and_cross_link_ends_use_their_markers() {
        let circle = mermaid("flowchart LR\n    A --o B\n");
        let cross = mermaid("flowchart LR\n    A --x B\n");

        assert_eq!(circle.lines().nth(1), Some("│ A │────○│ B │"));
        assert_eq!(cross.lines().nth(1), Some("│ A │────×│ B │"));
    }

    #[test]
    fn mermaid_bidirectional_link_has_a_marker_at_both_ends() {
        let arrows = mermaid("flowchart LR\n    A <--> B\n");
        let circles = mermaid("flowchart LR\n    A o--o B\n");

        assert_eq!(arrows.lines().nth(1), Some("│ A │◄───►│ B │"));
        assert_eq!(circles.lines().nth(1), Some("│ A │○───○│ B │"));
    }

    #[test]
    fn mermaid_lr_edge_label_sits_on_the_line() {
        assert_eq!(mermaid("flowchart LR\n    A -->|yes| B\n"), A_YES_B);
    }

    #[test]
    fn mermaid_lr_edge_label_widens_the_gap_when_it_is_longer() {
        assert_eq!(
            mermaid("flowchart LR\n    A -->|approved| B\n"),
            "┌───┐           ┌───┐\n│ A │─approved─►│ B │\n└───┘           └───┘"
        );
    }

    #[test]
    fn mermaid_rl_label_sits_on_the_line_before_the_arrowhead() {
        assert_eq!(
            mermaid("flowchart RL\n    A -->|yes| B\n"),
            "┌───┐      ┌───┐\n│ B │◄─yes─│ A │\n└───┘      └───┘"
        );
    }

    #[test]
    fn mermaid_lr_label_of_a_back_edge_keeps_a_line_cell_after_its_arrowhead() {
        let output = mermaid("flowchart LR\n    A --> B\n    B -->|back| A\n");
        let (row, col) = label_on_line(&output, "back");

        assert_eq!(whole_words(&output, "back"), 1, "{output}");
        assert_ne!(glyph_at(&output, row, col - 1), Some('◄'), "{output}");
        assert_eq!(count_glyph(&output, '◄'), 1, "{output}");
    }

    #[test]
    fn mermaid_lr_label_between_two_markers_keeps_a_line_cell_from_each() {
        let output = mermaid("flowchart LR\n    A <-->|yes| B\n");

        assert_eq!(output.lines().nth(1), Some("│ A │◄─yes─►│ B │"), "{output}");
    }

    #[test]
    fn mermaid_lr_label_on_a_link_without_arrowhead_needs_no_cell_for_one() {
        let output = mermaid("flowchart LR\n    A ---|yes| B\n");

        assert_eq!(output.lines().nth(1), Some("│ A │─yes─│ B │"), "{output}");
    }

    #[test]
    fn mermaid_lr_two_cell_label_fits_the_default_gap() {
        let output = mermaid("flowchart LR\n    A -->|no| B\n");

        assert_eq!(output.lines().nth(1), Some("│ A │─no─►│ B │"), "{output}");
        assert_eq!(widest(&output), widest(A_TO_B), "{output}");
    }

    #[test]
    fn mermaid_text_label_form_is_equivalent_to_pipes_for_every_stroke() {
        for (text_form, pipe_form, line) in [
            ("A -- yes --> B", "A -->|yes| B", "│ A │─yes─►│ B │"),
            ("A -. yes .-> B", "A -.->|yes| B", "│ A │┄yes┄►│ B │"),
            ("A == yes ==> B", "A ==>|yes| B", "│ A │━yes━►│ B │"),
        ] {
            let pipes = mermaid(&format!("flowchart LR\n    {pipe_form}\n"));

            assert_eq!(pipes.lines().nth(1), Some(line), "{pipe_form}");
            assert_eq!(mermaid(&format!("flowchart LR\n    {text_form}\n")), pipes, "{text_form}");
        }
    }

    #[test]
    fn mermaid_td_edge_label_sits_on_the_line() {
        assert_eq!(
            mermaid("flowchart TD\n    A -->|yes| B\n"),
            "┌───┐\n│ A │\n└───┘\n  │\n yes\n  ▼\n┌───┐\n│ B │\n└───┘"
        );
    }

    #[test]
    fn mermaid_td_even_width_label_leans_left_of_its_line() {
        assert_eq!(
            mermaid("flowchart TD\n    A -->|no| B\n"),
            "┌───┐\n│ A │\n└───┘\n  │\n no\n  ▼\n┌───┐\n│ B │\n└───┘"
        );
    }

    #[test]
    fn mermaid_td_label_row_comes_after_the_tracks() {
        let output = mermaid("flowchart TD\n    A -->|yes| B\n    A --> C\n");
        let Some((label_row, label_col)) = position_of_word(&output, "yes") else {
            panic!("missing label in\n{output}");
        };
        let (Some((_, _, a_bottom, _)), Some((_, b_left, _))) =
            (box_bounds(&output, "A"), box_of(&output, "B"))
        else {
            panic!("missing box in\n{output}");
        };
        let heads = arrowheads(&output);
        let Some(&(head_row, into_b, _)) =
            heads.iter().find(|&&(_, col, _)| col > b_left && col < b_left + 4)
        else {
            panic!("missing arrowhead into B in\n{output}");
        };

        assert_eq!(label_row + 1, head_row, "{output}");
        assert_eq!(label_col, into_b - 1, "{output}");
        assert!(
            output.lines().take(label_row).skip(a_bottom + 1).any(|line| line.contains('─')),
            "{output}"
        );
        assert_eq!(count_glyph(&output, '▼'), 2, "{output}");
        assert!(boxes_intact(&output, &["A", "B", "C"]), "{output}");
    }

    #[test]
    fn mermaid_td_wide_label_grows_the_box_its_link_enters() {
        let output =
            mermaid("flowchart TD\n    B{Ready?} -->|yes| C\n    B -->|not yet| D\n    D --> B\n");
        let (Some((row, start)), Some((yes_row, _))) =
            (position_of_word(&output, "not"), position_of_word(&output, "yes"))
        else {
            panic!("missing label in\n{output}");
        };
        let (Some((_, c_left, _, c_right)), Some((d_top, d_left, _, d_right))) =
            (box_bounds(&output, "C"), box_bounds(&output, "D"))
        else {
            panic!("missing box in\n{output}");
        };
        let Some(&(_, into_d, _)) = arrowheads(&output)
            .iter()
            .find(|&&(head_row, col, _)| head_row == row + 1 && col > d_left)
        else {
            panic!("missing arrowhead into D in\n{output}");
        };
        let end = start + "not yet".len();

        assert_eq!(output.matches("not yet").count(), 1, "{output}");
        assert_eq!(whole_words(&output, "yes"), 1, "{output}");
        assert_eq!(yes_row, row, "{output}");
        assert_eq!(start, into_d - 3, "{output}");
        assert_eq!(glyph_at(&output, row, end), Some(' '), "{output}");
        assert_eq!(glyph_at(&output, row, end + 1), Some('│'), "{output}");
        assert_eq!(glyph_at(&output, d_top - 1, end + 1), Some('│'), "{output}");
        assert_eq!(glyph_at(&output, row, start - 1), Some(' '), "{output}");
        assert_eq!(count_glyph(&output, '┼'), 0, "{output}");
        assert!(d_right - d_left > c_right - c_left, "{output}");
        assert!(boxes_intact(&output, &["Ready?", "C", "D"]), "{output}");
    }

    /// Asserts that `label` appears once on the row directly above the arrowheads,
    /// centred on the `▼` it rides (its start `width / 2` left of it), with blank cells
    /// on either side and every other `▼` at least two columns from its ends.
    fn assert_label_clear_of_siblings(output: &str, label: &str) {
        let Some((row, start)) = position_of_word(output, label) else {
            panic!("missing label in\n{output}");
        };
        let end = start + label.width() - 1;
        let heads: Vec<usize> = arrowheads(output)
            .into_iter()
            .filter(|&(head_row, _, glyph)| head_row == row + 1 && glyph == '▼')
            .map(|(_, col, _)| col)
            .collect();
        let centre = start + label.width() / 2;

        assert_eq!(output.matches(label).count(), 1, "{output}");
        assert!(heads.contains(&centre), "{output}");
        assert!(
            start
                .checked_sub(1)
                .is_none_or(|col| glyph_at(output, row, col).is_none_or(|c| c == ' ')),
            "{output}"
        );
        assert!(glyph_at(output, row, end + 1).is_none_or(|c| c == ' '), "{output}");
        assert!(
            heads
                .iter()
                .filter(|&&col| col != centre)
                .all(|&col| col + 2 <= start || end + 2 <= col),
            "{output}"
        );
    }

    #[test]
    fn mermaid_td_label_reaching_past_its_box_pushes_the_siblings_apart() {
        // The first label fits between the siblings' links at the default gap; the
        // second only once B and D move apart.
        for label in ["long label", "a much longer label"] {
            let output =
                mermaid(&format!("flowchart TD\n    A --> B\n    A -->|{label}| C\n    A --> D\n"));

            assert_label_clear_of_siblings(&output, label);
            assert_eq!(count_glyph(&output, '▼'), 3, "{output}");
            assert!(boxes_intact(&output, &["A", "B", "C", "D"]), "{output}");
        }
    }

    #[test]
    fn mermaid_td_label_reaching_past_its_box_keeps_one_blank_from_its_neighbours_line() {
        // On the label row there are no boxes, only the lines entering the layer: the
        // label keeps one blank from B's line, not the sibling gap from B's box.
        let label = "long label";
        let output =
            mermaid(&format!("flowchart TD\n    A --> B\n    A -->|{label}| C\n    A --> D\n"));
        let Some((row, start)) = position_of_word(&output, label) else {
            panic!("missing label in\n{output}");
        };
        let end = start + label.width() - 1;
        let heads: Vec<usize> = arrowheads(&output)
            .into_iter()
            .filter(|&(head_row, _, glyph)| head_row == row + 1 && glyph == '▼')
            .map(|(_, col, _)| col)
            .collect();
        let (Some(&into_b), Some(&into_d)) = (heads.first(), heads.last()) else {
            panic!("missing arrowheads in\n{output}");
        };

        assert_eq!(start, into_b + 2, "{output}");
        assert!(into_d >= end + 2, "{output}");
        assert_label_clear_of_siblings(&output, label);
        assert_eq!(count_glyph(&output, '▼'), 3, "{output}");
        assert!(boxes_intact(&output, &["A", "B", "C", "D"]), "{output}");
    }

    #[test]
    fn mermaid_td_label_near_a_frame_moves_nothing_the_rule_does_not_require() {
        // A frame border is a neighbour on the label row only when the frame spans the
        // layer before as well; a frame opening in the label's own layer starts below it.
        let label = "long label";
        let body = |edge: &str, members: &str| {
            format!("flowchart TD\n    A --> B\n    A {edge} C\n    subgraph S\n{members}    end\n")
        };
        let labelled = format!("-->|{label}|");
        let common = |output: &str| {
            assert_eq!(output.matches(label).count(), 1, "{output}");
            assert_eq!(count_glyph(output, '▼'), 2, "{output}");
            assert!(boxes_intact(output, &["A", "B", "C"]), "{output}");
        };
        let left_of = |output: &str, name: &str| match box_of(output, name) {
            Some((_, left, _)) => left,
            None => panic!("missing box {name} in\n{output}"),
        };
        let crossed_frame = |output: &str| match intact_crossed_frame(output, "S") {
            Some((_, left, _, right)) => (left, right),
            None => panic!("missing frame in\n{output}"),
        };
        let label_at = |output: &str| match position_of_word(output, label) {
            Some(at) => at,
            None => panic!("missing label in\n{output}"),
        };

        // The labelled link comes from outside the frame, which never encloses the label.
        let plain = mermaid(&body("-->", "    C\n"));
        let output = mermaid(&body(&labelled, "    C\n"));
        let (row, start) = label_at(&output);
        let Some(into_b) = arrowheads(&output)
            .into_iter()
            .find(|&(_, col, glyph)| glyph == '▼' && col < left_of(&output, "C"))
            .map(|(_, col, _)| col)
        else {
            panic!("missing arrowhead into B in\n{output}");
        };
        common(&output);
        assert_eq!(left_of(&output, "C"), left_of(&plain, "C"), "{output}\n{plain}");
        assert_eq!(crossed_frame(&output).0, crossed_frame(&plain).0, "{output}\n{plain}");
        assert!(start >= into_b + 2, "{output}");
        assert_eq!(glyph_at(&output, row, start - 1), Some(' '), "{output}");

        // The frame closes before the labelled slot and lies below the label row.
        let plain = mermaid(&body("-->", "    B\n"));
        let output = mermaid(&body(&labelled, "    B\n"));
        common(&output);
        assert_eq!(left_of(&output, "C"), crossed_frame(&output).1 + 3, "{output}");
        assert_eq!(left_of(&output, "C"), left_of(&plain, "C"), "{output}\n{plain}");

        // The frame spans the layer before, so its right border is on the label row.
        let output = mermaid(&format!(
            "flowchart TD\n    A {labelled} B\n    A --> C\n    subgraph S\n    A\n    C\n    end\n"
        ));
        let (row, start) = label_at(&output);
        let Some((_, _, _, right)) = intact_frame(&output, "S") else {
            panic!("missing frame in\n{output}");
        };
        common(&output);
        assert_eq!(glyph_at(&output, row, right), Some('│'), "{output}");
        assert!(start >= right + 2, "{output}");
    }

    #[test]
    fn mermaid_td_labels_on_crossing_links_hide_no_arrowhead() {
        let output = mermaid(
            "flowchart TD\n    A -->|one| C\n    A -->|two| D\n    B -->|three| C\n    B -->|four| D\n",
        );
        let positions: Vec<(usize, usize, usize)> = ["one", "two", "three", "four"]
            .iter()
            .map(|label| {
                let Some((row, col)) = position_of_word(&output, label) else {
                    panic!("missing label {label} in\n{output}");
                };
                (row, col, col + label.len() - 1)
            })
            .collect();
        let mut spans: Vec<(usize, usize)> =
            positions.iter().map(|&(_, start, end)| (start, end)).collect();
        spans.sort_unstable();
        let row = positions.first().map(|&(row, ..)| row);
        let heads: Vec<usize> = arrowheads(&output)
            .iter()
            .filter(|&&(head_row, _, glyph)| {
                Some(head_row) == row.map(|row| row + 1) && glyph == '▼'
            })
            .map(|&(_, col, _)| col)
            .collect();

        for label in ["one", "two", "three", "four"] {
            assert_eq!(whole_words(&output, label), 1, "{output}");
        }
        assert!(positions.iter().all(|&(label_row, ..)| Some(label_row) == row), "{output}");
        assert!(spans.windows(2).all(|pair| pair[0].1 + 1 < pair[1].0), "{output}");
        assert_eq!(count_glyph(&output, '▼'), 4, "{output}");
        for (label, &(_, start, _)) in ["one", "two", "three", "four"].iter().zip(&positions) {
            assert!(heads.contains(&(start + label.len() / 2)), "{label}\n{output}");
        }
        assert!(boxes_intact(&output, &["A", "B", "C", "D"]), "{output}");
    }

    #[test]
    fn mermaid_bt_label_row_lies_below_the_arrowheads() {
        assert_eq!(
            mermaid("flowchart BT\n    A -->|yes| B\n"),
            "┌───┐\n│ B │\n└───┘\n  ▲\n yes\n  │\n┌───┐\n│ A │\n└───┘"
        );
    }

    #[test]
    fn mermaid_fan_out_stacks_children_in_the_next_layer() {
        let output = mermaid("flowchart LR\n    A --> B\n    A --> C\n");
        let [Some(a), Some(b), Some(c)] = ["A", "B", "C"].map(|label| box_of(&output, label))
        else {
            panic!("missing box in\n{output}");
        };

        assert!(b.1 > a.2 && c.1 > a.2, "{output}");
        assert!(b.0 < c.0, "{output}");
        assert_eq!(count_glyph(&output, '►'), 2, "{output}");
        assert_eq!(glyph_at(&output, b.0, b.1 - 1), Some('►'), "{output}");
        assert_eq!(glyph_at(&output, c.0, c.1 - 1), Some('►'), "{output}");
        assert!(boxes_intact(&output, &["A", "B", "C"]), "{output}");
    }

    #[test]
    fn mermaid_fan_in_joins_two_sources_into_one_target() {
        let output = mermaid("flowchart LR\n    A --> C\n    B --> C\n");
        let [Some(a), Some(b), Some(c)] = ["A", "B", "C"].map(|label| box_of(&output, label))
        else {
            panic!("missing box in\n{output}");
        };

        assert!(c.1 > a.2 && c.1 > b.2, "{output}");
        assert!(is_line_glyph(glyph_at(&output, a.0, a.2 + 1)), "{output}");
        assert!(is_line_glyph(glyph_at(&output, b.0, b.2 + 1)), "{output}");
        let rows = |label| box_bounds(&output, label).map(|(top, _, bottom, _)| bottom - top + 1);
        assert_eq!(rows("C"), Some(5), "{output}");
        assert_eq!(rows("A"), Some(3), "{output}");
        assert_eq!(rows("B"), Some(3), "{output}");
        assert_eq!(count_glyph(&output, '►'), 2, "{output}");
        assert_eq!(glyph_at(&output, c.0 - 1, c.1 - 1), Some('►'), "{output}");
        assert_eq!(glyph_at(&output, c.0 + 1, c.1 - 1), Some('►'), "{output}");
        assert_ne!(glyph_at(&output, c.0, c.1 - 1), Some('►'), "{output}");
        assert!(boxes_intact(&output, &["A", "B", "C"]), "{output}");
    }

    #[test]
    fn mermaid_lr_three_links_into_one_box_enter_on_separate_rows() {
        let output = mermaid("flowchart LR\n    A --> D\n    B --> D\n    C --> D\n");
        let Some((top, left, bottom, _)) = box_bounds(&output, "D") else {
            panic!("missing box in\n{output}");
        };
        let Some((row, ..)) = box_of(&output, "D") else { panic!("missing box in\n{output}") };

        assert_eq!(bottom - top + 1, 5, "{output}");
        assert_eq!(count_glyph(&output, '►'), 3, "{output}");
        for entry in row - 1..=row + 1 {
            assert_eq!(glyph_at(&output, entry, left - 1), Some('►'), "{output}");
        }
        assert!(boxes_intact(&output, &["A", "B", "C", "D"]), "{output}");
    }

    #[test]
    fn mermaid_td_fan_out_leaves_the_box_from_separate_cells() {
        let output = mermaid("flowchart TD\n    A[Source node] --> B[Left]\n    A --> C[Right]\n");
        let Some((row, left, right)) = box_of(&output, "Source node") else {
            panic!("missing box in\n{output}");
        };
        let below: Vec<(usize, char)> = (left..=right)
            .filter_map(|col| Some((col, glyph_at(&output, row + 2, col)?)))
            .filter(|&(_, c)| is_line_glyph(Some(c)))
            .collect();

        assert_eq!(below.len(), 2, "{output}");
        assert!(below.iter().all(|&(col, c)| c == '│' && col != left && col != right), "{output}");
        assert_ne!(below.first().map(|&(col, _)| col), below.get(1).map(|&(col, _)| col));
        assert_eq!(count_glyph(&output, '▼'), 2, "{output}");
        assert!(boxes_intact(&output, &["Source node", "Left", "Right"]), "{output}");
    }

    #[test]
    fn mermaid_td_exits_are_ordered_like_their_targets() {
        let output = mermaid("flowchart TD\n    A[Source node] --> B[Left]\n    A --> C[Right]\n");

        assert_eq!(count_glyph(&output, '┼'), 0, "{output}");
        assert_eq!(count_glyph(&output, '▼'), 2, "{output}");
        assert!(boxes_intact(&output, &["Source node", "Left", "Right"]), "{output}");
    }

    #[test]
    fn mermaid_td_children_spread_under_their_exits() {
        let output = mermaid("flowchart TD\n    A[Source node] --> B[Left]\n    A --> C[Right]\n");
        let [Some(a), Some(b), Some(c)] =
            ["Source node", "Left", "Right"].map(|label| box_of(&output, label))
        else {
            panic!("missing box in\n{output}");
        };
        let below: Vec<usize> =
            (a.1..=a.2).filter(|&col| is_line_glyph(glyph_at(&output, a.0 + 2, col))).collect();
        let arrow_over = |(row, left, right): (usize, usize, usize)| {
            (left..=right).find(|&col| glyph_at(&output, row - 2, col) == Some('▼'))
        };
        let zigzag = output.lines().enumerate().any(|(row, line)| {
            let mut col = 0;
            line.chars().any(|c| {
                let at = col;
                col += c.width().unwrap_or(0);
                c == '└' && glyph_at(&output, row + 1, at) == Some('┐')
            })
        });

        assert_eq!(below.len(), 2, "{output}");
        assert!(below.iter().all(|&col| glyph_at(&output, a.0 + 2, col) == Some('│')), "{output}");
        assert!(arrow_over(b).is_some_and(|col| below.first().is_some_and(|&exit| col <= exit)));
        assert!(arrow_over(c).is_some_and(|col| below.last().is_some_and(|&exit| col >= exit)));
        assert!(!zigzag, "{output}");
        assert!(boxes_intact(&output, &["Source node", "Left", "Right"]), "{output}");
    }

    #[test]
    fn mermaid_td_back_edge_head_has_its_own_cell() {
        let output =
            mermaid("flowchart TD\n    B{Ready?} -->|yes| C\n    B -->|no| D\n    D --> B\n");
        let Some((row, left, right)) = box_of(&output, "Ready?") else {
            panic!("missing box in\n{output}");
        };
        let below: Vec<(usize, char)> = (left..=right)
            .filter_map(|col| Some((col, glyph_at(&output, row + 2, col)?)))
            .collect();
        let Some(head) = below.iter().find(|&&(_, c)| c == '▲').map(|&(col, _)| col) else {
            panic!("missing back edge head in\n{output}");
        };

        assert_eq!(below.iter().filter(|&&(_, c)| c == '▲').count(), 1, "{output}");
        assert!(below.iter().any(|&(col, c)| col != head && is_line_glyph(Some(c))), "{output}");
        assert_eq!(whole_words(&output, "yes"), 1, "{output}");
        assert_eq!(whole_words(&output, "no"), 1, "{output}");
        assert!(boxes_intact(&output, &["Ready?", "C", "D"]), "{output}");
    }

    #[test]
    fn mermaid_lr_two_labelled_links_into_one_box_keep_separate_columns() {
        let output = mermaid("flowchart LR\n    A -->|x| C\n    A -->|y| C\n    A --> B\n");

        assert_eq!(count_glyph(&output, '┬'), 0, "{output}");
        assert_eq!(count_glyph(&output, '┴'), 0, "{output}");
        assert_eq!(count_glyph(&output, '►'), 3, "{output}");
        assert_eq!(whole_words(&output, "x"), 1, "{output}");
        assert_eq!(whole_words(&output, "y"), 1, "{output}");
        assert!(boxes_intact(&output, &["A", "B", "C"]), "{output}");
        let ((x_row, _), (y_row, _)) = (label_on_line(&output, "x"), label_on_line(&output, "y"));
        // Labelled lines keep a blank row between them.
        assert!(x_row.abs_diff(y_row) >= 2, "{output}");
    }

    #[test]
    fn mermaid_td_labels_of_sibling_links_sit_on_the_label_row() {
        let output =
            mermaid("flowchart TD\n    B{Ready?} -->|yes| C\n    B -->|no| D\n    D --> B\n");
        let (Some((yes_row, yes_start)), Some((no_row, no_start))) =
            (position_of_word(&output, "yes"), position_of_word(&output, "no"))
        else {
            panic!("missing label in\n{output}");
        };
        let Some((d_top, d_left, ..)) = box_bounds(&output, "D") else {
            panic!("missing box in\n{output}");
        };
        let heads: Vec<usize> = arrowheads(&output)
            .iter()
            .filter(|&&(row, _, glyph)| row == yes_row + 1 && glyph == '▼')
            .map(|&(_, col, _)| col)
            .collect();

        assert_eq!(yes_row, no_row, "{output}");
        assert!(heads.contains(&(yes_start + 1)), "{output}");
        assert!(heads.contains(&(no_start + 1)), "{output}");
        // The `no` link and the back edge `D --> B` overlap across the flow, so they turn
        // on separate tracks rather than merging.
        for glyph in ['┴', '┬', '┼'] {
            assert_eq!(count_glyph(&output, glyph), 0, "{glyph}\n{output}");
        }
        assert_eq!(arrowheads(&output).len(), 3, "{output}");
        // The back edge runs whole down to D, beside the `no` label.
        assert_eq!(glyph_at(&output, d_top - 1, d_left + 3), Some('│'), "{output}");
        assert!(boxes_intact(&output, &["Ready?", "C", "D"]), "{output}");
    }

    #[test]
    fn mermaid_td_tail_marker_has_its_own_cell() {
        let output = mermaid("flowchart TD\n    A[Source node] <--> B[Left]\n    A --> C[Right]\n");
        let Some((row, left, right)) = box_of(&output, "Source node") else {
            panic!("missing box in\n{output}");
        };
        let below: Vec<(usize, char)> = (left..=right)
            .filter_map(|col| Some((col, glyph_at(&output, row + 2, col)?)))
            .collect();
        let column_of = |glyph: char| {
            let found: Vec<usize> =
                below.iter().filter(|&&(_, c)| c == glyph).map(|&(col, _)| col).collect();
            (found.len() == 1).then(|| found.first().copied()).flatten()
        };

        let (Some(tail), Some(exit)) = (column_of('▲'), column_of('│')) else {
            panic!("expected one ▲ and one │ below the box in\n{output}");
        };
        assert_ne!(tail, exit, "{output}");
        assert_eq!(count_glyph(&output, '▼'), 2, "{output}");
        assert!(boxes_intact(&output, &["Source node", "Left", "Right"]), "{output}");
    }

    #[test]
    fn mermaid_td_self_loop_takes_the_last_cells_of_its_border() {
        let output = mermaid("flowchart TD\n    A[Source node] --> A\n    A --> B\n");
        let Some((row, left, right)) = box_of(&output, "Source node") else {
            panic!("missing box in\n{output}");
        };
        let below: Vec<(usize, char)> = (left..=right)
            .filter_map(|col| glyph_at(&output, row + 2, col).map(|c| (col, c)))
            .filter(|&(_, c)| c != ' ')
            .collect();
        let glyphs: Vec<char> = below.iter().map(|&(_, c)| c).collect();

        assert_eq!(glyphs, ['│', '│', '▲'], "{output}");
        assert!(
            below.windows(2).all(|pair| matches!(pair, &[(a, _), (b, _)] if a + 2 == b)),
            "{output}"
        );
        assert_eq!(count_glyph(&output, '▼'), 1, "{output}");
        assert_eq!(count_glyph(&output, '▲'), 1, "{output}");
        assert_eq!(count_glyph(&output, '◄'), 0, "{output}");
        assert!(boxes_intact(&output, &["Source node", "B"]), "{output}");
    }

    #[test]
    fn mermaid_td_self_loop_label_keeps_a_blank_from_the_exit_beside_it() {
        assert_eq!(
            mermaid("flowchart TD\n    A[Source node] -->|retry now| A\n    A --> B\n"),
            "┌─────────────┐\n│ Source node │\n└─────────────┘\n  │    │ ▲\n  │    └─┘\n  │ retry now\n  ▼\n┌───┐\n│ B │\n└───┘"
        );
    }

    #[test]
    fn mermaid_td_two_self_loop_labels_keep_a_blank_between_them() {
        assert_eq!(
            mermaid("flowchart TD\n    A -->|once| A\n    A -->|more| A\n"),
            "┌─────────┐\n│    A    │\n└─────────┘\n  │ ▲  │ ▲\n  └─┘  └─┘\n once more"
        );
    }

    #[test]
    fn mermaid_node_is_placed_after_its_furthest_parent() {
        let output = mermaid("flowchart LR\n    A --> B --> C\n    A --> C\n");
        let [Some(b), Some(c)] = ["B", "C"].map(|label| box_of(&output, label)) else {
            panic!("missing box in\n{output}");
        };

        assert!(c.1 > b.2, "{output}");
        assert_eq!(count_glyph(&output, '►'), 3, "{output}");
        assert!(boxes_intact(&output, &["A", "B", "C"]), "{output}");
    }

    #[test]
    fn mermaid_extra_dots_equals_and_dashes_lengthen_the_link_by_one_layer_each() {
        for link in ["A ---> B", "A -..-> B", "A ===> B", "A ---- B"] {
            let output = mermaid(&format!("flowchart LR\n    {link}\n    A --> C --> D\n"));
            let [Some(b), Some(d)] = ["B", "D"].map(|label| box_of(&output, label)) else {
                panic!("missing box for {link} in\n{output}");
            };

            assert_eq!(b.1, d.1, "{link} in\n{output}");
            assert!(boxes_intact(&output, &["A", "B", "C", "D"]), "{link} in\n{output}");
        }
    }

    #[test]
    fn mermaid_td_fan_out_places_children_side_by_side() {
        let output = mermaid("flowchart TD\n    A --> B\n    A --> C\n");
        let [Some(a), Some(b), Some(c)] = ["A", "B", "C"].map(|label| box_of(&output, label))
        else {
            panic!("missing box in\n{output}");
        };
        let centre = |(_, left, right): (usize, usize, usize)| (left + right) / 2;

        assert_eq!(b.0, c.0, "{output}");
        assert!(b.0 - 1 > a.0 + 1, "{output}");
        // B and C do not touch: at least one blank column lies between them.
        assert!(c.1 > b.2 + 1, "{output}");
        assert_eq!(count_glyph(&output, '▼'), 2, "{output}");
        assert_eq!(glyph_at(&output, b.0 - 2, centre(b)), Some('▼'), "{output}");
        assert_eq!(glyph_at(&output, c.0 - 2, centre(c)), Some('▼'), "{output}");
        assert!(boxes_intact(&output, &["A", "B", "C"]), "{output}");
    }

    #[test]
    fn mermaid_lines_never_cross_box_interiors() {
        let output = mermaid(
            "flowchart LR\n    A --> B\n    A --> C\n    B --> D\n    C --> D\n    A --> D\n",
        );

        assert!(boxes_intact(&output, &["A", "B", "C", "D"]), "{output}");
        assert_eq!(count_glyph(&output, '►'), 5, "{output}");
    }

    #[test]
    fn mermaid_back_edge_renders_both_arrowheads() {
        let output = mermaid("flowchart LR\n    A --> B --> A\n");

        assert!(boxes_intact(&output, &["A", "B"]), "{output}");
        assert_eq!(arrowheads(&output).len(), 2, "{output}");
    }

    #[test]
    fn mermaid_lr_self_loop_bumps_out_of_the_right_border() {
        assert_eq!(
            mermaid("flowchart LR\n    A --> A\n"),
            "┌───┐\n│   │─┐\n│ A │ │\n│   │◄┘\n└───┘"
        );
    }

    #[test]
    fn mermaid_td_self_loop_bumps_out_of_the_bottom_border() {
        assert_eq!(mermaid("flowchart TD\n    A --> A\n"), "┌───┐\n│ A │\n└───┘\n │ ▲\n └─┘");
    }

    #[test]
    fn mermaid_ampersand_groups_expand_to_every_pair() {
        let output = mermaid("flowchart LR\n    A & B --> C & D\n");
        let [Some(a), Some(b), Some(c), Some(d)] =
            ["A", "B", "C", "D"].map(|label| box_of(&output, label))
        else {
            panic!("missing box in\n{output}");
        };

        assert!(boxes_intact(&output, &["A", "B", "C", "D"]), "{output}");
        assert_eq!(count_glyph(&output, '►'), 4, "{output}");
        assert!([c, d].iter().all(|target| target.1 > a.2 && target.1 > b.2), "{output}");
    }

    #[test]
    fn mermaid_branch_labels_do_not_overlap_each_other_or_boxes() {
        let output = mermaid("flowchart LR\n    A -->|yes| B\n    A -->|no| C\n");

        assert_eq!(whole_words(&output, "yes"), 1, "{output}");
        assert_eq!(whole_words(&output, "no"), 1, "{output}");
        assert!(boxes_intact(&output, &["A", "B", "C"]), "{output}");
        assert_eq!(count_glyph(&output, '►'), 2, "{output}");
    }

    #[test]
    fn mermaid_fan_in_labels_do_not_cover_lines_or_each_other() {
        let output = mermaid("flowchart LR\n    A -->|a| C\n    B -->|b| C\n");

        for label in ["a", "b"] {
            assert_eq!(whole_words(&output, label), 1, "{label} in\n{output}");
        }
        let ((a_row, _), (b_row, _)) = (label_on_line(&output, "a"), label_on_line(&output, "b"));
        // Labelled lines keep a blank row between them.
        assert!(a_row.abs_diff(b_row) >= 2, "{output}");
        assert_eq!(count_glyph(&output, '►'), 2, "{output}");
    }

    #[test]
    fn mermaid_td_labels_of_links_into_one_box_stay_separate() {
        let output = mermaid("flowchart TD\n    A -->|yes| B\n    B -->|no| A\n");
        let (Some((yes_row, yes_col)), Some((no_row, no_col))) =
            (position_of_word(&output, "yes"), position_of_word(&output, "no"))
        else {
            panic!("missing label in\n{output}");
        };
        let ((first, first_col), (second, second_col)) = if yes_col < no_col {
            (("yes", yes_col), ("no", no_col))
        } else {
            (("no", no_col), ("yes", yes_col))
        };
        let blank_or_beyond = |col: Option<usize>| {
            col.is_none_or(|col| glyph_at(&output, yes_row, col).is_none_or(|c| c == ' '))
        };

        assert!(boxes_intact(&output, &["A", "B"]), "{output}");
        assert_eq!(arrowheads(&output).len(), 2, "{output}");
        assert_eq!(whole_words(&output, "yes"), 1, "{output}");
        assert_eq!(whole_words(&output, "no"), 1, "{output}");
        assert_eq!(yes_row, no_row, "{output}");
        assert!(first_col + first.len() < second_col, "{output}");
        assert!(blank_or_beyond(first_col.checked_sub(1)), "{output}");
        assert!(blank_or_beyond(Some(second_col + second.len())), "{output}");
    }

    #[test]
    fn mermaid_label_in_the_middle_gap_of_a_three_segment_link() {
        let output = mermaid("flowchart TD\n    A --> B --> C --> D\n    A -->|x| D\n");
        let [Some(b), Some(c), Some(d)] = ["B", "C", "D"].map(|label| box_bounds(&output, label))
        else {
            panic!("missing box in\n{output}");
        };
        let Some((row, col)) = position_of_word(&output, "x") else {
            panic!("missing label in\n{output}");
        };
        let into_d = arrowheads(&output)
            .into_iter()
            .filter(|&(at, _, glyph)| at + 1 == d.0 && glyph == '▼')
            .count();

        assert_eq!(whole_words(&output, "x"), 1, "{output}");
        assert!(b.2 < row && row < c.0, "{output}");
        assert!(is_line_glyph(glyph_at(&output, row - 1, col)), "{output}");
        assert!(is_line_glyph(glyph_at(&output, row + 1, col)), "{output}");
        assert_eq!(into_d, 2, "{output}");
        assert_eq!(arrowheads(&output).len(), 4, "{output}");
        assert!(boxes_intact(&output, &["A", "B", "C", "D"]), "{output}");
    }

    #[test]
    fn mermaid_lr_label_in_the_middle_gap_of_a_three_segment_link() {
        let output = mermaid("flowchart LR\n    A --> B --> C --> D\n    A -->|x| D\n");
        let [Some(b), Some(c)] = ["B", "C"].map(|label| box_bounds(&output, label)) else {
            panic!("missing box in\n{output}");
        };
        let (row, col) = label_on_line(&output, "x");

        assert_eq!(whole_words(&output, "x"), 1, "{output}");
        assert!(row > b.2, "{output}");
        assert_eq!(col, (b.3 + c.1) / 2, "{output}");
        assert_eq!(count_glyph(&output, '►'), 4, "{output}");
        assert!(boxes_intact(&output, &["A", "B", "C", "D"]), "{output}");
    }

    #[test]
    fn mermaid_td_label_of_a_two_segment_link_sits_on_the_passing_slot() {
        let output = mermaid("flowchart TD\n    A --> B --> C\n    A -->|x| C\n");
        let Some((b_row, _, b_right)) = box_of(&output, "B") else {
            panic!("missing box in\n{output}");
        };
        let Some((row, col)) = position_of_word(&output, "x") else {
            panic!("missing label in\n{output}");
        };

        assert_eq!(whole_words(&output, "x"), 1, "{output}");
        assert_eq!(row, b_row, "{output}");
        assert!(col > b_right, "{output}");
        assert_eq!(glyph_at(&output, row - 1, col), Some('│'), "{output}");
        assert_eq!(glyph_at(&output, row + 1, col), Some('│'), "{output}");
        assert_eq!(arrowheads(&output).len(), 3, "{output}");
        assert!(boxes_intact(&output, &["A", "B", "C"]), "{output}");
    }

    #[test]
    fn mermaid_td_passing_slot_label_keeps_a_blank_from_its_neighbour() {
        let output = mermaid("flowchart TD\n    A --> B --> C\n    A -->|wide one| C\n");
        let Some((b_row, _, b_right)) = box_of(&output, "B") else {
            panic!("missing box in\n{output}");
        };
        let Some((row, col)) = position_of_word(&output, "wide one") else {
            panic!("missing label in\n{output}");
        };

        assert_eq!(output.matches("wide one").count(), 1, "{output}");
        assert_eq!(row, b_row, "{output}");
        assert!(col >= b_right + 2, "{output}");
        assert!(boxes_intact(&output, &["A", "B", "C"]), "{output}");
    }

    #[test]
    fn mermaid_lr_label_of_a_two_segment_link_runs_along_the_passing_line() {
        let output = mermaid("flowchart LR\n    A --> B --> C\n    A -->|x| C\n");
        let Some((_, b_left, b_bottom, b_right)) = box_bounds(&output, "B") else {
            panic!("missing box in\n{output}");
        };
        let (row, col) = label_on_line(&output, "x");

        assert_eq!(whole_words(&output, "x"), 1, "{output}");
        assert!(row > b_bottom, "{output}");
        assert!((b_left..=b_right).contains(&col), "{output}");
        assert_eq!(count_glyph(&output, '►'), 3, "{output}");
        assert!(boxes_intact(&output, &["A", "B", "C"]), "{output}");
    }

    #[test]
    fn mermaid_label_of_a_back_edge_spanning_several_layers_rides_the_passing_line() {
        for direction in ["LR", "RL"] {
            let output = mermaid(&format!(
                "flowchart {direction}\n    A --> B --> C\n    C -->|back again| A\n"
            ));
            let [Some(b), Some(c)] = ["B", "C"].map(|label| box_bounds(&output, label)) else {
                panic!("missing box in\n{output}");
            };
            let (row, first) = label_on_line(&output, "back again");
            let last = first + "back again".width() - 1;

            assert_eq!(output.matches("back again").count(), 1, "{output}");
            assert!(row > b.2, "{output}");
            if direction == "LR" {
                assert!(b.1 <= first && last < c.1, "{output}");
            } else {
                assert!(last <= b.3 && c.3 < first, "{output}");
            }
            assert_eq!(arrowheads(&output).len(), 3, "{output}");
            assert!(boxes_intact(&output, &["A", "B", "C"]), "{output}");
        }
    }

    #[test]
    fn mermaid_reordering_a_layer_removes_a_crossing() {
        let output = mermaid("flowchart LR\n    A ---> B\n    A --> C --> D\n");
        let [Some(b), Some(d)] = ["B", "D"].map(|label| box_of(&output, label)) else {
            panic!("missing box in\n{output}");
        };

        assert!(d.0 < b.0, "{output}");
        assert_eq!(count_glyph(&output, '┼'), 0, "{output}");
    }

    #[test]
    fn mermaid_lr_self_loop_label_sits_right_of_the_loop_on_its_middle_row() {
        assert_eq!(
            mermaid("flowchart LR\n    A -->|again| A\n"),
            "┌───┐\n│   │─┐\n│ A │ │ again\n│   │◄┘\n└───┘"
        );
    }

    #[test]
    fn mermaid_lr_three_row_self_loop_label_keeps_a_blank_row_from_the_exit_beside_it() {
        assert_eq!(
            mermaid("flowchart LR\n    A -->|x<br>y<br>z| A\n    A --> B\n"),
            "┌───┐     ┌───┐\n│   │────►│ B │\n│   │     └───┘\n│ A │─┐ x\n│   │ │ y\n│   │◄┘ z\n└───┘"
        );
    }

    #[test]
    fn mermaid_lr_two_row_self_loop_label_puts_its_lower_row_on_the_second_leg() {
        assert_eq!(
            mermaid("flowchart LR\n    A -->|x<br>yy| A\n    A --> B\n"),
            "┌───┐\n│   │      ┌───┐\n│   │─────►│ B │\n│ A │─┐    └───┘\n│   │ │ x\n│   │◄┘ yy\n└───┘"
        );
    }

    #[test]
    fn mermaid_td_two_row_self_loop_label_centres_each_row_on_the_loop() {
        assert_eq!(
            mermaid("flowchart TD\n    A -->|x<br>yy| A\n"),
            "┌───┐\n│ A │\n└───┘\n │ ▲\n └─┘\n  x\n yy"
        );
    }

    #[test]
    fn mermaid_lr_two_row_label_on_a_passing_slot_runs_its_upper_row_on_the_line() {
        assert_eq!(
            mermaid("flowchart LR\n    A --> B --> C\n    A -->|x<br>y| C\n"),
            "┌───┐     ┌───┐\n│   │────►│ B │─┐   ┌───┐\n│ A │     └───┘ └──►│   │\n│   │─┐             │ C │\n└───┘ │          ┌─►│   │\n      └─────x────┘  └───┘\n            y"
        );
    }

    #[test]
    fn mermaid_td_self_loop_label_sits_below_the_loop_centred_on_it() {
        assert_eq!(
            mermaid("flowchart TD\n    A -->|again| A\n"),
            "┌───┐\n│ A │\n└───┘\n │ ▲\n └─┘\nagain"
        );
    }

    #[test]
    fn mermaid_bt_self_loop_label_sits_above_the_loop() {
        assert_eq!(
            mermaid("flowchart BT\n    A -->|again| A\n"),
            "again\n ┌─┐\n │ ▼\n┌───┐\n│ A │\n└───┘"
        );
    }

    #[test]
    fn mermaid_rl_self_loop_label_sits_left_of_the_loop() {
        assert_eq!(
            mermaid("flowchart RL\n    A -->|again| A\n"),
            "        ┌───┐\n      ┌─│   │\nagain │ │ A │\n      └►│   │\n        └───┘"
        );
    }

    #[test]
    fn mermaid_lr_self_loop_label_keeps_the_next_layer_beyond_it() {
        let output = mermaid("flowchart LR\n    A -->|retry on failure| A\n    A --> B\n");
        let Some(b) = box_of(&output, "B") else {
            panic!("missing box in\n{output}");
        };
        let Some(&(head_row, ..)) = arrowheads(&output).iter().find(|&&(.., c)| c == '◄') else {
            panic!("missing loop arrowhead in\n{output}");
        };
        let Some((row, col)) = position_of_word(&output, "retry on failure") else {
            panic!("missing label in\n{output}");
        };
        let last = col + "retry on failure".width() - 1;

        assert_eq!(output.matches("retry on failure").count(), 1, "{output}");
        assert_eq!(row + 1, head_row, "{output}");
        assert!(b.1 >= last + 2, "{output}");
        assert_eq!(count_glyph(&output, '►'), 1, "{output}");
        assert_eq!(count_glyph(&output, '◄'), 1, "{output}");
        assert!(boxes_intact(&output, &["A", "B"]), "{output}");
    }

    #[test]
    fn mermaid_lr_self_loop_label_keeps_the_tracks_of_its_box_beyond_it() {
        let output = mermaid("flowchart LR\n    A -->|again| A\n    A --> B\n    A --> C\n");
        let Some(&(head_row, head_col, _)) = arrowheads(&output).iter().find(|&&(.., c)| c == '◄')
        else {
            panic!("missing loop arrowhead in\n{output}");
        };
        let corner_col = head_col + 1;
        let Some((row, col)) = position_of_word(&output, "again") else {
            panic!("missing label in\n{output}");
        };
        let after = glyph_at(&output, row, col + "again".width());
        let Some(c) = box_of(&output, "C") else {
            panic!("missing box in\n{output}");
        };
        // The link into C turns at the first column left of its arrowhead that is not `─`.
        let Some(turn_col) =
            (0..c.1 - 1).rev().find(|&col| glyph_at(&output, c.0, col) != Some('─'))
        else {
            panic!("missing link into C in\n{output}");
        };

        assert_eq!(whole_words(&output, "again"), 1, "{output}");
        assert_eq!(glyph_at(&output, head_row, corner_col), Some('┘'), "{output}");
        assert_eq!(row + 1, head_row, "{output}");
        assert_eq!(col, corner_col + 2, "{output}");
        assert!(after.is_none_or(|c| c == ' '), "{output}");
        assert!(turn_col > col + "again".width(), "{output}");
        assert_eq!(count_glyph(&output, '►'), 2, "{output}");
        assert_eq!(count_glyph(&output, '◄'), 1, "{output}");
        assert!(boxes_intact(&output, &["A", "B", "C"]), "{output}");
    }

    #[test]
    fn mermaid_td_self_loop_label_reaching_past_its_box_keeps_a_blank_from_its_neighbour() {
        let output = mermaid("flowchart TD\n    A -->|retry on failure| A\n    B --> C\n");
        let Some((b_row, b_left, b_right)) = box_of(&output, "B") else {
            panic!("missing box in\n{output}");
        };
        let Some((_, col)) = position_of_word(&output, "retry on failure") else {
            panic!("missing label in\n{output}");
        };
        let last = col + "retry on failure".width() - 1;
        let Some(exit) =
            (b_left..=b_right).find(|&col| glyph_at(&output, b_row + 2, col) == Some('│'))
        else {
            panic!("missing link leaving B in\n{output}");
        };

        assert_eq!(output.matches("retry on failure").count(), 1, "{output}");
        assert!(exit >= last + 2, "{output}");
        assert!(boxes_intact(&output, &["A", "B", "C"]), "{output}");
    }

    #[test]
    fn mermaid_td_self_loop_label_inside_a_subgraph_stays_inside_the_frame() {
        let output = mermaid("flowchart TD\n    subgraph s\n    A -->|again| A\n    end\n");
        let Some((_, left, bottom, right)) = intact_frame(&output, "s") else {
            panic!("missing or broken frame in\n{output}");
        };
        let Some((row, col)) = position_of_word(&output, "again") else {
            panic!("missing label in\n{output}");
        };

        assert!(bottom > row, "{output}");
        assert!(left < col && col + "again".width() - 1 < right, "{output}");
        assert_eq!(whole_words(&output, "again"), 1, "{output}");
        assert!(boxes_intact(&output, &["A"]), "{output}");
    }

    #[test]
    fn mermaid_td_self_loop_label_of_a_node_between_a_frames_layers_stays_outside_the_frame() {
        let output = mermaid(
            "flowchart TD\n    subgraph s\n    A ---> B\n    end\n    A --> C\n    C -->|retry on failure| C\n",
        );
        let Some((_, left, _, right)) = intact_frame(&output, "s") else {
            panic!("missing or broken frame in\n{output}");
        };
        let Some((_, col)) = position_of_word(&output, "retry on failure") else {
            panic!("missing label in\n{output}");
        };
        let last = col + "retry on failure".width() - 1;

        assert_eq!(output.matches("retry on failure").count(), 1, "{output}");
        assert!(col >= right + 2 || last + 2 <= left, "{output}");
        assert!(boxes_intact(&output, &["A", "B", "C"]), "{output}");
    }

    #[test]
    fn mermaid_lr_self_loop_and_the_tracks_of_its_box_keep_separate_columns() {
        let output = mermaid("flowchart LR\n    A --> A\n    A --> B\n    A --> C\n");
        let [Some(a), Some(b), Some(c)] = ["A", "B", "C"].map(|label| box_of(&output, label))
        else {
            panic!("missing box in\n{output}");
        };
        let Some((a_top, _, a_bottom, _)) = box_bounds(&output, "A") else {
            panic!("missing box in\n{output}");
        };
        // The loop's last corner lies beside A, between its right border and the next
        // layer.
        let Some(loop_col) = (a_top..=a_bottom).find_map(|row| {
            (a.2 + 1..b.1.min(c.1)).find(|&col| glyph_at(&output, row, col) == Some('┘'))
        }) else {
            panic!("missing self loop in\n{output}");
        };
        // The link into C turns at the first column left of its arrowhead that is not `─`.
        let Some(turn_col) =
            (0..c.1 - 1).rev().find(|&col| glyph_at(&output, c.0, col) != Some('─'))
        else {
            panic!("missing link into C in\n{output}");
        };

        assert_eq!(glyph_at(&output, c.0, c.1 - 1), Some('►'), "{output}");
        assert!(matches!(glyph_at(&output, c.0, turn_col), Some('└' | '┌')), "{output}");
        assert!(turn_col > loop_col, "{output}");
        assert_eq!(count_glyph(&output, '►'), 2, "{output}");
        assert_eq!(count_glyph(&output, '◄'), 1, "{output}");
        assert!(boxes_intact(&output, &["A", "B", "C"]), "{output}");
    }

    #[test]
    fn mermaid_td_self_loop_label_wider_than_its_box_moves_the_drawing_right() {
        assert_eq!(
            mermaid("flowchart TD\n    A -->|retry on failure| A\n"),
            "      ┌───┐\n      │ A │\n      └───┘\n       │ ▲\n       └─┘\nretry on failure"
        );
    }

    #[test]
    fn mermaid_disconnected_nodes_all_render() {
        let output = mermaid("flowchart LR\n    A --> B\n    C\n");

        assert!(boxes_intact(&output, &["A", "B", "C"]), "{output}");
        assert_eq!(count_glyph(&output, '►'), 1, "{output}");
    }

    #[test]
    fn mermaid_subgraph_draws_a_titled_frame_around_its_nodes() {
        assert_eq!(
            mermaid("flowchart LR\n    subgraph lib\n    A\n    end\n"),
            "┌─ lib ─┐\n│ ┌───┐ │\n│ │ A │ │\n│ └───┘ │\n└───────┘"
        );
    }

    #[test]
    fn mermaid_subgraph_bracket_title_is_shown_instead_of_the_id() {
        assert_eq!(
            mermaid("flowchart LR\n    subgraph s1 [My Lib]\n    A\n    end\n"),
            "┌─ My Lib ─┐\n│ ┌───┐    │\n│ │ A │    │\n│ └───┘    │\n└──────────┘"
        );
    }

    #[test]
    fn mermaid_edge_into_a_subgraph_crosses_its_frame() {
        let output = mermaid("flowchart LR\n    A --> B\n    subgraph s\n    B\n    end\n");
        let (Some((top, left, bottom, right)), Some(a), Some(b)) =
            (frame_of(&output, "s"), box_of(&output, "A"), box_of(&output, "B"))
        else {
            panic!("missing frame or box in\n{output}");
        };
        let overlaps_frame = |(row, box_left, box_right): (usize, usize, usize)| {
            row + 1 >= top
                && row.saturating_sub(1) <= bottom
                && box_right >= left
                && box_left <= right
        };
        let inside_frame = |(row, box_left, box_right): (usize, usize, usize)| {
            top < row.saturating_sub(1) && row + 1 < bottom && left < box_left && box_right < right
        };
        let arrowheads_at_b =
            (b.0 - 1..=b.0 + 1).filter(|&row| glyph_at(&output, row, b.1 - 1) == Some('►')).count();

        assert!(!overlaps_frame(a), "{output}");
        assert!(inside_frame(b), "{output}");
        assert!((top + 1..bottom).any(|row| glyph_at(&output, row, left) == Some('┼')), "{output}");
        assert_eq!(arrowheads_at_b, 1, "{output}");
        assert!(boxes_intact(&output, &["A", "B"]), "{output}");
    }

    #[test]
    fn mermaid_td_arrow_into_a_subgraph_does_not_overwrite_its_frame() {
        let output = mermaid("flowchart TD\n    A --> B\n    subgraph s\n    B\n    end\n");
        let (Some((top, left, bottom, right)), Some(b)) =
            (crossed_frame_of(&output, "s"), box_of(&output, "B"))
        else {
            panic!("missing frame or box in\n{output}");
        };
        let width = b.2 - b.1 + 1;
        let centre = b.1 + width / 2;
        let inside =
            |row: usize, col: usize| top < row && row < bottom && left < col && col < right;
        let arrowheads_inside: Vec<(usize, usize)> = output
            .lines()
            .enumerate()
            .flat_map(|(row, _)| (left..=right).map(move |col| (row, col)))
            .filter(|&(row, col)| inside(row, col) && glyph_at(&output, row, col) == Some('▼'))
            .collect();

        let Some(title) = title_cells(&output, top, left, "s") else {
            panic!("missing title in\n{output}");
        };
        let top_border_breaks: Vec<char> = (left + 1..right)
            .filter(|col| !title.contains(col))
            .filter_map(|col| glyph_at(&output, top, col))
            .filter(|&c| c != '─')
            .collect();

        assert_eq!(arrowheads_inside, vec![(b.0 - 2, centre)], "{output}");
        assert_eq!(count_glyph(&output, '▼'), 1, "{output}");
        assert!(boxes_intact(&output, &["A", "B"]), "{output}");
        assert_eq!(top_border_breaks, vec!['┼'], "{output}");
    }

    #[test]
    fn mermaid_td_edge_label_into_a_subgraph_does_not_cover_its_frame() {
        let output = mermaid("flowchart TD\n    A -->|yes| B\n    subgraph s\n    B\n    end\n");
        let (Some((top, ..)), Some((label_row, label_col))) =
            (crossed_frame_of(&output, "s"), position_of_word(&output, "yes"))
        else {
            panic!("missing frame or label in\n{output}");
        };
        let Some((_, b_left, _)) = box_of(&output, "B") else {
            panic!("missing box in\n{output}");
        };
        let Some(&(_, into_b, _)) = arrowheads(&output).iter().find(|&&(_, col, _)| col > b_left)
        else {
            panic!("missing arrowhead into B in\n{output}");
        };
        let centre = label_col + 1;

        assert_eq!(whole_words(&output, "yes"), 1, "{output}");
        assert!(label_row < top, "{output}");
        assert_eq!(centre, into_b, "{output}");
        assert_eq!(glyph_at(&output, label_row - 1, centre), Some('│'), "{output}");
        assert_eq!(glyph_at(&output, label_row + 1, centre), Some('│'), "{output}");
        assert!(boxes_intact(&output, &["A", "B"]), "{output}");
        assert!(intact_crossed_frame(&output, "s").is_some(), "{output}");
    }

    #[test]
    fn mermaid_label_reaching_left_of_the_drawing_moves_the_drawing_right() {
        let output = mermaid("flowchart TD\n    A -->|a wide label| B\n");
        let Some((row, start)) = position_of_word(&output, "a") else {
            panic!("missing label in\n{output}");
        };
        let (Some((_, a_left, _)), Some((_, b_left, _))) =
            (box_of(&output, "A"), box_of(&output, "B"))
        else {
            panic!("missing box in\n{output}");
        };
        let heads = arrowheads(&output);

        assert_eq!(output.matches("a wide label").count(), 1, "{output}");
        assert_eq!(start, 0, "{output}");
        assert!(
            heads.iter().any(|&(head_row, col, _)| head_row == row + 1 && col == start + 6),
            "{output}"
        );
        assert_eq!(a_left, b_left, "{output}");
        assert!(a_left > 0, "{output}");
        assert!(boxes_intact(&output, &["A", "B"]), "{output}");
    }

    #[test]
    fn mermaid_td_frame_encloses_the_labels_of_its_members_links() {
        let output = mermaid("flowchart TD\n    subgraph S\n    A -->|long label| B\n    end\n");
        let (Some((_, left, _, right)), Some((_, start))) =
            (intact_frame(&output, "S"), position_of_word(&output, "long"))
        else {
            panic!("missing frame or label in\n{output}");
        };
        let end = start + "long label".len() - 1;

        assert_eq!(output.matches("long label").count(), 1, "{output}");
        assert!(left + 2 <= start, "{output}");
        assert!(end + 2 <= right, "{output}");
        assert!(boxes_intact(&output, &["A", "B"]), "{output}");
    }

    #[test]
    fn mermaid_lr_edge_label_into_a_subgraph_keeps_a_line_cell_before_its_frame() {
        let output = mermaid("flowchart LR\n    A -->|yes| B\n    subgraph s\n    B\n    end\n");
        let Some((_, left, ..)) = frame_of(&output, "s") else {
            panic!("missing frame in\n{output}");
        };
        let (_, col) = label_on_line(&output, "yes");

        assert_eq!(whole_words(&output, "yes"), 1, "{output}");
        assert!(col + "yes".width() < left, "{output}");
    }

    #[test]
    fn mermaid_lr_edge_label_out_of_a_wide_titled_subgraph_lies_outside_its_frame() {
        let output = mermaid(
            "flowchart LR\n    subgraph s [a very long title]\n    A\n    end\n    A -->|yes| B\n",
        );
        let (Some((_, left, _, right)), Some((_, col))) =
            (frame_of(&output, "a very long title"), position_of_word(&output, "yes"))
        else {
            panic!("missing frame or label in\n{output}");
        };
        let last = col + "yes".width() - 1;

        assert_eq!(whole_words(&output, "yes"), 1, "{output}");
        assert!(last < left || right < col, "{output}");
    }

    #[test]
    fn mermaid_wide_subgraph_title_does_not_overlap_the_next_layer() {
        let output = mermaid(
            "flowchart LR\n    subgraph s [a very long title]\n    A\n    end\n    A --> B\n",
        );
        let (Some((_, left, _, right)), Some((_, b_left, b_right))) =
            (frame_of(&output, "a very long title"), box_of(&output, "B"))
        else {
            panic!("missing frame or box in\n{output}");
        };

        assert!(boxes_intact(&output, &["A", "B"]), "{output}");
        assert!(b_right < left || right < b_left, "{output}");
        assert_eq!(count_glyph(&output, '►'), 1, "{output}");
    }

    #[test]
    fn mermaid_rl_wide_subgraph_title_does_not_overlap_the_previous_layer() {
        let output = mermaid(
            "flowchart RL\n    A --> B\n    subgraph s [a very long title]\n    B\n    end\n",
        );
        let (Some((_, left, _, right)), Some((_, a_left, a_right))) =
            (frame_of(&output, "a very long title"), box_of(&output, "A"))
        else {
            panic!("missing frame or box in\n{output}");
        };

        assert!(boxes_intact(&output, &["A", "B"]), "{output}");
        assert!(a_right < left || right < a_left, "{output}");
        assert_eq!(count_glyph(&output, '◄'), 1, "{output}");
    }

    #[test]
    fn mermaid_non_member_between_subgraph_members_is_placed_outside_the_frame() {
        let output =
            mermaid("flowchart LR\n    A --> B --> C\n    subgraph s\n    A\n    C\n    end\n");
        let (Some(frame), Some(a), Some(b), Some(c)) = (
            intact_crossed_frame(&output, "s"),
            box_of(&output, "A"),
            box_of(&output, "B"),
            box_of(&output, "C"),
        ) else {
            panic!("missing frame or box in\n{output}");
        };

        assert!(!output.starts_with("mermaid:"), "{output}");
        assert!(box_inside_frame(a, frame), "{output}");
        assert!(box_inside_frame(c, frame), "{output}");
        assert!(box_outside_frame(&output, "B", frame), "{output}");
        assert_eq!(count_glyph(&output, '►'), 2, "{output}");
        assert!(a.2 < b.1 && b.1 < c.1, "{output}");
        assert!(boxes_intact(&output, &["A", "B", "C"]), "{output}");
    }

    #[test]
    fn mermaid_non_member_declared_before_a_member_in_a_spanned_layer_stays_outside() {
        let output = mermaid(
            "flowchart LR\n    A --> B\n    B --> Y\n    B --> D\n    subgraph s\n    B\n    D\n    end\n",
        );
        let (Some(frame), Some(b), Some(d)) =
            (crossed_frame_of(&output, "s"), box_of(&output, "B"), box_of(&output, "D"))
        else {
            panic!("missing frame or box in\n{output}");
        };

        assert!(!output.starts_with("mermaid:"), "{output}");
        assert!(box_inside_frame(b, frame), "{output}");
        assert!(box_inside_frame(d, frame), "{output}");
        assert!(box_outside_frame(&output, "A", frame), "{output}");
        assert!(box_outside_frame(&output, "Y", frame), "{output}");
        assert_eq!(count_glyph(&output, '►'), 3, "{output}");
        assert!(boxes_intact(&output, &["A", "B", "Y", "D"]), "{output}");
    }

    #[test]
    fn mermaid_td_non_member_between_subgraph_members_is_placed_outside_the_frame() {
        let output =
            mermaid("flowchart TD\n    A --> B --> C\n    subgraph s\n    A\n    C\n    end\n");
        let (Some(frame), Some(a), Some(c), Some(a_bounds), Some(b_bounds), Some(c_bounds)) = (
            intact_crossed_frame(&output, "s"),
            box_of(&output, "A"),
            box_of(&output, "C"),
            box_bounds(&output, "A"),
            box_bounds(&output, "B"),
            box_bounds(&output, "C"),
        ) else {
            panic!("missing frame or box in\n{output}");
        };

        assert!(!output.starts_with("mermaid:"), "{output}");
        assert!(box_inside_frame(a, frame), "{output}");
        assert!(box_inside_frame(c, frame), "{output}");
        assert!(box_outside_frame(&output, "B", frame), "{output}");
        assert_eq!(count_glyph(&output, '▼'), 2, "{output}");
        assert!(a_bounds.2 < b_bounds.0 && b_bounds.0 < c_bounds.0, "{output}");
        assert!(boxes_intact(&output, &["A", "B", "C"]), "{output}");
    }

    #[test]
    fn mermaid_td_sibling_subgraphs_draw_disjoint_intact_frames() {
        // A title wider than its box widens the frame across the flow, towards the sibling.
        for (body, title) in [
            ("flowchart TD\n    subgraph a\n    A\n    end\n    subgraph b\n    B\n    end\n", "a"),
            (
                "flowchart TD\n    subgraph a [long title here]\n    A\n    end\n    subgraph b\n    B\n    end\n",
                "long title here",
            ),
        ] {
            let output = mermaid(body);
            let (Some(a), Some(b)) = (intact_frame(&output, title), intact_frame(&output, "b"))
            else {
                panic!("missing or broken frame in\n{output}");
            };
            let disjoint = a.2 < b.0 || b.2 < a.0 || a.3 < b.1 || b.3 < a.1;

            assert!(disjoint, "{output}");
            assert!(boxes_intact(&output, &["A", "B"]), "{output}");
            assert!(box_outside_frame(&output, "A", b), "{output}");
            assert!(box_outside_frame(&output, "B", a), "{output}");
        }
    }

    #[test]
    fn mermaid_non_member_in_a_layer_with_members_stays_outside_their_frame() {
        let output = mermaid(
            "flowchart LR\n    A --> X\n    B --> X\n    subgraph s\n    A\n    C\n    end\n",
        );
        let Some(frame) = intact_frame(&output, "s") else {
            panic!("missing or broken frame in\n{output}");
        };

        assert!(boxes_intact(&output, &["A", "B", "C", "X"]), "{output}");
        assert!(box_outside_frame(&output, "B", frame), "{output}");
        assert!(box_outside_frame(&output, "X", frame), "{output}");
    }

    #[test]
    fn mermaid_subgraph_frame_glyphs_are_muted_and_title_is_body_colored() {
        let lines =
            lay_out("```mermaid\nflowchart LR\n    subgraph lib\n    A\n    end\n```\n", None);
        let spans: Vec<&Span> = lines.iter().flatten().collect();
        let titles: Vec<&&Span> = spans.iter().filter(|span| span.text.contains("lib")).collect();
        let frame: Vec<&&Span> =
            spans.iter().filter(|span| span.text.chars().all(|c| "┌┐└┘─│".contains(c))).collect();

        assert_eq!(titles.len(), 1);
        assert!(titles.iter().all(|span| span.style.fg == Some(DARK_PALETTE.body)));
        assert!(!frame.is_empty());
        assert!(frame.iter().all(|span| span.style.fg == Some(DARK_PALETTE.muted)));
    }

    #[test]
    fn mermaid_end_without_subgraph_is_a_syntax_error() {
        assert_eq!(
            mermaid("flowchart LR\n    A\n    end\n"),
            "mermaid: line 3: \"end\" without an open subgraph\n```mermaid\nflowchart LR\n    A\n    end\n```"
        );
    }

    #[test]
    fn mermaid_unclosed_subgraph_is_a_syntax_error() {
        assert_eq!(
            mermaid("flowchart LR\n    subgraph s\n    A\n"),
            "mermaid: line 2: subgraph is not closed with \"end\"\n```mermaid\nflowchart LR\n    subgraph s\n    A\n```"
        );
    }

    #[test]
    fn mermaid_subgraph_with_unclosed_title_bracket_is_a_syntax_error() {
        assert_eq!(
            mermaid("flowchart LR\n    subgraph s [My Lib\n    A\n    end\n"),
            "mermaid: line 2: unclosed subgraph title\n```mermaid\nflowchart LR\n    subgraph s [My Lib\n    A\n    end\n```"
        );
    }

    #[test]
    fn mermaid_end_as_an_edge_target_is_a_syntax_error() {
        assert_eq!(
            mermaid("flowchart LR\n    A --> end\n"),
            "mermaid: line 2: \"end\" cannot be a node id\n```mermaid\nflowchart LR\n    A --> end\n```"
        );
    }

    #[test]
    fn mermaid_empty_subgraph_renders_as_a_titled_frame() {
        assert_eq!(mermaid("flowchart LR\n    subgraph s\n    end\n"), "┌─ s ─┐\n│     │\n└─────┘");
    }

    #[test]
    fn mermaid_empty_subgraph_beside_other_nodes_keeps_them_intact() {
        // A subgraph holding only an ignored statement is empty.
        let output =
            mermaid("flowchart LR\n    A --> B\n    subgraph s\n    style A fill:#f9f\n    end\n");
        let Some(frame) = intact_frame(&output, "s") else {
            panic!("missing or broken frame in\n{output}");
        };

        assert!(!output.starts_with("mermaid:"), "{output}");
        assert!(box_outside_frame(&output, "A", frame), "{output}");
        assert!(box_outside_frame(&output, "B", frame), "{output}");
        assert_eq!(count_glyph(&output, '►'), 1, "{output}");
        assert!(boxes_intact(&output, &["A", "B"]), "{output}");
    }

    #[test]
    fn mermaid_subgraph_with_its_own_direction_lays_out_its_members_that_way() {
        assert_eq!(
            mermaid("flowchart LR\n    subgraph s\n    direction TB\n    A --> B\n    end\n"),
            "┌─ s ───┐\n│ ┌───┐ │\n│ │ A │ │\n│ └───┘ │\n│   │   │\n│   │   │\n│   ▼   │\n│ ┌───┐ │\n│ │ B │ │\n│ └───┘ │\n└───────┘"
        );
    }

    #[test]
    fn mermaid_td_is_read_like_tb_inside_a_subgraph() {
        assert_eq!(
            mermaid("flowchart LR\n    subgraph s\n    direction TD\n    A --> B\n    end\n"),
            mermaid("flowchart LR\n    subgraph s\n    direction TB\n    A --> B\n    end\n")
        );
    }

    #[test]
    fn mermaid_link_into_a_unit_subgraph_ends_at_its_frame() {
        let output = mermaid(
            "flowchart LR\n    X --> s\n    subgraph s\n    direction TB\n    A --> B\n    end\n",
        );
        let (Some((top, left, bottom, _)), Some(x), Some(a), Some(b)) = (
            intact_frame(&output, "s"),
            box_of(&output, "X"),
            box_of(&output, "A"),
            box_of(&output, "B"),
        ) else {
            panic!("missing frame or box in\n{output}");
        };
        let into: Vec<_> = arrowheads(&output).into_iter().filter(|&(.., c)| c == '►').collect();

        assert_eq!(into.len(), 1, "{output}");
        assert!(
            into.iter().all(|&(row, col, _)| col + 1 == left && top < row && row < bottom),
            "{output}"
        );
        assert!(a.1 == b.1 && a.0 < b.0, "{output}");
        assert!(x.2 < left, "{output}");
        assert_eq!(count_glyph(&output, '▼'), 1, "{output}");
        assert!(boxes_intact(&output, &["X", "A", "B"]), "{output}");
    }

    #[test]
    fn mermaid_link_out_of_a_unit_subgraph_starts_at_its_frame() {
        let output = mermaid(
            "flowchart LR\n    subgraph s\n    direction TB\n    A --> B\n    end\n    s --> Y\n",
        );
        let (Some((.., right)), Some((y_row, y_left, _))) =
            (intact_frame(&output, "s"), box_of(&output, "Y"))
        else {
            panic!("missing frame or box in\n{output}");
        };
        let into: Vec<_> = arrowheads(&output).into_iter().filter(|&(.., c)| c == '►').collect();

        assert_eq!(into, vec![(y_row, y_left - 1, '►')], "{output}");
        assert!(is_line_glyph(glyph_at(&output, y_row, right + 1)), "{output}");
        assert!(y_left > right, "{output}");
        assert!(boxes_intact(&output, &["A", "B", "Y"]), "{output}");
    }

    #[test]
    fn mermaid_lr_self_loop_on_a_unit_whose_border_cannot_hold_its_legs_falls_back() {
        let body = "flowchart LR\n    subgraph s\n    direction TB\n    A\n    end\n    s --> s\n    s --> Y\n";

        assert_eq!(mermaid(body), format!("```mermaid\n{body}```"), "{body}");
    }

    #[test]
    fn mermaid_subgraph_direction_is_ignored_when_a_member_links_outside() {
        let output = mermaid(
            "flowchart LR\n    subgraph s\n    direction TB\n    A --> B\n    end\n    X --> A\n",
        );
        let (Some(frame), Some(a), Some(b)) =
            (intact_crossed_frame(&output, "s"), box_of(&output, "A"), box_of(&output, "B"))
        else {
            panic!("missing frame or box in\n{output}");
        };

        assert!(is_lr_chain(&output, &["X", "A", "B"]), "{output}");
        assert!(box_inside_frame(a, frame) && box_inside_frame(b, frame), "{output}");
        assert!(box_outside_frame(&output, "X", frame), "{output}");
    }

    #[test]
    fn mermaid_subgraph_direction_is_kept_when_only_its_id_links_outside() {
        let output = mermaid(
            "flowchart LR\n    subgraph subgraph1\n    direction TB\n    top1 --> bottom1\n    end\n    subgraph subgraph2\n    direction TB\n    top2 --> bottom2\n    end\n    outside --> subgraph1\n    outside ---> top2\n",
        );
        let find = |title: &str| {
            intact_frame(&output, title).or_else(|| intact_crossed_frame(&output, title))
        };
        let (Some(one), Some(two)) = (find("subgraph1"), find("subgraph2")) else {
            panic!("missing frame in\n{output}");
        };
        let (Some(top1), Some(bottom1), Some(top2), Some(bottom2), Some(outside)) = (
            box_of(&output, "top1"),
            box_of(&output, "bottom1"),
            box_of(&output, "top2"),
            box_of(&output, "bottom2"),
            box_of(&output, "outside"),
        ) else {
            panic!("missing box in\n{output}");
        };
        let into_one = arrowheads(&output)
            .into_iter()
            .filter(|&(row, col, c)| c == '►' && col + 1 == one.1 && one.0 < row && row < one.2)
            .count();

        // A TB child is centred under its parent, so boxes of different widths share
        // their centre column rather than their left border. A box of even width has no
        // middle cell; the layout takes the one right of the middle, `left + width / 2`.
        let centre = |(_, left, right): (usize, usize, usize)| (left + right).div_ceil(2);
        assert!(centre(top1) == centre(bottom1) && top1.0 < bottom1.0, "{output}");
        assert!(top2.0 == bottom2.0 && top2.2 < bottom2.1, "{output}");
        assert!(outside.2 < one.1 && outside.2 < two.1, "{output}");
        assert_eq!(into_one, 1, "{output}");
        assert!(
            boxes_intact(&output, &["outside", "top1", "bottom1", "top2", "bottom2"]),
            "{output}"
        );
    }

    #[test]
    fn mermaid_nested_unit_subgraphs_each_follow_their_own_direction() {
        let output = mermaid(
            "flowchart LR\n    subgraph TOP\n    direction TB\n    subgraph B1\n    direction RL\n    i1 -->f1\n    end\n    subgraph B2\n    direction BT\n    i2 -->f2\n    end\n    end\n    A --> TOP --> B\n    B1 --> B2\n",
        );
        let (Some(top), Some(b1), Some(b2)) =
            (frame_of(&output, "TOP"), frame_of(&output, "B1"), frame_of(&output, "B2"))
        else {
            panic!("missing frame in\n{output}");
        };
        let labels = ["A", "B", "i1", "f1", "i2", "f2"];
        let Some([a, b, i1, f1, i2, f2]) = labels
            .iter()
            .map(|label| box_bounds(&output, label))
            .collect::<Option<Vec<_>>>()
            .and_then(|bounds| <[_; 6]>::try_from(bounds).ok())
        else {
            panic!("missing box in\n{output}");
        };
        let (Some(f1_row), Some(i1_row)) = (box_of(&output, "f1"), box_of(&output, "i1")) else {
            panic!("missing box in\n{output}");
        };
        let arrows = arrowheads(&output);
        let with = |glyph: char| -> Vec<(usize, usize)> {
            arrows.iter().filter(|&&(.., c)| c == glyph).map(|&(row, col, _)| (row, col)).collect()
        };

        assert!(frame_inside_frame(b1, top) && frame_inside_frame(b2, top), "{output}");
        assert!(b1.2 < b2.0, "{output}");
        assert!(f1.3 < i1.1 && f1_row.0 == i1_row.0, "{output}");
        assert_eq!(with('◄'), vec![(f1_row.0, f1.3 + 1)], "{output}");
        assert!(f2.0 < i2.0 && f2.1 == i2.1, "{output}");
        let up = with('▲');
        assert_eq!(up.len(), 1, "{output}");
        assert!(
            up.iter().all(|&(row, col)| row == f2.2 + 1 && f2.1 < col && col < f2.3),
            "{output}"
        );
        assert!(a.3 < top.1 && top.1 < top.3 && top.3 < b.1, "{output}");
        let down = with('▼');
        assert_eq!(down.len(), 1, "{output}");
        assert!(down.iter().all(|&(row, _)| row + 1 == b2.0), "{output}");
        let mut right = with('►');
        right.sort_unstable_by_key(|&(_, col)| col);
        assert_eq!(right.len(), 2, "{output}");
        assert!(right.first().is_some_and(|&(_, col)| col + 1 == top.1), "{output}");
        assert!(right.last().is_some_and(|&(_, col)| col + 1 == b.1), "{output}");
        assert_eq!(arrows.len(), 5, "{output}");
        assert!(boxes_intact(&output, &["i1", "f1", "i2", "f2"]), "{output}");
        // The titles `B1` and `B2` contain `B`, which `boxes_intact` requires to occur once.
        for (top, left, bottom, right) in [a, b] {
            assert_eq!(glyph_at(&output, top, left), Some('┌'), "{output}");
            assert_eq!(glyph_at(&output, top, right), Some('┐'), "{output}");
            assert_eq!(glyph_at(&output, bottom, left), Some('└'), "{output}");
            assert_eq!(glyph_at(&output, bottom, right), Some('┘'), "{output}");
        }
    }

    #[test]
    fn mermaid_nested_subgraph_draws_a_frame_inside_a_frame() {
        assert_eq!(
            mermaid("flowchart LR\n    subgraph a\n    subgraph b\n    A\n    end\n    end\n"),
            "┌─ a ───────┐\n│ ┌─ b ───┐ │\n│ │ ┌───┐ │ │\n│ │ │ A │ │ │\n│ │ └───┘ │ │\n│ └───────┘ │\n└───────────┘"
        );
    }

    #[test]
    fn mermaid_nested_subgraph_member_belongs_to_both_frames() {
        let output = mermaid(
            "flowchart LR\n    subgraph a\n    X\n    subgraph b\n    A\n    end\n    end\n",
        );
        let (Some(a), Some(b), Some(x), Some(inner)) = (
            intact_frame(&output, "a"),
            intact_frame(&output, "b"),
            box_of(&output, "X"),
            box_of(&output, "A"),
        ) else {
            panic!("missing frame or box in\n{output}");
        };

        assert!(frame_inside_frame(b, a), "{output}");
        assert!(box_inside_frame(inner, b), "{output}");
        assert!(box_inside_frame(x, a), "{output}");
        assert!(box_outside_frame(&output, "X", b), "{output}");
        assert!(boxes_intact(&output, &["X", "A"]), "{output}");
    }

    #[test]
    fn mermaid_link_from_outside_into_a_nested_member_crosses_both_frames() {
        let output = mermaid(
            "flowchart LR\n    S --> A\n    subgraph a\n    subgraph b\n    A\n    end\n    end\n",
        );
        let (Some(a), Some(b), Some((row, left, _))) = (
            intact_crossed_frame(&output, "a"),
            intact_crossed_frame(&output, "b"),
            box_of(&output, "A"),
        ) else {
            panic!("missing frame or box in\n{output}");
        };

        assert_eq!(arrowheads(&output), vec![(row, left - 1, '►')], "{output}");
        assert_eq!(glyph_at(&output, row, a.1), Some('┼'), "{output}");
        assert_eq!(glyph_at(&output, row, b.1), Some('┼'), "{output}");
        assert!(box_outside_frame(&output, "S", a), "{output}");
        assert!(box_outside_frame(&output, "S", b), "{output}");
        assert!(boxes_intact(&output, &["S", "A"]), "{output}");
    }

    #[test]
    fn mermaid_link_to_a_nested_subgraph_id_ends_at_the_inner_frame() {
        let output = mermaid(
            "flowchart LR\n    S --> b\n    subgraph a\n    subgraph b\n    A\n    end\n    end\n",
        );
        let (Some(a), Some((top, left, bottom, _))) =
            (crossed_frame_of(&output, "a"), crossed_frame_of(&output, "b"))
        else {
            panic!("missing frame in\n{output}");
        };
        let arrows = arrowheads(&output);

        assert_eq!(box_of(&output, "b"), None, "{output}");
        assert_eq!(arrows.len(), 1, "{output}");
        assert!(
            arrows.iter().all(|&(row, col, c)| {
                c == '►'
                    && col + 1 == left
                    && top < row
                    && row < bottom
                    && glyph_at(&output, row, a.1) == Some('┼')
            }),
            "{output}"
        );
        assert!(boxes_intact(&output, &["S", "A"]), "{output}");
    }

    #[test]
    fn mermaid_link_to_an_outer_subgraph_places_inner_members_after_the_source() {
        let output = mermaid(
            "flowchart LR\n    S --> a\n    subgraph a\n    X\n    subgraph b\n    A\n    end\n    end\n",
        );
        let (Some((_, left, ..)), Some(s), Some(x), Some(inner)) = (
            crossed_frame_of(&output, "a"),
            box_of(&output, "S"),
            box_of(&output, "X"),
            box_of(&output, "A"),
        ) else {
            panic!("missing frame or box in\n{output}");
        };
        let arrows = arrowheads(&output);

        assert_eq!(arrows.len(), 1, "{output}");
        assert!(arrows.iter().all(|&(_, col, c)| c == '►' && col + 1 == left), "{output}");
        assert!(x.1 > s.2 && inner.1 > s.2, "{output}");
        assert!(boxes_intact(&output, &["S", "X", "A"]), "{output}");
    }

    #[test]
    fn mermaid_three_levels_of_nesting_draw_three_frames() {
        let output = mermaid(
            "flowchart TD\n    subgraph a\n    subgraph b\n    subgraph c\n    A\n    end\n    end\n    end\n",
        );
        let (Some(a), Some(b), Some(c), Some(inner)) = (
            intact_frame(&output, "a"),
            intact_frame(&output, "b"),
            intact_frame(&output, "c"),
            box_of(&output, "A"),
        ) else {
            panic!("missing frame or box in\n{output}");
        };

        assert!(frame_inside_frame(c, b), "{output}");
        assert!(frame_inside_frame(b, a), "{output}");
        assert!(box_inside_frame(inner, c), "{output}");
        assert!(boxes_intact(&output, &["A"]), "{output}");
    }

    #[test]
    fn mermaid_td_link_across_nested_frames_crosses_the_inner_top_border() {
        let output = mermaid(
            "flowchart TD\n    subgraph a\n    X --> A\n    subgraph b\n    A\n    end\n    end\n",
        );
        let (Some(b), Some(_), Some((a_top, a_left, _, a_right))) = (
            intact_crossed_frame(&output, "b"),
            intact_frame(&output, "a"),
            box_bounds(&output, "A"),
        ) else {
            panic!("missing frame or box in\n{output}");
        };
        let arrows = arrowheads(&output);

        assert_eq!(arrows.len(), 1, "{output}");
        assert!(
            arrows.iter().all(|&(row, col, c)| {
                c == '▼'
                    && row + 1 == a_top
                    && a_left < col
                    && col < a_right
                    && glyph_at(&output, b.0, col) == Some('┼')
            }),
            "{output}"
        );
        assert_eq!(
            words(output.lines().nth(b.0).unwrap_or_default()).filter(|&word| word == "b").count(),
            1,
            "{output}"
        );
        assert!(boxes_intact(&output, &["X", "A"]), "{output}");
    }

    #[test]
    fn mermaid_example_document_nested_subgraph_flowchart_renders() {
        let document = plain(include_str!("../../../examples/EXAMPLE.md"), None);
        let output = mermaid(
            "flowchart TD\n    subgraph services [ServicesLayer]\n    Svc1[Receive request] --> Svc2[Validate payload]\n    subgraph adapters [AdaptersLayer]\n    A1[DB adapter] --> A2[Cache adapter]\n    end\n    Svc2 --> A1\n    end\n    A2 --> Out([Done])\n",
        );
        let (Some(services), Some(adapters)) = (
            crossed_frame_of(&output, "ServicesLayer"),
            crossed_frame_of(&output, "AdaptersLayer"),
        ) else {
            panic!("missing frame in\n{output}");
        };
        let inside =
            |label: &str, frame| box_of(&output, label).is_some_and(|b| box_inside_frame(b, frame));
        let done_row = output.lines().position(|line| line.contains("( Done )"));

        assert!(!document.contains("subgraph adapters [AdaptersLayer]"), "{document}");
        assert!(
            document.contains("ServicesLayer") && document.contains("AdaptersLayer"),
            "{document}"
        );
        assert!(frame_inside_frame(adapters, services), "{output}");
        for label in ["Receive request", "Validate payload"] {
            assert!(inside(label, services), "{label} in\n{output}");
            assert!(box_outside_frame(&output, label, adapters), "{label} in\n{output}");
        }
        for label in ["DB adapter", "Cache adapter"] {
            assert!(inside(label, adapters), "{label} in\n{output}");
        }
        assert!(done_row.is_some_and(|row| row > services.2), "{output}");
        assert_eq!(count_glyph(&output, '▼'), 4, "{output}");
        assert!(
            boxes_intact(
                &output,
                &["Receive request", "Validate payload", "DB adapter", "Cache adapter"]
            ),
            "{output}"
        );
    }

    #[test]
    fn mermaid_direction_before_a_word_that_names_no_direction_is_a_syntax_error() {
        for body in [
            "flowchart LR\n    subgraph s\n    direction XY\n    A\n    end\n",
            "flowchart LR\n    A\n    direction XY\n",
        ] {
            assert_eq!(
                mermaid(body),
                format!("mermaid: line 3: expected a link\n```mermaid\n{body}```"),
                "{body}"
            );
        }
    }

    #[test]
    fn mermaid_direction_before_a_link_is_a_node_like_mermaid() {
        for body in [
            "flowchart LR\n    direction --> A\n",
            "flowchart LR\n    subgraph s\n    direction --> A\n    end\n",
        ] {
            let output = mermaid(body);

            assert!(boxes_intact(&output, &["direction", "A"]), "{body}\n{output}");
            assert_eq!(arrowheads(&output).len(), 1, "{body}\n{output}");
        }
    }

    #[test]
    fn mermaid_top_level_direction_statement_is_ignored_like_mermaid() {
        assert_eq!(mermaid("flowchart LR\n    direction TB\n    A --> B\n"), mermaid(LR_A_TO_B));
    }

    #[test]
    fn mermaid_direction_without_a_direction_word_is_a_node_like_mermaid() {
        for body in [
            "flowchart LR\n    direction\n",
            "flowchart LR\n    subgraph s\n    direction\n    end\n",
        ] {
            let output = mermaid(body);

            assert!(box_of(&output, "direction").is_some(), "{body}\n{output}");
        }
    }

    #[test]
    fn mermaid_wider_than_the_width_is_drawn_with_tighter_spacing() {
        let body = "flowchart LR\n    A --> B --> C\n";
        let width = widest(&mermaid(body)) - 1;
        let output = mermaid_in(body, Some(width));

        assert!(!output.starts_with("```"), "fell back to the code block:\n{output}");
        assert!(widest(&output) <= width, "{output}");
        assert!(boxes_intact(&output, &["A", "B", "C"]), "{output}");
        assert_eq!(count_glyph(&output, '►'), 2, "{output}");
    }

    #[test]
    fn mermaid_in_a_block_quote_fits_the_width_left_after_its_prefix() {
        let output = plain("> ```mermaid\n> flowchart LR\n> A --> B --> C\n> ```\n", Some(21));

        assert!(!output.contains("```"), "fell back to the code block:\n{output}");
        assert!(output.contains("│ C │"), "{output}");
        assert!(widest(&output) <= 21, "{output}");
        assert!(output.lines().all(|line| line.starts_with("│ ")), "{output}");
    }

    #[test]
    fn mermaid_in_a_list_item_fits_the_width_left_after_its_marker() {
        let output = plain("- ```mermaid\n  flowchart LR\n  A --> B --> C\n  ```\n", Some(21));
        let mut lines = output.lines();

        assert!(!output.contains("```"), "fell back to the code block:\n{output}");
        assert!(output.contains("│ C │"), "{output}");
        assert!(widest(&output) <= 21, "{output}");
        assert!(lines.next().is_some_and(|line| line.starts_with("• ")), "{output}");
        assert!(lines.all(|line| line.starts_with("  ")), "{output}");
    }

    #[test]
    fn mermaid_wide_enough_width_keeps_the_default_gap() {
        let body = "flowchart LR\n    A --> B --> C\n";

        assert_eq!(mermaid_in(body, Some(25)), mermaid(body));
    }

    #[test]
    fn mermaid_labelled_link_too_wide_for_its_label_falls_back_without_a_reason_line() {
        let body = "flowchart LR\n    A -->|approved| B\n";

        assert_eq!(mermaid_in(body, Some(19)), format!("```mermaid\n{body}```"));
    }

    #[test]
    fn mermaid_td_fits_width_by_shrinking_the_sibling_gap() {
        let body = "flowchart TD\n    A --> B\n    A --> C\n";
        let width = widest(&mermaid(body)) - 1;
        let output = mermaid_in(body, Some(width));

        assert!(!output.starts_with("```"), "fell back to the code block:\n{output}");
        assert!(widest(&output) <= width, "{output}");
        assert!(boxes_intact(&output, &["A", "B", "C"]), "{output}");
    }

    #[test]
    fn mermaid_too_wide_after_compaction_falls_back_without_a_reason_line() {
        let body = "flowchart LR\n    A --> B --> C\n";

        assert_eq!(mermaid_in(body, Some(18)), format!("```mermaid\n{body}```"));
    }

    #[test]
    fn mermaid_drawing_larger_than_a_million_cells_falls_back_without_a_reason_line() {
        let body = huge_drawing_body();

        assert_eq!(mermaid(&body), format!("```mermaid\n{body}```"));
    }

    #[test]
    fn mermaid_drawing_larger_than_a_million_cells_at_the_default_spacing_is_drawn_tighter() {
        // A chain of 100 layers beside a layer stacking 231 boxes: about 1 190 columns
        // by 923 rows with the default gap of five, and about 990 columns with a gap of
        // three, which brings the drawing under a million cells.
        let mut body = String::from("flowchart LR\n");
        for node in 0..99 {
            body.push_str(&format!("    C{node} --> C{}\n", node + 1));
        }
        let targets: Vec<String> = (1..=230).map(|index| format!("T{index}")).collect();
        body.push_str(&format!("    C0 --> {}\n", targets.join(" & ")));
        let output = mermaid(&body);

        assert!(!output.starts_with("```"), "fell back to the code block");
        assert_eq!(output.matches("│ C99 │").count(), 1);
        assert_eq!(output.matches("│ T230 │").count(), 1);
        let (columns, rows) =
            (output.lines().map(UnicodeWidthStr::width).max(), output.lines().count());
        assert!(columns.is_some_and(|columns| columns * rows <= 1_000_000));
    }

    #[test]
    fn mermaid_more_than_five_hundred_edges_is_an_error_like_mermaid() {
        // 23 × 22 = 506 edges; the 501st is on line 502, after the header and 500 accepted
        // edges.
        let mut body = String::from("flowchart LR\n");
        for from in 0..23 {
            for to in (0..23).filter(|&to| to != from) {
                body.push_str(&format!("N{from} --> N{to}\n"));
            }
        }

        assert_eq!(
            mermaid(&body),
            format!("mermaid: line 502: too many edges (limit 500)\n```mermaid\n{body}```")
        );
    }

    #[test]
    fn mermaid_text_limit_is_fifty_thousand_characters_inclusive() {
        // `flowchart LR\nA[` and `]\n` take 17 characters around the label. A label of
        // three-byte characters puts both texts over the limit in bytes, so that only
        // the count of characters decides.
        let at_limit = format!("flowchart LR\nA[{}]\n", "あ".repeat(50_000 - 17));
        let over_limit = format!("flowchart LR\nA[{}]\n", "あ".repeat(50_001 - 17));

        assert_eq!(at_limit.chars().count(), 50_000);
        let output = mermaid(&at_limit);
        assert!(output.starts_with('┌'), "not drawn");
        assert_eq!(
            mermaid(&over_limit),
            format!("mermaid: diagram text exceeds 50000 characters\n```mermaid\n{over_limit}```")
        );
    }

    #[test]
    fn mermaid_repeated_references_inside_a_subgraph_draw_one_intact_frame() {
        let output = mermaid(
            "flowchart LR\n    subgraph s\n    A --> B\n    A --> C\n    A --> D\n    end\n",
        );

        assert_eq!(output.matches("─ s ─").count(), 1, "{output}");
        assert!(intact_frame(&output, "s").is_some(), "{output}");
        assert!(boxes_intact(&output, &["A", "B", "C", "D"]), "{output}");
    }

    #[test]
    fn mermaid_subgraph_title_without_brackets_is_the_rest_of_the_statement() {
        let output = mermaid("flowchart LR\n    subgraph Data Layer\n    A\n    end\n");

        assert!(intact_frame(&output, "Data Layer").is_some(), "{output}");
        assert!(boxes_intact(&output, &["A"]), "{output}");
    }

    #[test]
    fn mermaid_link_into_a_subgraph_ends_at_its_frame() {
        let output = mermaid("flowchart LR\n    A --> s\n    subgraph s\n    B\n    end\n");
        let (Some((top, left, bottom, _)), Some((_, _, a_right))) =
            (frame_of(&output, "s"), box_of(&output, "A"))
        else {
            panic!("missing frame or box in\n{output}");
        };
        let arrows = arrowheads(&output);

        assert!(!output.starts_with("mermaid:"), "{output}");
        assert_eq!(box_of(&output, "s"), None, "{output}");
        assert_eq!(arrows.len(), 1, "{output}");
        assert!(
            arrows
                .iter()
                .all(|&(row, col, c)| c == '►' && col + 1 == left && top < row && row < bottom),
            "{output}"
        );
        assert!(a_right < left, "{output}");
        assert!(boxes_intact(&output, &["A", "B"]), "{output}");
    }

    #[test]
    fn mermaid_td_link_between_members_spanning_a_layer_stays_inside_the_frame() {
        let output = mermaid("flowchart TD\n    subgraph s\n    A ---> B\n    end\n    A --> C\n");
        let Some((top, left, bottom, right)) = intact_frame(&output, "s") else {
            panic!("missing or broken frame in\n{output}");
        };
        let (Some(a), Some(b), Some(c)) =
            (box_bounds(&output, "A"), box_bounds(&output, "B"), box_bounds(&output, "C"))
        else {
            panic!("missing box in\n{output}");
        };
        let inside = |(box_top, box_left, box_bottom, box_right): (usize, usize, usize, usize)| {
            top < box_top && box_bottom < bottom && left < box_left && box_right < right
        };
        let Some((b_row, b_left, b_right)) = box_of(&output, "B") else {
            panic!("missing box in\n{output}");
        };
        let b_centre = (b_left + b_right) / 2;

        assert!(inside(a) && inside(b), "{output}");
        assert!(c.1 > right, "{output}");
        assert_eq!(count_glyph(&output, '┼'), 1, "{output}");
        assert_eq!(count_glyph(&output, '▼'), 2, "{output}");
        assert_eq!(glyph_at(&output, b_row - 2, b_centre), Some('▼'), "{output}");
        assert!(left < b_centre && b_centre < right, "{output}");
        assert!(boxes_intact(&output, &["A", "B", "C"]), "{output}");
    }

    #[test]
    fn mermaid_lr_link_between_members_spanning_a_layer_stays_inside_the_frame() {
        let output = mermaid("flowchart LR\n    subgraph s\n    A ---> B\n    end\n    A --> C\n");
        // A→C leaves the frame through its bottom border, where it crosses with `┼`.
        let Some((top, left, bottom, right)) = intact_crossed_frame(&output, "s") else {
            panic!("missing or broken frame in\n{output}");
        };
        let (Some(a), Some(b), Some(c)) =
            (box_bounds(&output, "A"), box_bounds(&output, "B"), box_bounds(&output, "C"))
        else {
            panic!("missing box in\n{output}");
        };
        let inside = |(box_top, box_left, box_bottom, box_right): (usize, usize, usize, usize)| {
            top < box_top && box_bottom < bottom && left < box_left && box_right < right
        };
        let Some((b_row, b_left, _)) = box_of(&output, "B") else {
            panic!("missing box in\n{output}");
        };

        assert!(inside(a) && inside(b), "{output}");
        assert!(c.0 > bottom, "{output}");
        assert_eq!(count_glyph(&output, '┼'), 1, "{output}");
        assert_eq!(count_glyph(&output, '►'), 2, "{output}");
        assert_eq!(glyph_at(&output, b_row, b_left - 1), Some('►'), "{output}");
        assert!(top < b_row && b_row < bottom, "{output}");
        assert!(boxes_intact(&output, &["A", "B", "C"]), "{output}");
    }

    #[test]
    fn mermaid_self_loop_inside_a_subgraph_stays_inside_an_intact_frame() {
        for direction in ["TD", "LR"] {
            let output =
                mermaid(&format!("flowchart {direction}\n    subgraph s\n    A --> A\n    end\n"));
            let Some((top, left, bottom, right)) = intact_frame(&output, "s") else {
                panic!("missing or broken frame in\n{output}");
            };
            let heads = arrowheads(&output);

            assert_eq!(heads.len(), 1, "{output}");
            assert!(
                heads
                    .iter()
                    .all(|&(row, col, _)| top < row && row < bottom && left < col && col < right),
                "{output}"
            );
            assert!(boxes_intact(&output, &["A"]), "{output}");
            let border = (left..=right)
                .flat_map(|col| [(top, col), (bottom, col)])
                .chain((top..=bottom).flat_map(|row| [(row, left), (row, right)]));
            let joints: Vec<(usize, usize)> = border
                .filter(|&(row, col)| {
                    glyph_at(&output, row, col).is_some_and(|c| "┼├┤┬┴".contains(c))
                })
                .collect();
            assert!(joints.is_empty(), "{output}");
        }
    }

    #[test]
    fn mermaid_edge_label_does_not_overwrite_a_frame_border() {
        // The second link into B makes the first turn, so the run its label sits beside
        // crosses the frame border.
        let output = mermaid(
            "flowchart LR\n    A -->|yes| B\n    C --> B\n    subgraph s\n    B\n    end\n",
        );

        assert_eq!(whole_words(&output, "yes"), 1, "{output}");
        assert!(intact_frame(&output, "s").is_some(), "{output}");
        assert!(boxes_intact(&output, &["A", "B", "C"]), "{output}");
    }

    #[test]
    fn mermaid_quoted_edge_label_drops_its_quotes_and_may_contain_pipes() {
        assert_eq!(mermaid("flowchart LR\n    A -->|\"yes\"| B\n"), mermaid(LR_A_YES_B));
        assert_eq!(
            mermaid("flowchart LR\n    A -->|\"a|b\"| B\n").lines().nth(1),
            Some("│ A │─a|b─►│ B │")
        );
    }

    #[test]
    fn mermaid_quoted_text_form_label_drops_its_quotes() {
        for (quoted, bare) in [
            ("A -- \"yes\" --> B", "A -- yes --> B"),
            ("A -. \"yes\" .-> B", "A -. yes .-> B"),
            ("A == \"yes\" ==> B", "A == yes ==> B"),
        ] {
            assert_eq!(
                mermaid(&format!("flowchart LR\n    {quoted}\n")),
                mermaid(&format!("flowchart LR\n    {bare}\n")),
                "{quoted}"
            );
        }
        for (link, line) in [
            ("A -- \"a--b\" --> B", "│ A │─a--b─►│ B │"),
            ("A -. \"a.-b\" .-> B", "│ A │┄a.-b┄►│ B │"),
        ] {
            let output = mermaid(&format!("flowchart LR\n    {link}\n"));

            assert_eq!(output.lines().nth(1), Some(line), "{link}");
        }
    }

    #[test]
    fn mermaid_quoted_subgraph_title_drops_its_quotes() {
        assert_eq!(
            mermaid("flowchart LR\n    subgraph \"My title\"\n    A\n    end\n"),
            mermaid("flowchart LR\n    subgraph s [My title]\n    A\n    end\n")
        );
    }

    #[test]
    fn mermaid_link_to_a_subgraph_declared_later_renders_the_same() {
        assert_eq!(
            mermaid("flowchart LR\n    A --> s\n    subgraph s\n    B\n    end\n"),
            mermaid("flowchart LR\n    subgraph s\n    B\n    end\n    A --> s\n")
        );
    }

    #[test]
    fn mermaid_link_out_of_a_subgraph_starts_at_its_frame() {
        let output = mermaid("flowchart LR\n    subgraph s\n    B\n    end\n    s --> C\n");
        let (Some((_, _, _, right)), Some((c_row, c_left, _))) =
            (frame_of(&output, "s"), box_of(&output, "C"))
        else {
            panic!("missing frame or box in\n{output}");
        };

        assert_eq!(box_of(&output, "s"), None, "{output}");
        assert_eq!(count_glyph(&output, '►'), 1, "{output}");
        assert_eq!(glyph_at(&output, c_row, c_left - 1), Some('►'), "{output}");
        assert!(is_line_glyph(glyph_at(&output, c_row, right + 1)), "{output}");
        assert!(c_left > right, "{output}");
        assert!(boxes_intact(&output, &["B", "C"]), "{output}");
    }

    #[test]
    fn mermaid_link_into_a_subgraph_places_every_member_after_the_source() {
        let output = mermaid("flowchart LR\n    A --> s\n    subgraph s\n    B\n    C\n    end\n");
        let (Some((top, left, bottom, right)), Some(a), Some(b), Some(c)) = (
            frame_of(&output, "s"),
            box_of(&output, "A"),
            box_of(&output, "B"),
            box_of(&output, "C"),
        ) else {
            panic!("missing frame or box in\n{output}");
        };

        for (row, box_left, box_right) in [b, c] {
            assert!(box_left > a.2, "{output}");
            assert!(top < row && row < bottom && left < box_left && box_right < right, "{output}");
        }
        assert_eq!(count_glyph(&output, '►'), 1, "{output}");
    }

    #[test]
    fn mermaid_link_out_of_a_subgraph_places_the_target_after_every_member() {
        let output = mermaid("flowchart LR\n    subgraph s\n    B --> C\n    end\n    s --> D\n");
        let (Some((.., right)), Some((.., c_right)), Some((_, d_left, _))) =
            (frame_of(&output, "s"), box_of(&output, "C"), box_of(&output, "D"))
        else {
            panic!("missing frame or box in\n{output}");
        };

        assert!(d_left > c_right && d_left > right, "{output}");
        assert_eq!(count_glyph(&output, '►'), 2, "{output}");
    }

    #[test]
    fn mermaid_td_link_into_a_subgraph_enters_its_top_border_beside_the_title() {
        let output = mermaid("flowchart TD\n    A --> s\n    subgraph s\n    B\n    end\n");
        let Some((top, left, _, right)) = crossed_frame_of(&output, "s") else {
            panic!("missing frame in\n{output}");
        };
        let arrows = arrowheads(&output);
        let Some(title) = title_cells(&output, top, left, "s") else {
            panic!("missing title in\n{output}");
        };

        assert_eq!(arrows.len(), 1, "{output}");
        assert!(
            arrows.iter().all(|&(row, col, c)| {
                c == '▼'
                    && row + 1 == top
                    && left < col
                    && col < right
                    && !title.contains(&col)
                    && glyph_at(&output, top, col) == Some('─')
            }),
            "{output}"
        );
        assert_eq!(
            words(output.lines().nth(top).unwrap_or_default()).filter(|&word| word == "s").count(),
            1,
            "{output}"
        );
        assert!(boxes_intact(&output, &["A", "B"]), "{output}");
    }

    #[test]
    fn mermaid_link_between_two_subgraphs_joins_their_frames() {
        let output = mermaid(
            "flowchart LR\n    subgraph one\n    A\n    end\n    subgraph two\n    B\n    end\n    one --> two\n",
        );
        let (Some(one), Some(two)) = (frame_of(&output, "one"), frame_of(&output, "two")) else {
            panic!("missing frame in\n{output}");
        };
        let arrows = arrowheads(&output);

        assert_eq!(box_of(&output, "one"), None, "{output}");
        assert_eq!(box_of(&output, "two"), None, "{output}");
        assert_eq!(arrows.len(), 1, "{output}");
        assert!(arrows.iter().all(|&(_, col, c)| c == '►' && col + 1 == two.1), "{output}");
        assert!(one.3 < two.1, "{output}");
        assert!(boxes_intact(&output, &["A", "B"]), "{output}");
    }

    #[test]
    fn mermaid_label_on_a_link_into_a_subgraph_sits_before_the_frame() {
        let output = mermaid("flowchart LR\n    A -->|yes| s\n    subgraph s\n    B\n    end\n");
        let Some((_, left, ..)) = frame_of(&output, "s") else {
            panic!("missing frame in\n{output}");
        };
        let (_, col) = label_on_line(&output, "yes");

        assert_eq!(whole_words(&output, "yes"), 1, "{output}");
        assert_eq!(count_glyph(&output, '►'), 1, "{output}");
        assert_eq!(box_of(&output, "s"), None, "{output}");
        assert!(col + "yes".width() < left, "{output}");
        // The label does not take the arrowhead's cell, which is just outside the frame.
        assert!(
            arrowheads(&output).iter().all(|&(_, arrow_col, _)| arrow_col + 1 == left),
            "{output}"
        );
    }

    #[test]
    fn mermaid_link_from_a_member_to_its_own_subgraph_falls_back_without_a_reason_line() {
        let body = "flowchart LR\n    subgraph s\n    A\n    end\n    A --> s\n";

        assert_eq!(mermaid(body), format!("```mermaid\n{body}```"), "{body}");
    }

    #[test]
    fn mermaid_subgraph_listed_in_two_blocks_that_do_not_nest_falls_back() {
        let body = "flowchart LR\n    subgraph a\n    x\n    end\n    subgraph b\n    x\n    end\n    subgraph x\n    C\n    end\n";

        assert_eq!(mermaid(body), format!("```mermaid\n{body}```"), "{body}");
    }

    #[test]
    fn mermaid_subgraphs_listed_in_each_other_fall_back() {
        let body = "flowchart LR\n    subgraph a\n    b\n    end\n    subgraph b\n    a\n    end\n";

        assert_eq!(mermaid(body), format!("```mermaid\n{body}```"), "{body}");
    }

    #[test]
    fn mermaid_link_from_a_member_of_a_directed_subgraph_to_another_subgraph_falls_back() {
        let body = "flowchart LR\n    subgraph s\n    direction TB\n    A\n    end\n    subgraph t\n    B\n    end\n    A --> t\n";

        assert_eq!(mermaid(body), format!("```mermaid\n{body}```"), "{body}");
    }

    #[test]
    fn mermaid_links_into_an_empty_subgraph_top_down_enter_its_top_border_apart() {
        // The number of arrowheads just above the top border of frame `s`, and the
        // frame's width.
        let entries = |body: &str| {
            let output = mermaid(body);
            let (top, left, _, right) = intact_crossed_frame(&output, "s")?;
            let arrows = (left + 1..right)
                .filter(|&col| glyph_at(&output, top - 1, col) == Some('▼'))
                .count();
            Some((arrows, right - left))
        };
        let one = entries("flowchart TD\n    A --> s\n    subgraph s\n    end\n");
        let two = entries("flowchart TD\n    A --> s\n    B --> s\n    subgraph s\n    end\n");

        let (Some((1, one_width)), Some((2, two_width))) = (one, two) else {
            panic!("{one:?} {two:?}");
        };
        assert!(one_width < two_width, "{one_width} {two_width}");
    }

    #[test]
    fn mermaid_cycle_through_a_subgraph_end_draws_the_later_link_backwards() {
        let output =
            mermaid("flowchart LR\n    A --> s\n    s --> A\n    subgraph s\n    B\n    end\n");
        let (Some((top, left, bottom, _)), Some((a_top, _, a_bottom, a_right))) =
            (intact_frame(&output, "s"), box_bounds(&output, "A"))
        else {
            panic!("missing frame or box in\n{output}");
        };
        let heads = |glyph: char| -> Vec<(usize, usize)> {
            arrowheads(&output)
                .into_iter()
                .filter(|&(.., c)| c == glyph)
                .map(|(row, col, _)| (row, col))
                .collect()
        };
        let (into, back) = (heads('►'), heads('◄'));

        assert!(!output.starts_with("mermaid:"), "{output}");
        assert!(a_right < left, "{output}");
        assert_eq!(into.len(), 1, "{output}");
        assert!(
            into.iter().all(|&(row, col)| col + 1 == left && top < row && row < bottom),
            "{output}"
        );
        assert_eq!(back.len(), 1, "{output}");
        assert!(
            back.iter().all(|&(row, col)| col == a_right + 1 && a_top <= row && row <= a_bottom),
            "{output}"
        );
        assert_eq!(a_bottom - a_top, 4, "{output}");
        assert!(boxes_intact(&output, &["A", "B"]), "{output}");
    }

    #[test]
    fn mermaid_link_into_an_empty_subgraph_ends_at_its_frame() {
        assert_eq!(
            mermaid("flowchart LR\n    A --> s\n    subgraph s\n    end\n"),
            "┌───┐     ┌─ s ─┐\n│ A │────►│     │\n└───┘     └─────┘"
        );
    }

    #[test]
    fn mermaid_td_link_into_an_empty_subgraph_points_at_its_top_border() {
        let output = mermaid("flowchart TD\n    A --> s\n    subgraph s\n    end\n");
        let Some((top, left, bottom, right)) = intact_crossed_frame(&output, "s") else {
            panic!("missing or broken frame in\n{output}");
        };
        let Some(title) = title_cells(&output, top, left, "s") else {
            panic!("missing title in\n{output}");
        };
        let arrows = arrowheads(&output);

        assert_eq!(arrows.len(), 1, "{output}");
        assert!(
            arrows.iter().all(|&(row, col, c)| {
                c == '▼'
                    && row + 1 == top
                    && left < col
                    && col < right
                    && !title.contains(&col)
                    && glyph_at(&output, top, col) == Some('─')
            }),
            "{output}"
        );
        assert_eq!(bottom - top, 2, "{output}");
        assert!(boxes_intact(&output, &["A"]), "{output}");
    }

    #[test]
    fn mermaid_links_into_an_empty_subgraph_enter_on_rows_of_their_own() {
        let output = mermaid("flowchart LR\n    A --> s\n    B --> s\n    subgraph s\n    end\n");
        let Some((top, left, bottom, _)) = intact_crossed_frame(&output, "s") else {
            panic!("missing or broken frame in\n{output}");
        };
        let arrows = arrowheads(&output);

        assert_eq!(arrows.len(), 2, "{output}");
        assert!(
            arrows
                .iter()
                .all(|&(row, col, c)| c == '►' && col + 1 == left && top < row && row < bottom),
            "{output}"
        );
        assert_ne!(arrows.first().map(|arrow| arrow.0), arrows.last().map(|arrow| arrow.0));
        assert!(boxes_intact(&output, &["A", "B"]), "{output}");
    }

    const UPSTREAM_SUBGRAPH_EDGES_EXAMPLE: &str = "flowchart TB\n    c1-->a2\n    subgraph one\n    a1-->a2\n    end\n    subgraph two\n    b1-->b2\n    end\n    subgraph three\n    c1-->c2\n    end\n    one --> two\n    three --> two\n    two --> c2\n";

    #[test]
    fn mermaid_upstream_subgraph_edges_example_renders_every_link() {
        let output = mermaid(UPSTREAM_SUBGRAPH_EDGES_EXAMPLE);
        let (Some(one), Some(two), Some(three), Some((_, c2_left, c2_bottom, c2_right))) = (
            crossed_frame_of(&output, "one"),
            crossed_frame_of(&output, "two"),
            crossed_frame_of(&output, "three"),
            box_bounds(&output, "c2"),
        ) else {
            panic!("missing frame or box in\n{output}");
        };
        let arrows = arrowheads(&output);
        let up: Vec<_> = arrows.iter().filter(|&&(.., c)| c == '▲').collect();

        assert!(!output.starts_with("mermaid:"), "{output}");
        for title in ["one", "two", "three"] {
            assert_eq!(box_of(&output, title), None, "{title} in\n{output}");
        }
        assert!(boxes_intact(&output, &["a1", "a2", "b1", "b2", "c1", "c2"]), "{output}");
        assert_eq!(arrows.len(), 7, "{output}");
        assert_eq!(up.len(), 1, "{output}");
        assert!(
            up.iter()
                .all(|&&(row, col, _)| row == c2_bottom + 1 && c2_left < col && col < c2_right),
            "{output}"
        );
        assert!(two.0 > three.0 && two.0 > one.0, "{output}");
    }

    #[test]
    fn mermaid_node_cycle_reverses_the_last_declared_link() {
        let output = mermaid("flowchart LR\n    A --> B\n    C --> A\n    B --> C\n");
        let (Some(a), Some(b), Some(c)) =
            (box_of(&output, "A"), box_of(&output, "B"), box_of(&output, "C"))
        else {
            panic!("missing box in\n{output}");
        };
        let back: Vec<_> = arrowheads(&output).into_iter().filter(|&(.., g)| g == '◄').collect();

        assert!(!is_lr_chain(&output, &["A", "B", "C"]), "{output}");
        assert!(c.1 < a.1 && a.1 < b.1, "{output}");
        assert_eq!(back.len(), 1, "{output}");
        assert!(back.iter().all(|&(_, col, _)| col == c.2 + 1), "{output}");
        assert_eq!(count_glyph(&output, '►'), 2, "{output}");
        assert!(boxes_intact(&output, &["A", "B", "C"]), "{output}");
    }

    #[test]
    fn mermaid_reversed_link_out_of_a_subgraph_starts_at_the_frame_border_facing_its_target() {
        let output = mermaid(
            "flowchart TD\n    A --> s\n    subgraph s\n    B --> C\n    end\n    s --> A\n",
        );
        let frame = intact_frame(&output, "s").or_else(|| intact_crossed_frame(&output, "s"));
        let (Some((top, _, bottom, _)), Some((_, a_left, a_bottom, a_right))) =
            (frame, box_bounds(&output, "A"))
        else {
            panic!("missing frame or box in\n{output}");
        };
        let up: Vec<_> = arrowheads(&output).into_iter().filter(|&(.., g)| g == '▲').collect();
        let width = widest(&output);
        let marked = |row: usize| -> Vec<char> {
            (0..width)
                .filter_map(|col| glyph_at(&output, row, col))
                .filter(|&c| is_line_glyph(Some(c)) || "►◄▲▼".contains(c))
                .collect()
        };
        let mut above = marked(top - 1);
        above.sort_unstable();

        assert_eq!(up.len(), 1, "{output}");
        assert!(
            up.iter().all(|&(row, col, _)| row == a_bottom + 1 && a_left < col && col < a_right),
            "{output}"
        );
        assert!(top > a_bottom, "{output}");
        assert_eq!(above, vec!['│', '▼'], "{output}");
        assert!(marked(bottom + 1).iter().all(|&c| !is_line_glyph(Some(c))), "{output}");
        assert_eq!(count_glyph(&output, '▼'), 2, "{output}");
        assert!(boxes_intact(&output, &["A", "B", "C"]), "{output}");
    }

    #[test]
    fn mermaid_link_to_a_quoted_subgraph_id_attaches_to_its_frame() {
        let quoted = mermaid("flowchart LR\n    subgraph \"s\"\n    B\n    end\n    A --> s\n");
        let bare = mermaid("flowchart LR\n    subgraph s\n    B\n    end\n    A --> s\n");

        assert_eq!(quoted, bare);
        assert_eq!(box_of(&quoted, "s"), None, "{quoted}");
        assert!(frame_of(&quoted, "s").is_some(), "{quoted}");
    }

    #[test]
    fn mermaid_unclosed_edge_label_is_a_syntax_error() {
        assert_eq!(
            mermaid("flowchart LR\n    A -->|yes B\n"),
            "mermaid: line 2: unclosed edge label\n```mermaid\nflowchart LR\n    A -->|yes B\n```"
        );
    }

    #[test]
    fn mermaid_node_followed_by_another_node_is_a_syntax_error() {
        for statement in ["A B", "A .foo"] {
            let body = format!("flowchart LR\n    {statement}\n");
            assert_eq!(
                mermaid(&body),
                format!("mermaid: line 2: expected a link\n```mermaid\n{body}```"),
                "{statement}"
            );
        }
    }

    #[test]
    fn mermaid_unclosed_link_is_a_syntax_error() {
        for link in ["A -- text", "A --B", "A == text", "A -. text"] {
            let body = format!("flowchart LR\n    {link}\n");
            assert_eq!(
                mermaid(&body),
                format!("mermaid: line 2: unclosed link\n```mermaid\n{body}```"),
                "{link}"
            );
        }
    }

    #[test]
    fn mermaid_edge_id_before_a_link_renders_the_link() {
        assert_eq!(mermaid("flowchart LR\n    A e1@--> B\n"), A_TO_B);
        assert_eq!(mermaid("flowchart LR\n    A e1@-- yes --> B\n"), A_YES_B);
        assert_eq!(
            mermaid("flowchart LR\n    A e1@==> B\n"),
            mermaid("flowchart LR\n    A ==> B\n")
        );
    }

    #[test]
    fn mermaid_at_sign_inside_an_edge_label_is_label_text() {
        let output = mermaid("flowchart LR\n    A -->|a@b| B\n");

        assert_eq!(mermaid("flowchart LR\n    A e1@-->|a@b| B\n"), output);
        // `whole_words` splits at `@`, so the label is counted as a substring.
        assert_eq!(output.matches("a@b").count(), 1, "{output}");
        assert_eq!(count_glyph(&output, '►'), 1, "{output}");
        assert!(boxes_intact(&output, &["A", "B"]), "{output}");
    }

    #[test]
    fn mermaid_edge_data_statement_applies_to_the_edge_not_a_node() {
        assert_eq!(mermaid("flowchart LR\n    A e1@--> B\n    e1@{ animate: true }\n"), A_TO_B);
        assert_eq!(
            mermaid("flowchart LR\n    A e1@--> B\n    e1@{ animation: fast, curve: linear }\n"),
            A_TO_B
        );
        // An id no link declared is a node with shape data, as before edge ids existed.
        assert_eq!(mermaid("flowchart LR\n    e1@{ shape: stadium }\n"), "╭────╮\n( e1 )\n╰────╯");
    }

    #[test]
    fn mermaid_invisible_link_places_the_target_in_the_next_layer_and_draws_nothing() {
        assert_eq!(
            mermaid("flowchart LR\n    A ~~~ B\n"),
            "┌───┐     ┌───┐\n│ A │     │ B │\n└───┘     └───┘"
        );
    }

    #[test]
    fn mermaid_td_invisible_link_leaves_blank_rows_between_the_boxes() {
        assert_eq!(
            mermaid("flowchart TD\n    A ~~~ B\n"),
            "┌───┐\n│ A │\n└───┘\n\n\n\n┌───┐\n│ B │\n└───┘"
        );
    }

    #[test]
    fn mermaid_invisible_link_chains_with_visible_links() {
        let output = mermaid("flowchart LR\n    A ~~~ B --> C\n");
        let (Some(a), Some(b), Some(c)) =
            (box_of(&output, "A"), box_of(&output, "B"), box_of(&output, "C"))
        else {
            panic!("missing box in\n{output}");
        };

        assert!(a.0 == b.0 && b.0 == c.0, "{output}");
        assert!(a.2 < b.1 && b.1 < c.1, "{output}");
        assert_eq!(count_glyph(&output, '►'), 1, "{output}");
        assert_eq!(glyph_at(&output, c.0, c.1 - 1), Some('►'), "{output}");
        assert!((a.2 + 1..b.1).all(|col| !is_line_glyph(glyph_at(&output, a.0, col))), "{output}");
        assert!(boxes_intact(&output, &["A", "B", "C"]), "{output}");
    }

    #[test]
    fn mermaid_extra_tildes_lengthen_an_invisible_link() {
        let output = mermaid("flowchart LR\n    A ~~~~ B\n    A --> C\n");
        let (Some((_, _, _, c_right)), Some((_, b_left, _, _))) =
            (box_bounds(&output, "C"), box_bounds(&output, "B"))
        else {
            panic!("missing box in\n{output}");
        };

        assert!(b_left > c_right, "{output}");
        assert_eq!(count_glyph(&output, '►'), 1, "{output}");
        assert!(boxes_intact(&output, &["A", "B", "C"]), "{output}");
    }

    #[test]
    fn mermaid_invisible_link_claims_no_port_cell_and_grows_no_box() {
        for (direction, expected) in [("TD", A_ABOVE_B), ("LR", A_TO_B)] {
            for links in ["A --> B\n    A ~~~ B", "A ~~~ B\n    A --> B"] {
                let body = format!("flowchart {direction}\n    {links}\n");
                assert_eq!(mermaid(&body), expected, "{body}");
            }
        }
    }

    #[test]
    fn mermaid_label_on_an_invisible_link_falls_back_without_a_reason_line() {
        let body = "flowchart LR\n    A ~~~|x| B\n";
        assert_eq!(mermaid(body), format!("```mermaid\n{body}```"));
    }

    #[test]
    fn mermaid_invisible_sibling_does_not_pull_its_parent_off_the_visible_link() {
        for (body, b_first) in [
            ("flowchart TD\n    A ~~~ B\n    A --> C\n", true),
            ("flowchart TD\n    A --> C\n    A ~~~ B\n", false),
        ] {
            let output = mermaid(body);
            let (
                Some((_, a_left, a_bottom, _)),
                Some((b_top, b_left, _, b_right)),
                Some((_, c_left, _, c_right)),
            ) = (box_bounds(&output, "A"), box_bounds(&output, "B"), box_bounds(&output, "C"))
            else {
                panic!("missing box in\n{output}");
            };
            // Box borders are drawn with `─` too, so only the rows between the layers count.
            let turns = output
                .lines()
                .enumerate()
                .any(|(row, line)| a_bottom < row && row < b_top && line.contains('─'));

            assert_eq!(a_left, c_left, "{output}");
            assert_eq!(count_glyph(&output, '▼'), 1, "{output}");
            assert!(!turns, "{output}");
            assert!(boxes_intact(&output, &["A", "B", "C"]), "{output}");
            if b_first {
                assert!(b_right < c_left, "{output}");
            } else {
                assert!(c_right < b_left, "{output}");
            }
        }
    }

    #[test]
    fn mermaid_invisible_sibling_in_a_subgraph_keeps_the_visible_link_straight() {
        for member in ["B", "C"] {
            let body = format!(
                "flowchart TD\n    A ~~~ B\n    A --> C\n    subgraph S\n      {member}\n    end\n"
            );
            let output = mermaid(&body);
            let (Some((_, a_left, a_bottom, _)), Some((c_top, c_left, ..))) =
                (box_bounds(&output, "A"), box_bounds(&output, "C"))
            else {
                panic!("missing box in\n{output}");
            };
            let Some(&(_, head, _)) = arrowheads(&output).first() else {
                panic!("missing arrowhead in\n{output}");
            };

            assert_eq!(a_left, c_left, "{output}");
            assert_eq!(count_glyph(&output, '▼'), 1, "{output}");
            assert!(
                (a_bottom + 1..c_top)
                    .all(|row| glyph_at(&output, row, head).is_some_and(|c| "│┼▼".contains(c))),
                "{output}"
            );
            assert!(boxes_intact(&output, &["A", "B", "C"]), "{output}");
        }
    }

    #[test]
    fn mermaid_invisible_self_loop_takes_no_cells() {
        assert_eq!(
            mermaid("flowchart LR\n    A ~~~ A\n    A --> B\n    C --> D\n"),
            mermaid("flowchart LR\n    A --> B\n    C --> D\n")
        );
        assert_eq!(mermaid("flowchart TD\n    A ~~~ A\n    A --> B\n"), mermaid(TD_A_TO_B));
    }

    #[test]
    fn mermaid_invisible_link_takes_no_track() {
        let output = mermaid("flowchart TD\n    A ~~~~ B\n    A --> C\n    C --> B\n");
        let (Some((_, a_left, a_bottom, _)), Some((c_top, c_left, c_bottom, _)), Some((b_top, ..))) =
            (box_bounds(&output, "A"), box_bounds(&output, "C"), box_bounds(&output, "B"))
        else {
            panic!("missing box in\n{output}");
        };

        // The invisible link takes no exit cell, so the link into C leaves A's centre.
        assert_eq!(a_left, c_left, "{output}");
        assert_eq!(b_top - c_bottom, c_top - a_bottom, "{output}");
        assert_eq!(count_glyph(&output, '▼'), 2, "{output}");
        assert!(boxes_intact(&output, &["A", "B", "C"]), "{output}");
    }

    #[test]
    fn mermaid_direction_symbols_render_like_their_names() {
        for (symbol, name) in [(">", "LR"), ("<", "RL"), ("^", "BT"), ("v", "TD")] {
            assert_eq!(
                mermaid(&format!("graph {symbol}\n    A --> B\n")),
                mermaid(&format!("flowchart {name}\n    A --> B\n")),
                "{symbol}"
            );
        }
    }

    #[test]
    fn mermaid_tabs_in_labels_render_as_one_space() {
        assert_eq!(mermaid("flowchart LR\n    A[a\tb]\n"), mermaid("flowchart LR\n    A[a b]\n"));
        assert_eq!(
            mermaid("flowchart LR\n    A -->|a\tb| B\n"),
            mermaid("flowchart LR\n    A -->|a b| B\n")
        );
    }

    /// The plain drawing of a lone rectangle whose label is `text` written in quotes.
    fn quoted_box(text: &str) -> String {
        mermaid(&format!("flowchart LR\n    A[\"{text}\"]\n"))
    }

    /// A lone rectangle around `label`, which is one display cell per character.
    fn box_around(label: &str) -> String {
        let border = "─".repeat(label.chars().count() + 2);
        format!("┌{border}┐\n│ {label} │\n└{border}┘")
    }

    #[test]
    fn mermaid_decimal_entity_code_decodes_in_a_node_label() {
        assert_eq!(
            mermaid("flowchart LR\n    A[\"A dec char:#9829;\"]\n"),
            "┌──────────────┐\n│ A dec char:♥ │\n└──────────────┘"
        );
    }

    #[test]
    fn mermaid_named_entity_codes_decode_in_a_node_label() {
        let output = mermaid("flowchart LR\n    A[\"#quot;x#quot; #amp; #lt;y#gt;\"]\n");

        assert!(output.lines().any(|line| line == "│ \"x\" & <y> │"), "{output}");
    }

    #[test]
    fn mermaid_entity_codes_decode_in_edge_labels_and_subgraph_titles() {
        let edge = mermaid("flowchart LR\n    A -->|#quot;yes#quot;| B\n");
        assert_eq!(edge, "┌───┐        ┌───┐\n│ A │─\"yes\"─►│ B │\n└───┘        └───┘");

        let title = mermaid("flowchart LR\n    subgraph s [a #amp; b]\n    A\n    end\n");
        assert!(intact_frame(&title, "a & b").is_some(), "{title}");
    }

    #[test]
    fn mermaid_control_characters_from_entity_codes_render_as_control_pictures() {
        let escape = mermaid("flowchart LR\n    A[\"x#27;y\"]\n");
        assert!(escape.lines().any(|line| line == "│ x␛y │"), "{escape}");
        assert!(!escape.contains('\u{1b}'), "{escape}");

        // 0x9D is one of the C1 codes HTML's table leaves as they are.
        let c1 = mermaid("flowchart LR\n    A[\"x#157;y\"]\n");
        assert!(c1.lines().any(|line| line == "│ x�y │"), "{c1}");

        let tab = mermaid("flowchart LR\n    A[\"x#9;y\"]\n");
        assert!(tab.lines().any(|line| line == "│ x y │"), "{tab}");
    }

    #[test]
    fn mermaid_entity_codes_decode_as_html_and_leave_partial_codes_as_written() {
        for (written, shown) in [
            ("#35;1", "#1"),
            ("#nosuch;", "#nosuch;"),
            ("#0;", "�"),
            ("#1114112;", "�"),
            ("#55296;", "�"),
            ("#x26;", "#x26;"),
            ("#;", "#;"),
            ("#amp", "#amp"),
            ("a#", "a#"),
            ("##35;", "##"),
            ("#fjlig;", "fj"),
        ] {
            assert_eq!(quoted_box(written), box_around(shown), "{written}");
        }
    }

    #[test]
    fn mermaid_decimal_entity_codes_128_to_159_decode_as_html_does() {
        assert_eq!(quoted_box("x#128;y"), box_around("x€y"));
        assert_eq!(quoted_box("x#159;y"), box_around("xŸy"));
    }

    #[test]
    fn mermaid_br_in_a_node_label_makes_a_second_row() {
        assert_eq!(
            mermaid("flowchart LR\n    A[\"Line 1<br>Line 2\"]\n"),
            "┌────────┐\n│ Line 1 │\n│ Line 2 │\n└────────┘"
        );
    }

    #[test]
    fn mermaid_br_spellings_all_break_the_line() {
        for br in ["<br/>", "<br />", "<BR>", "</br>"] {
            assert_eq!(
                mermaid(&format!("flowchart LR\n    A[\"a{br}b\"]\n")),
                "┌───┐\n│ a │\n│ b │\n└───┘",
                "{br}"
            );
        }
    }

    #[test]
    fn mermaid_shorter_label_rows_are_centred() {
        assert_eq!(
            mermaid("flowchart LR\n    A[\"Longer row<br>ab\"]\n"),
            "┌────────────┐\n│ Longer row │\n│     ab     │\n└────────────┘"
        );
        assert_eq!(
            mermaid("flowchart LR\n    A[\"abc<br>ab\"]\n"),
            "┌─────┐\n│ abc │\n│ ab  │\n└─────┘"
        );
    }

    #[test]
    fn mermaid_lr_link_into_a_two_row_label_enters_on_the_upper_middle_row() {
        assert_eq!(
            mermaid("flowchart LR\n    A --> B[\"x<br>y\"]\n"),
            "┌───┐     ┌───┐\n│ A │────►│ x │\n└───┘     │ y │\n          └───┘"
        );
    }

    #[test]
    fn mermaid_td_link_into_a_two_row_label_points_at_its_top_border() {
        assert_eq!(
            mermaid("flowchart TD\n    A --> B[\"x<br>y\"]\n"),
            "┌───┐\n│ A │\n└───┘\n  │\n  │\n  ▼\n┌───┐\n│ x │\n│ y │\n└───┘"
        );
    }

    #[test]
    fn mermaid_br_rows_follow_how_html_breaks_lines() {
        // A last `<br>` ends the line before it without starting another.
        assert_eq!(quoted_box("a<br>"), box_around("a"));
        assert_eq!(quoted_box("<br>"), "┌──┐\n│  │\n└──┘");
        assert_eq!(quoted_box("a<br><br>"), "┌───┐\n│ a │\n│   │\n└───┘");
        assert_eq!(quoted_box("a<br><br>b"), "┌───┐\n│ a │\n│   │\n│ b │\n└───┘");
        assert_eq!(quoted_box("<br>b"), "┌───┐\n│   │\n│ b │\n└───┘");
        assert_eq!(quoted_box("a<brx>"), box_around("a<brx>"));
        assert_eq!(quoted_box("a<b r>"), box_around("a<b r>"));
    }

    #[test]
    fn mermaid_two_row_labels_repeat_the_label_row_of_each_shape() {
        assert_eq!(mermaid("flowchart LR\n    A{\"x<br>y\"}\n"), "╱───╲\n│ x │\n│ y │\n╲───╱");
        assert_eq!(mermaid("flowchart LR\n    A((\"x<br>y\"))\n"), " ╭─╮\n( x )\n( y )\n ╰─╯");
    }

    #[test]
    fn mermaid_br_in_a_shape_data_label_makes_rows_too() {
        assert_eq!(
            mermaid("flowchart LR\n    A@{ shape: stadium, label: \"x<br>y\" }\n"),
            "╭───╮\n( x )\n( y )\n╰───╯"
        );
    }

    #[test]
    fn mermaid_br_in_an_lr_edge_label_stacks_its_rows_on_the_line() {
        assert_eq!(
            mermaid("flowchart LR\n    A -->|yes<br>no| B\n"),
            "┌───┐      ┌───┐\n│ A │─yes─►│ B │\n└───┘ no   └───┘"
        );
    }

    #[test]
    fn mermaid_td_edge_label_rows_are_each_centred_on_the_line() {
        assert_eq!(
            mermaid("flowchart TD\n    A -->|x<br>yy| B\n"),
            "┌───┐\n│ A │\n└───┘\n  │\n  x\n yy\n  ▼\n┌───┐\n│ B │\n└───┘"
        );
    }

    #[test]
    fn mermaid_td_passing_slot_label_as_tall_as_its_layer_fills_it() {
        assert_eq!(
            mermaid("flowchart TD\n    A --> B --> C\n    A -->|a<br>b<br>c<br>d| C\n"),
            concat!(
                " ┌───┐\n │ A │\n └───┘\n  │ │\n  │ └───┐\n  ▼     │\n",
                "┌───┐   a\n│ B │   b\n└───┘   c\n  │     d\n",
                "  │     │\n  └─┐   │\n    │ ┌─┘\n    ▼ ▼\n   ┌───┐\n   │ C │\n   └───┘"
            )
        );
    }

    #[test]
    fn mermaid_even_rows_in_a_td_passing_slot_put_the_upper_middle_row_on_the_layer_middle() {
        assert_eq!(
            mermaid("flowchart TD\n    A --> B --> C\n    A -->|x<br>y| C\n"),
            concat!(
                " ┌───┐\n │ A │\n └───┘\n  │ │\n  │ └───┐\n  ▼     │\n",
                "┌───┐   │\n│ B │   x\n└───┘   y\n",
                "  │     │\n  └─┐   │\n    │ ┌─┘\n    ▼ ▼\n   ┌───┐\n   │ C │\n   └───┘"
            )
        );
    }

    #[test]
    fn mermaid_br_in_a_subgraph_title_joins_the_rows_with_a_space() {
        assert_eq!(
            mermaid("flowchart LR\n    subgraph s [Data<br>Layer]\n    A\n    end\n"),
            mermaid("flowchart LR\n    subgraph s [Data Layer]\n    A\n    end\n")
        );
    }

    #[test]
    fn mermaid_quoted_label_spanning_source_lines_joins_them_with_a_space() {
        assert_eq!(
            mermaid("flowchart LR\n    A[\"first\n    second\"] --> B\n"),
            mermaid("flowchart LR\n    A[\"first second\"] --> B\n")
        );
    }

    #[test]
    fn mermaid_unclosed_quote_at_the_end_of_the_diagram_is_a_syntax_error() {
        let body = "flowchart LR\n    A[\"first\n    B\n";

        assert_eq!(
            mermaid(body),
            format!("mermaid: line 2: unclosed node label\n```mermaid\n{body}```")
        );
    }

    /// The first span of `lines` whose text contains `text`.
    fn span_containing<'a>(lines: &'a [Line], text: &str) -> Option<&'a Span> {
        lines.iter().flatten().find(|span| span.text.contains(text))
    }

    #[test]
    fn mermaid_markdown_string_bold_is_drawn_bold_without_the_markers() {
        let body = "flowchart LR\n    A[\"`The **cat** sat`\"]\n";
        let output = mermaid(body);
        assert!(output.lines().any(|line| line == "│ The cat sat │"), "{output}");

        let lines = lay_out(&format!("```mermaid\n{body}```\n"), None);
        let cat = lines.iter().flatten().find(|span| span.text == "cat");
        assert!(cat.is_some_and(|cat| cat.style.bold), "{lines:?}");
        assert!(cat.is_some_and(|cat| cat.style.fg == Some(DARK_PALETTE.body)), "{lines:?}");
        for plain in ["The ", " sat"] {
            let span = span_containing(&lines, plain);
            assert!(span.is_some_and(|span| !span.style.bold), "{plain}: {lines:?}");
        }
    }

    #[test]
    fn mermaid_markdown_string_italic_uses_asterisk_or_underscore() {
        let body = "flowchart LR\n    A[\"`This *is* _Markdown_`\"]\n";
        let output = mermaid(body);
        assert!(output.lines().any(|line| line == "│ This is Markdown │"), "{output}");

        let lines = lay_out(&format!("```mermaid\n{body}```\n"), None);
        for italic in ["is", "Markdown"] {
            let span = lines.iter().flatten().find(|span| span.text == italic);
            assert!(span.is_some_and(|span| span.style.italic), "{italic}: {lines:?}");
        }
    }

    #[test]
    fn mermaid_markdown_string_newlines_make_rows() {
        assert_eq!(
            mermaid("flowchart LR\n    A[\"`Line1\n    Line 2\n    Line 3`\"]\n"),
            "┌────────┐\n│ Line1  │\n│ Line 2 │\n│ Line 3 │\n└────────┘"
        );
    }

    #[test]
    fn mermaid_asterisks_outside_markdown_strings_are_plain_text() {
        let body = "flowchart LR\n    A[\"**not bold**\"]\n";
        let output = mermaid(body);
        assert!(output.lines().any(|line| line == "│ **not bold** │"), "{output}");

        let lines = lay_out(&format!("```mermaid\n{body}```\n"), None);
        assert!(lines.iter().flatten().all(|span| !span.style.bold), "{lines:?}");
    }

    #[test]
    fn mermaid_markdown_edge_label_and_subgraph_title_render_bold() {
        let body = "flowchart LR\n    subgraph \"`**Two**`\"\n    c(\"`The **cat**\n    in the hat`\") -- \"`Bold **edge label**`\" --> d(\"The dog in the hog\")\n    end\n";
        let output = mermaid(body);
        assert!(!output.starts_with("mermaid:"), "{output}");
        assert!(
            intact_frame(&output, "Two").or_else(|| intact_crossed_frame(&output, "Two")).is_some(),
            "{output}"
        );
        assert!(output.lines().any(|line| line.contains("│  The cat   │")), "{output}");
        assert!(output.lines().any(|line| line.contains("│ in the hat │")), "{output}");
        assert_eq!(output.matches("Bold edge label").count(), 1, "{output}");
        assert!(boxes_intact(&output, &["The dog in the hog"]), "{output}");

        let lines = lay_out(&format!("```mermaid\n{body}```\n"), None);
        for bold in ["Two", "edge label"] {
            let span = lines.iter().flatten().find(|span| span.text == bold);
            assert!(span.is_some_and(|span| span.style.bold), "{bold}: {lines:?}");
        }
    }

    /// The laid-out lines of a lone rectangle whose label is the markdown string `text`.
    fn markdown_box_lines(text: &str) -> Vec<Line> {
        lay_out(&format!("```mermaid\nflowchart LR\n    A[\"`{text}`\"]\n```\n"), None)
    }

    #[test]
    fn mermaid_markdown_string_double_underscores_are_bold() {
        assert_eq!(quoted_box("`__b__`"), box_around("b"));
        let lines = markdown_box_lines("__b__");
        let b = lines.iter().flatten().find(|span| span.text == "b");
        assert!(b.is_some_and(|b| b.style.bold && !b.style.italic), "{lines:?}");
    }

    #[test]
    fn mermaid_markdown_string_emphasis_nests() {
        assert_eq!(quoted_box("`*a **b** c*`"), box_around("a b c"));
        let lines = markdown_box_lines("*a **b** c*");
        let style_of = |text: &str| span_containing(&lines, text).map(|span| span.style);
        assert!(style_of("a").is_some_and(|style| style.italic && !style.bold), "{lines:?}");
        assert!(style_of("b").is_some_and(|style| style.italic && style.bold), "{lines:?}");
        assert!(style_of("c").is_some_and(|style| style.italic && !style.bold), "{lines:?}");
    }

    #[test]
    fn mermaid_markdown_string_unpaired_markers_and_inner_underscores_stay_text() {
        for (written, shown) in [("*x", "*x"), ("snake_case_name", "snake_case_name")] {
            assert_eq!(quoted_box(&format!("`{written}`")), box_around(shown), "{written}");
            let lines = markdown_box_lines(written);
            assert!(
                lines.iter().flatten().all(|span| !span.style.italic && !span.style.bold),
                "{written}: {lines:?}"
            );
        }
    }

    #[test]
    fn mermaid_markdown_string_drops_blank_lines_and_the_blanks_around_lines() {
        assert_eq!(quoted_box("``"), "┌──┐\n│  │\n└──┘");
        assert_eq!(quoted_box("`  a  \n\n    b  `"), "┌───┐\n│ a │\n│ b │\n└───┘");
    }

    #[test]
    fn mermaid_markdown_string_unclosed_backtick_is_plain_text() {
        let output = mermaid("flowchart LR\n    A[\"`not markdown\"]\n");

        assert!(output.lines().any(|line| line == "│ `not markdown │"), "{output}");
    }

    #[test]
    fn mermaid_td_frame_title_moves_right_of_a_link_crossing_the_top_border() {
        let output =
            mermaid("flowchart TD\n    A --> B\n    subgraph s [Long Title]\n    B\n    end\n");
        let (Some((top, ..)), Some(b)) =
            (crossed_frame_of(&output, "Long Title"), box_of(&output, "B"))
        else {
            panic!("missing frame or box in\n{output}");
        };
        let centre = (b.1 + b.2) / 2;

        assert_eq!(whole_words(&output, "Long"), 1, "{output}");
        assert_eq!(whole_words(&output, "Title"), 1, "{output}");
        assert_eq!(count_glyph(&output, '▼'), 1, "{output}");
        assert!(boxes_intact(&output, &["A", "B"]), "{output}");
        assert_eq!(glyph_at(&output, top, centre), Some('┼'), "{output}");
    }

    #[test]
    fn mermaid_td_frame_title_clears_links_into_a_member_beside_an_outside_sibling() {
        for (body, labels) in [
            // The basic case: one link from outside crosses the top border.
            (
                "flowchart TD\n    A --> B\n    A --> C\n    subgraph s [Ti]\n    B\n    end\n",
                &["A", "B", "C"][..],
            ),
            // A wide source box: the frame no longer starts at the first column, so the
            // crossings are counted from its corner rather than from column 0.
            (
                "flowchart TD\n    A[Source node] --> B\n    A --> C\n    subgraph s [Ti]\n    B\n    end\n",
                &["Source node", "B", "C"][..],
            ),
            // Three children: the frame closes before two outside siblings, not one.
            (
                "flowchart TD\n    A --> B\n    A --> C\n    A --> D\n    subgraph s [Ti]\n    B\n    end\n",
                &["A", "B", "C", "D"][..],
            ),
        ] {
            let output = mermaid(body);
            let (Some((top, left, bottom, right)), Some((_, c_left, _))) =
                (intact_crossed_frame(&output, "Ti"), box_of(&output, "C"))
            else {
                panic!("missing frame or box in\n{output}");
            };
            let Some(title) = title_cells(&output, top, left, "Ti") else {
                panic!("missing title in\n{output}");
            };
            let crossings_before_title =
                (left + 1..title.start - 1).filter(|&col| glyph_at(&output, top, col) == Some('┼'));
            // Above the frame, the link to C runs across these columns.
            let blank = |col: usize| {
                (top..=bottom).all(|row| glyph_at(&output, row, col).is_none_or(|c| c == ' '))
            };

            assert!(boxes_intact(&output, labels), "{output}");
            assert!(crossings_before_title.count() > 0, "{output}");
            assert!(right + 1 < c_left, "{output}");
            assert!((right + 1..c_left).all(blank), "{output}");
        }
    }

    #[test]
    fn mermaid_bt_frame_title_leaves_the_sibling_gap_before_an_outside_sibling() {
        // In BT the top border faces the next layer, so the crossings the title clears are
        // estimated from links leaving the member, not entering it; none leave B, so the
        // title does not widen the frame, which leaves the gap B has without a subgraph.
        let output = mermaid(
            "flowchart BT\n    A --> B\n    A --> C\n    subgraph s [Ti]\n    B\n    end\n",
        );
        let unframed = mermaid("flowchart BT\n    A --> B\n    A --> C\n");
        let (Some((_, _, _, right)), Some((_, c_left, _))) =
            (intact_crossed_frame(&output, "Ti"), box_of(&output, "C"))
        else {
            panic!("missing frame or box in\n{output}");
        };
        let (Some((_, _, b_right)), Some((_, unframed_c_left, _))) =
            (box_of(&unframed, "B"), box_of(&unframed, "C"))
        else {
            panic!("missing box in\n{unframed}");
        };

        assert!(boxes_intact(&output, &["A", "B", "C"]), "{output}");
        assert_eq!(c_left - right, unframed_c_left - b_right, "{output}\n{unframed}");
    }

    #[test]
    fn mermaid_tab_in_a_subgraph_title_renders_as_one_space() {
        let output = mermaid("flowchart LR\n    subgraph s [Data\tLayer]\n    A\n    end\n");

        assert!(intact_frame(&output, "Data Layer").is_some(), "{output}");
        assert!(!output.contains('\t'), "{output}");
    }

    #[test]
    fn mermaid_listing_a_subgraph_id_inside_another_nests_it() {
        let nested =
            mermaid("flowchart LR\n    subgraph a\n    subgraph b\n    X\n    end\n    end\n");
        for body in [
            "flowchart LR\n    subgraph a\n    b\n    end\n    subgraph b\n    X\n    end\n",
            "flowchart LR\n    subgraph b\n    X\n    end\n    subgraph a\n    b\n    end\n",
        ] {
            let output = mermaid(body);

            assert_eq!(output, nested, "{body}");
            assert_eq!(box_of(&output, "b"), None, "{output}");
        }
    }

    #[test]
    fn mermaid_long_links_needing_too_many_passing_slots_fall_back_without_a_reason_line() {
        // 400 links each passing 28 layers need 11 200 slots, over MAX_PASSING_SLOTS.
        let mut body = String::from("flowchart LR\n");
        for node in 0..29 {
            body.push_str(&format!("    N{node} --> N{}\n", node + 1));
        }
        body.push_str(&"    N0 --> N29\n".repeat(400));

        assert_eq!(mermaid(&body), format!("```mermaid\n{body}```"));
    }
}
