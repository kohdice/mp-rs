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
    const A_YES_B: &str = "┌───┐ yes ┌───┐\n│ A │────►│ B │\n└───┘     └───┘";
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

    /// The row of the box whose label row is `│ label │`, and the display columns of
    /// its left and right borders.
    fn box_of(output: &str, label: &str) -> Option<(usize, usize, usize)> {
        let label_row = format!("│ {label} │");
        output.lines().enumerate().find_map(|(row, line)| {
            let left = line.get(..line.find(&label_row)?)?.width();
            Some((row, left, left + label_row.width() - 1))
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

    /// Whether each label appears exactly once, inside a box whose border rows are made
    /// of corner glyphs and `─` over the box's columns.
    fn boxes_intact(output: &str, labels: &[&str]) -> bool {
        labels.iter().all(|label| {
            let Some((row, left, right)) = box_of(output, label) else { return false };
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
                && row > 0
                && border(row - 1, ["┌╭╱", "┐╮╲"])
                && border(row + 1, ["└╰╲", "┘╯╱"])
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
    fn mermaid_unsupported_node_shapes_fall_back_without_a_reason_line() {
        for node in ["A([x])", "A[(x)]", "A[/x/]", "A@{ shape: rect }"] {
            let body = format!("flowchart LR\n    {node}\n");
            assert_eq!(mermaid(&body), format!("```mermaid\n{body}```"), "{node}");
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
    fn mermaid_lr_edge_label_sits_above_the_line() {
        assert_eq!(mermaid("flowchart LR\n    A -->|yes| B\n"), A_YES_B);
    }

    #[test]
    fn mermaid_lr_edge_label_widens_the_gap_when_it_is_longer() {
        assert_eq!(
            mermaid("flowchart LR\n    A -->|approved| B\n"),
            "┌───┐ approved ┌───┐\n│ A │─────────►│ B │\n└───┘          └───┘"
        );
    }

    #[test]
    fn mermaid_text_label_form_is_equivalent_to_pipes_for_every_stroke() {
        for (text_form, pipe_form) in [
            ("A -- yes --> B", "A -->|yes| B"),
            ("A -. yes .-> B", "A -.->|yes| B"),
            ("A == yes ==> B", "A ==>|yes| B"),
        ] {
            let pipes = mermaid(&format!("flowchart LR\n    {pipe_form}\n"));

            assert_eq!(pipes.lines().next(), Some("┌───┐ yes ┌───┐"), "{pipe_form}");
            assert_eq!(mermaid(&format!("flowchart LR\n    {text_form}\n")), pipes, "{text_form}");
        }
    }

    #[test]
    fn mermaid_td_edge_label_sits_beside_the_line() {
        assert_eq!(
            mermaid("flowchart TD\n    A -->|yes| B\n"),
            "┌───┐\n│ A │\n└───┘\n  │\n  │ yes\n  ▼\n┌───┐\n│ B │\n└───┘"
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
        assert!(
            (c.0 - 1..=c.0 + 1).any(|row| glyph_at(&output, row, c.1 - 1) == Some('►')),
            "{output}"
        );
        assert!(boxes_intact(&output, &["A", "B", "C"]), "{output}");
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
    fn mermaid_self_loop_renders_one_arrowhead_outside_the_box() {
        let output = mermaid("flowchart LR\n    A --> A\n");
        let Some((row, left, right)) = box_of(&output, "A") else {
            panic!("missing box in\n{output}");
        };
        let line_outside_box = output.lines().enumerate().any(|(line_row, line)| {
            let mut col = 0;
            line.chars().any(|c| {
                let outside =
                    !(row - 1..=row + 1).contains(&line_row) || !(left..=right).contains(&col);
                col += c.width().unwrap_or(0);
                outside && is_line_glyph(Some(c))
            })
        });

        assert!(boxes_intact(&output, &["A"]), "{output}");
        assert_eq!(output.matches("│ A │").count(), 1, "{output}");
        assert_eq!(arrowheads(&output).len(), 1, "{output}");
        assert!(line_outside_box, "{output}");
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
            let Some((row, col)) = position_of_word(&output, label) else {
                panic!("missing {label} in\n{output}");
            };
            assert_eq!(glyph_at(&output, row, col - 1), Some(' '), "{label} in\n{output}");
            assert!(
                matches!(glyph_at(&output, row, col + label.width()), Some(' ') | None),
                "{label} in\n{output}"
            );
        }
        assert_eq!(count_glyph(&output, '►'), 2, "{output}");
    }

    #[test]
    fn mermaid_td_labels_of_links_into_one_box_stay_separate() {
        let output = mermaid("flowchart TD\n    A -->|yes| B\n    B -->|no| A\n");

        assert!(boxes_intact(&output, &["A", "B"]), "{output}");
        assert_eq!(arrowheads(&output).len(), 2, "{output}");
        assert_eq!(whole_words(&output, "yes"), 1, "{output}");
        assert_eq!(whole_words(&output, "no"), 1, "{output}");
    }

    #[test]
    fn mermaid_label_of_a_back_edge_spanning_several_layers_has_room_of_its_own() {
        for direction in ["LR", "RL"] {
            let output = mermaid(&format!(
                "flowchart {direction}\n    A --> B --> C\n    C -->|back again| A\n"
            ));
            let Some((row, col)) = position_of_word(&output, "back") else {
                panic!("missing label in\n{output}");
            };
            let after = col + "back again".width();
            let unlabelled = mermaid(&format!("flowchart {direction}\n    A --> B --> C\n"));
            // The columns from one box's border to the other's, whichever is on the left.
            let b_to_c = |output: &str| {
                let (Some((_, b_left, b_right)), Some((_, c_left, c_right))) =
                    (box_of(output, "B"), box_of(output, "C"))
                else {
                    panic!("missing box in\n{output}");
                };
                if b_right < c_left { c_left - b_right } else { b_left - c_right }
            };

            assert_eq!(b_to_c(&output), b_to_c(&unlabelled), "{output}\n{unlabelled}");
            assert_eq!(output.matches("back again").count(), 1, "{output}");
            assert_eq!(glyph_at(&output, row, col - 1), Some(' '), "{output}");
            assert!(matches!(glyph_at(&output, row, after), Some(' ') | None), "{output}");
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
    fn mermaid_self_loop_label_is_shown() {
        let output = mermaid("flowchart LR\n    A -->|again| A\n");

        assert_eq!(whole_words(&output, "again"), 1, "{output}");
        assert!(boxes_intact(&output, &["A"]), "{output}");
    }

    #[test]
    fn mermaid_self_loop_and_a_turning_link_from_the_same_box_run_down_separate_columns() {
        let output = mermaid("flowchart LR\n    A --> A\n    A --> B\n    A --> C\n");
        let [Some(a), Some(c)] = ["A", "C"].map(|label| box_of(&output, label)) else {
            panic!("missing box in\n{output}");
        };
        let Some(loop_col) = (a.0 + 1..output.lines().count()).find_map(|row| {
            (a.2 + 1..)
                .take_while(|&col| glyph_at(&output, row, col).is_some())
                .find(|&col| glyph_at(&output, row, col) == Some('│'))
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
        assert_eq!(glyph_at(&output, c.0, turn_col), Some('└'), "{output}");
        assert_ne!(turn_col, loop_col, "{output}");
        assert!(boxes_intact(&output, &["A", "B", "C"]), "{output}");
    }

    #[test]
    fn mermaid_self_loop_label_wider_than_its_box_is_shown() {
        // There is no place for the label clear of the box and the loop, and it is shown
        // anyway.
        let output = mermaid("flowchart LR\n    A -->|retry on failure| A\n");

        assert!(output.contains("retry on failure"), "{output}");
        assert!(boxes_intact(&output, &["A"]), "{output}");
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
        let (Some((top, ..)), Some((label_row, _))) =
            (crossed_frame_of(&output, "s"), position_of_word(&output, "yes"))
        else {
            panic!("missing frame or label in\n{output}");
        };

        assert_eq!(whole_words(&output, "yes"), 1, "{output}");
        assert_ne!(label_row, top, "{output}");
        assert!(boxes_intact(&output, &["A", "B"]), "{output}");
        assert!(intact_crossed_frame(&output, "s").is_some(), "{output}");
    }

    #[test]
    fn mermaid_lr_edge_label_into_a_subgraph_keeps_a_blank_column_before_its_frame() {
        let output = mermaid("flowchart LR\n    A -->|yes| B\n    subgraph s\n    B\n    end\n");
        let (Some((_, left, ..)), Some((row, col))) =
            (frame_of(&output, "s"), position_of_word(&output, "yes"))
        else {
            panic!("missing frame or label in\n{output}");
        };
        let after = col + "yes".width();

        assert_eq!(whole_words(&output, "yes"), 1, "{output}");
        assert!(after < left, "{output}");
        assert_eq!(glyph_at(&output, row, after), Some(' '), "{output}");
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
    fn mermaid_non_member_between_subgraph_members_falls_back_without_a_reason_line() {
        let body = "flowchart LR\n    A --> B --> C\n    subgraph s\n    A\n    C\n    end\n";

        assert_eq!(mermaid(body), format!("```mermaid\n{body}```"));
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
    fn mermaid_empty_subgraph_falls_back_without_a_reason_line() {
        for body in [
            "flowchart LR\n    subgraph s\n    end\n    A --> B\n",
            "flowchart LR\n    A --> B\n    subgraph s\n    style A fill:#f9f\n    end\n",
        ] {
            assert_eq!(mermaid(body), format!("```mermaid\n{body}```"));
        }
    }

    #[test]
    fn mermaid_nested_subgraph_and_inner_direction_fall_back_without_a_reason_line() {
        for body in [
            "flowchart LR\n    subgraph a\n    subgraph b\n    A\n    end\n    end\n",
            "flowchart LR\n    subgraph a\n    direction TD\n    A\n    end\n",
        ] {
            assert_eq!(mermaid(body), format!("```mermaid\n{body}```"));
        }
    }

    #[test]
    fn mermaid_top_level_direction_statement_is_ignored_like_mermaid() {
        assert_eq!(mermaid("flowchart LR\n    direction TB\n    A --> B\n"), mermaid(LR_A_TO_B));
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
    fn mermaid_link_to_a_subgraph_id_falls_back_without_a_reason_line() {
        for body in [
            "flowchart LR\n    A --> s\n    subgraph s\n    B\n    end\n",
            "flowchart LR\n    subgraph s\n    B\n    end\n    A --> s\n",
        ] {
            assert_eq!(mermaid(body), format!("```mermaid\n{body}```"));
        }
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
            mermaid("flowchart LR\n    A -->|\"a|b\"| B\n").lines().next(),
            Some("┌───┐ a|b ┌───┐")
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
        for (link, label) in [("A -- \"a--b\" --> B", "a--b"), ("A -. \"a.-b\" .-> B", "a.-b")] {
            let output = mermaid(&format!("flowchart LR\n    {link}\n"));

            assert_eq!(output.lines().next(), Some(format!("┌───┐ {label} ┌───┐").as_str()));
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
    fn mermaid_link_to_a_quoted_subgraph_id_falls_back_without_a_reason_line() {
        let body = "flowchart LR\n    subgraph \"s\"\n    B\n    end\n    A --> s\n";

        assert_eq!(mermaid(body), format!("```mermaid\n{body}```"));
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
    fn mermaid_unsupported_link_syntax_falls_back_without_a_reason_line() {
        for body in ["flowchart LR\n    A ~~~ B\n", "flowchart LR\n    A e1@--> B\n"] {
            assert_eq!(mermaid(body), format!("```mermaid\n{body}```"));
        }
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
    fn mermaid_subgraph_id_referenced_inside_another_subgraph_falls_back_without_a_reason_line() {
        for body in [
            "flowchart LR\n    subgraph a\n    b\n    end\n    subgraph b\n    X\n    end\n",
            "flowchart LR\n    subgraph b\n    X\n    end\n    subgraph a\n    b\n    end\n",
        ] {
            assert_eq!(mermaid(body), format!("```mermaid\n{body}```"));
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
