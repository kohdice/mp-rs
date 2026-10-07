//! Lays out one block as styled lines. Spacing between top-level blocks is the caller's.

mod code;
mod list;
mod quote;
mod table;
mod text;

use crate::ansi::ColorMode;
use crate::model::Block;
use crate::style::{Line, Span, Style};
use crate::theme::solarized::DARK_PALETTE;

/// Lays out `block` in `width` columns; `None` means no width limit, so nothing is
/// wrapped. Code blocks are syntax highlighted only when `color` is `Ansi`. `depth` is
/// the list nesting level, which picks bullet shapes; top-level blocks pass 0.
pub(crate) fn lay_out_block(
    block: &Block,
    width: Option<usize>,
    color: ColorMode,
    depth: usize,
) -> Vec<Line> {
    match block {
        Block::Paragraph(inlines) => text::lay_out_inlines(
            inlines,
            Style { fg: Some(DARK_PALETTE.body), ..Style::default() },
            width,
        ),
        Block::Heading { level, inlines } => {
            text::lay_out_inlines(inlines, heading_style(*level), width)
        }
        Block::BlockQuote(blocks) => quote::lay_out_quote(blocks, width, color, depth),
        Block::Alert { kind, title, blocks } => {
            quote::lay_out_alert(*kind, title, blocks, width, color, depth)
        }
        Block::List { start, tight, items } => {
            list::lay_out_list(*start, *tight, items, width, color, depth)
        }
        Block::ThematicBreak => {
            let columns =
                width.map_or(THEMATIC_BREAK_WIDTH, |width| width.clamp(1, THEMATIC_BREAK_WIDTH));
            let style = Style { fg: Some(DARK_PALETTE.muted), ..Style::default() };
            vec![vec![Span { text: "─".repeat(columns), style }]]
        }
        Block::Table { align, header, rows } => table::lay_out_table(align, header, rows, width),
        Block::CodeBlock { info, code } => code::lay_out_code_block(info, code, width, color),
        Block::Html(html) => code::lay_out_html(html),
    }
}

/// Lays out sibling blocks, with one blank line between them when `blank_between`.
/// A block that lays out to no lines gets no separator either.
fn lay_out_blocks(
    blocks: &[Block],
    blank_between: bool,
    width: Option<usize>,
    color: ColorMode,
    depth: usize,
) -> Vec<Line> {
    let mut lines = Vec::new();
    for block in blocks {
        let block_lines = lay_out_block(block, width, color, depth);
        if block_lines.is_empty() {
            continue;
        }
        if blank_between && !lines.is_empty() {
            lines.push(Line::new());
        }
        lines.extend(block_lines);
    }
    lines
}

/// Width left after a prefix of `columns`. It never drops below 1, so wrapping still
/// places one grapheme cluster per line when prefixes consume the whole width.
fn narrow(width: Option<usize>, columns: usize) -> Option<usize> {
    width.map(|width| width.saturating_sub(columns).max(1))
}

const THEMATIC_BREAK_WIDTH: usize = 32;

fn heading_style(level: u8) -> Style {
    let color = DARK_PALETTE.heading_colors.get(usize::from(level).saturating_sub(1));
    Style { fg: color.copied(), bold: level <= 4, underline: level <= 2, ..Style::default() }
}

#[cfg(test)]
mod tests {
    use super::lay_out_block;
    use crate::ansi::{ColorMode, to_ansi};
    use crate::markdown::parse;
    use crate::model::{Block, ListItem};
    use crate::style::{Line, Span, Style};
    use crate::theme::solarized::{
        BLUE, CYAN, DARK_PALETTE, GREEN, MAGENTA, ORANGE, RED, VIOLET, YELLOW,
    };
    use unicode_width::UnicodeWidthStr;

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

    #[test]
    fn paragraph_without_width_is_one_body_colored_line() {
        let lines = lay_out("Hello world\n", None);

        assert_eq!(to_ansi(&lines, ColorMode::Plain), "Hello world");
        assert_eq!(lines.len(), 1);
        assert!(lines.iter().flatten().all(|span| span.style.fg == Some(DARK_PALETTE.body)));
    }

    #[test]
    fn soft_breaks_start_new_lines_without_a_width_limit() {
        assert_eq!(plain("a\nb\n", None), "a\nb");
    }

    #[test]
    fn soft_breaks_become_spaces_when_reflowing() {
        assert_eq!(plain("a\nb\n", Some(80)), "a b");
    }

    #[test]
    fn hard_breaks_start_new_lines_when_reflowing() {
        assert_eq!(plain("a  \nb\n", Some(80)), "a\nb");
    }

    #[test]
    fn hard_breaks_start_new_lines_without_a_width_limit() {
        assert_eq!(plain("a  \nb\n", None), "a\nb");
    }

    #[test]
    fn wraps_whole_words_that_do_not_fit() {
        assert_eq!(plain("hello world\n", Some(10)), "hello\nworld");
    }

    #[test]
    fn keeps_text_that_exactly_fills_the_width_on_one_line() {
        assert_eq!(plain("hello world\n", Some(11)), "hello world");
    }

    #[test]
    fn drops_separator_spaces_at_wrap_points() {
        assert_eq!(plain("a  b\n", Some(1)), "a\nb");
    }

    #[test]
    fn keeps_runs_of_spaces_between_words_that_fit() {
        assert_eq!(plain("a  b\n", Some(80)), "a  b");
    }

    #[test]
    fn splits_an_oversized_ascii_word_into_width_sized_chunks() {
        assert_eq!(plain("abcdefghijk\n", Some(5)), "abcde\nfghij\nk");
    }

    #[test]
    fn splits_oversized_japanese_text_without_breaking_wide_characters() {
        assert_eq!(plain("あいうえおかきくけこ\n", Some(5)), "あい\nうえ\nおか\nきく\nけこ");
    }

    #[test]
    fn never_splits_a_grapheme_cluster_inside_an_oversized_word() {
        assert_eq!(
            plain("aaaa\u{1f469}\u{200d}\u{1f4bb}bbbb\n", Some(5)),
            "aaaa\n\u{1f469}\u{200d}\u{1f4bb}bbb\nb"
        );
    }

    #[test]
    fn measures_words_by_whole_string_width() {
        assert_eq!(plain("\u{644}\u{627}\n", Some(1)), "\u{644}\u{627}");
    }

    #[test]
    fn inline_styles_survive_wrap_points_without_leaking() {
        let lines = lay_out("**bold words** tail\n", Some(5));

        assert_eq!(to_ansi(&lines, ColorMode::Plain), "bold\nwords\ntail");
        let [bold, words, tail] = lines.as_slice() else {
            panic!("expected three lines, got {lines:?}");
        };
        assert!(bold.iter().chain(words).all(|span| span.style.bold));
        assert!(tail.iter().all(|span| !span.style.bold));
    }

    #[test]
    fn words_spanning_style_boundaries_wrap_as_one_word() {
        let lines = lay_out("**ab**cd efgh\n", Some(5));

        assert_eq!(to_ansi(&lines, ColorMode::Plain), "abcd\nefgh");
        let bold_flags: Vec<(&str, bool)> =
            lines[0].iter().map(|span| (span.text.as_str(), span.style.bold)).collect();
        assert_eq!(bold_flags, [("ab", true), ("cd", false)]);
    }

    #[test]
    fn styles_emphasis_strong_strikethrough_and_code_spans() {
        let lines = lay_out("*i* **b** ~~s~~ `x`\n", None);

        let body = Style { fg: Some(DARK_PALETTE.body), ..Style::default() };
        let space = span(" ", body);
        assert_eq!(
            lines,
            [vec![
                span("i", Style { italic: true, ..body }),
                space.clone(),
                span("b", Style { bold: true, ..body }),
                space.clone(),
                span("s", Style { strikethrough: true, ..body }),
                space,
                span("`x`", Style { fg: Some(DARK_PALETTE.inline_code), ..Style::default() }),
            ]]
        );
    }

    #[test]
    fn inline_code_keeps_the_surrounding_emphasis() {
        let lines = lay_out("**`x`**\n", None);

        let style = Style { fg: Some(DARK_PALETTE.inline_code), bold: true, ..Style::default() };
        assert_eq!(lines, [vec![span("`x`", style)]]);
    }

    #[test]
    fn shows_link_urls_and_titles_only_when_show_url_is_set() {
        let lines = lay_out("[t](u \"T\")\n", None);

        let muted = Style { fg: Some(DARK_PALETTE.muted), dim: true, ..Style::default() };
        assert_eq!(
            lines,
            [vec![
                span(
                    "t",
                    Style { fg: Some(DARK_PALETTE.link), underline: true, ..Style::default() }
                ),
                span("(u)", muted),
                span(" — T", Style { italic: true, ..muted }),
            ]]
        );
        assert_eq!(plain("<https://x.test>\n", None), "https://x.test");
    }

    #[test]
    fn link_text_spaces_keep_the_link_style() {
        let lines = lay_out("[a b](u)\n", Some(80));

        let link = Style { fg: Some(DARK_PALETTE.link), underline: true, ..Style::default() };
        let muted = Style { fg: Some(DARK_PALETTE.muted), dim: true, ..Style::default() };
        assert_eq!(lines, [vec![span("a b", link), span("(u)", muted)]]);
    }

    #[test]
    fn renders_images_as_alt_text_with_url() {
        assert_eq!(plain("![alt](i.png)\n", None), "[img: alt](i.png)");
    }

    #[test]
    fn shows_image_titles_after_the_url() {
        assert_eq!(plain("![a](i.png \"T\")\n", None), "[img: a](i.png) — T");
    }

    #[test]
    fn styles_headings_by_level() {
        let cases = [
            (
                "# Title\n",
                Style { fg: Some(YELLOW), bold: true, underline: true, ..Style::default() },
            ),
            (
                "## Title\n",
                Style { fg: Some(ORANGE), bold: true, underline: true, ..Style::default() },
            ),
            ("### Title\n", Style { fg: Some(MAGENTA), bold: true, ..Style::default() }),
            ("#### Title\n", Style { fg: Some(CYAN), bold: true, ..Style::default() }),
            ("##### Title\n", Style { fg: Some(BLUE), ..Style::default() }),
            ("###### Title\n", Style { fg: Some(VIOLET), ..Style::default() }),
        ];
        for (markdown, style) in cases {
            assert_eq!(lay_out(markdown, None), [vec![span("Title", style)]], "{markdown:?}");
        }
    }

    #[test]
    fn headings_wrap_like_paragraphs() {
        assert_eq!(plain("# hello world again\n", Some(10)), "hello\nworld\nagain");
    }

    #[test]
    fn thematic_break_is_32_columns_or_the_available_width() {
        let muted = Style { fg: Some(DARK_PALETTE.muted), ..Style::default() };
        assert_eq!(lay_out("---\n", None), [vec![span(&"─".repeat(32), muted)]]);
        assert_eq!(plain("---\n", Some(10)), "─".repeat(10));
        assert_eq!(plain("---\n", Some(0)), "─");
    }

    #[test]
    fn tight_unordered_list_has_one_line_per_item() {
        assert_eq!(plain("- a\n- b\n", None), "• a\n• b");
    }

    #[test]
    fn nested_lists_cycle_bullets_and_indent_to_the_content_column() {
        assert_eq!(plain("- a\n  - b\n    - c\n", None), "• a\n  ◦ b\n    ▪ c");
    }

    #[test]
    fn ordered_lists_number_from_start() {
        assert_eq!(plain("9. a\n10. b\n", None), "9. a\n10. b");
    }

    #[test]
    fn task_items_show_check_boxes_after_the_marker() {
        let lines = lay_out("- [ ] a\n- [x] b\n", None);

        assert_eq!(to_ansi(&lines, ColorMode::Plain), "• ☐ a\n• ☑ b");
        let box_style = |line: &Line| {
            line.iter().find(|span| span.text.contains(['☐', '☑'])).map(|span| span.style)
        };
        let unchecked = Style { fg: Some(DARK_PALETTE.muted), dim: true, ..Style::default() };
        let checked = Style { fg: Some(DARK_PALETTE.list_marker), ..Style::default() };
        assert_eq!(box_style(&lines[0]), Some(unchecked));
        assert_eq!(box_style(&lines[1]), Some(checked));
    }

    #[test]
    fn loose_lists_separate_items_and_child_blocks_with_one_blank_line() {
        assert_eq!(plain("- a\n\n  b\n- c\n", None), "• a\n\n  b\n\n• c");
    }

    #[test]
    fn tight_list_item_with_lazy_text_and_nested_list_has_no_blank_lines() {
        assert_eq!(plain("- a\n  b\n  - c\n- d\n", None), "• a\n  b\n  ◦ c\n• d");
    }

    #[test]
    fn wrapped_list_item_lines_indent_to_the_content_column() {
        let lines = lay_out("- one two three four\n", Some(10));

        assert_eq!(to_ansi(&lines, ColorMode::Plain), "• one two\n  three\n  four");
        let indent = span("  ", Style::default());
        assert!(lines[1..].iter().all(|line| line.first() == Some(&indent)), "{lines:?}");
    }

    #[test]
    fn empty_list_item_renders_only_its_marker() {
        assert_eq!(plain("-\n", None), "•");
    }

    #[test]
    fn empty_task_item_renders_marker_and_box_without_trailing_space() {
        // Markdown cannot produce a task item without content, so the block is built by hand.
        let list = Block::List {
            start: None,
            tight: true,
            items: vec![ListItem { task: Some(false), blocks: Vec::new() }],
        };
        assert_eq!(
            to_ansi(&lay_out_block(&list, None, ColorMode::Plain, 0), ColorMode::Plain),
            "• ☐"
        );
    }

    #[test]
    fn wrapped_task_item_lines_indent_past_the_check_box() {
        assert_eq!(plain("- [ ] one two three\n", Some(10)), "• ☐ one\n    two\n    three");
    }

    #[test]
    fn quote_prefixes_each_line_with_a_bar() {
        let lines = lay_out("> a\n> b\n", None);

        assert_eq!(to_ansi(&lines, ColorMode::Plain), "│ a\n│ b");
        let bar = span("│ ", Style { fg: Some(DARK_PALETTE.muted), dim: true, ..Style::default() });
        assert!(lines.iter().all(|line| line.first() == Some(&bar)), "{lines:?}");
        assert_eq!(plain("> a\n> b\n", Some(80)), "│ a b");
    }

    #[test]
    fn quote_separates_child_blocks_with_a_bare_bar_line() {
        assert_eq!(plain("> # T\n> text\n", None), "│ T\n│\n│ text");
    }

    #[test]
    fn quote_skips_empty_headings_between_child_blocks() {
        assert_eq!(plain("> a\n>\n> #\n>\n> b\n", None), "│ a\n│\n│ b");
    }

    #[test]
    fn nested_quotes_repeat_the_bar() {
        assert_eq!(plain("> > a\n> >\n> > b\n", None), "│ │ a\n│ │\n│ │ b");
    }

    #[test]
    fn loose_list_inside_a_quote_keeps_the_bar_on_blank_lines() {
        assert_eq!(plain("> - a\n>\n> - b\n", None), "│ • a\n│\n│ • b");
    }

    #[test]
    fn wrapped_quote_lines_keep_the_bar_and_lose_two_columns() {
        assert_eq!(plain("> aaaa bbbbb cc dddddddd\n", Some(10)), "│ aaaa\n│ bbbbb cc\n│ dddddddd");
    }

    #[test]
    fn zero_content_width_still_places_one_grapheme_per_line() {
        assert_eq!(plain("> abc", Some(2)), "│ a\n│ b\n│ c");
    }

    #[test]
    fn quote_inside_a_list_item_hangs_after_the_marker() {
        assert_eq!(plain("- > a\n  > b\n", None), "• │ a\n  │ b");
    }

    #[test]
    fn empty_quote_renders_a_single_bar() {
        assert_eq!(plain(">\n", None), "│");
    }

    #[test]
    fn note_alert_colors_its_bar_and_title_blue() {
        let lines = lay_out("> [!NOTE]\n> body\n", None);

        assert_eq!(to_ansi(&lines, ColorMode::Plain), "│ NOTE\n│\n│ body");
        let bar = Style { fg: Some(BLUE), ..Style::default() };
        let title = Style { fg: Some(BLUE), bold: true, ..Style::default() };
        assert_eq!(lines[0], vec![span("│ ", bar), span("NOTE", title)]);
        assert_eq!(lines[1], vec![span("│", bar)]);
        assert_eq!(lines[2].first(), Some(&span("│ ", bar)), "{lines:?}");
    }

    #[test]
    fn each_alert_kind_has_its_own_bar_and_title_color() {
        for (kind, color) in
            [("TIP", GREEN), ("IMPORTANT", VIOLET), ("WARNING", YELLOW), ("CAUTION", RED)]
        {
            let lines = lay_out(&format!("> [!{kind}]\n> body\n"), None);

            let title = Style { fg: Some(color), bold: true, ..Style::default() };
            assert!(lines[0].contains(&span(kind, title)), "{kind}: {lines:?}");
            let bar = Style { fg: Some(color), ..Style::default() };
            assert!(
                lines.iter().all(|line| line.first().map(|span| span.style) == Some(bar)),
                "{kind}: {lines:?}"
            );
        }
    }

    #[test]
    fn alert_body_keeps_the_body_color() {
        let lines = lay_out("> [!CAUTION]\n> body **strong**\n", None);

        let body = Style { fg: Some(DARK_PALETTE.body), ..Style::default() };
        let strong = Style { bold: true, ..body };
        assert_eq!(lines[2][1..], [span("body ", body), span("strong", strong)], "{lines:?}");
    }

    #[test]
    fn custom_alert_title_is_colored_by_its_kind() {
        let lines = lay_out("> [!TIP] Custom title\n> body\n", None);

        assert_eq!(to_ansi(&lines, ColorMode::Plain), "│ Custom title\n│\n│ body");
        let title = Style { fg: Some(GREEN), bold: true, ..Style::default() };
        assert!(lines[0].contains(&span("Custom title", title)), "{lines:?}");
        let bar = Style { fg: Some(GREEN), ..Style::default() };
        assert!(
            lines.iter().all(|line| line.first().map(|span| span.style) == Some(bar)),
            "{lines:?}"
        );
    }

    #[test]
    fn quote_nested_in_an_alert_keeps_the_muted_inner_bar() {
        let lines = lay_out("> [!NOTE]\n> > inner\n", None);

        assert_eq!(to_ansi(&lines, ColorMode::Plain), "│ NOTE\n│\n│ │ inner");
        let outer = Style { fg: Some(BLUE), ..Style::default() };
        let inner = Style { fg: Some(DARK_PALETTE.muted), dim: true, ..Style::default() };
        assert_eq!(lines[2][..2], [span("│ ", outer), span("│ ", inner)], "{lines:?}");
    }

    #[test]
    fn alert_without_a_body_is_a_single_title_line() {
        let lines = lay_out("> [!NOTE]\n", None);

        assert_eq!(to_ansi(&lines, ColorMode::Plain), "│ NOTE");
        assert_eq!(lines.len(), 1, "{lines:?}");
    }

    #[test]
    fn renders_a_table_with_box_borders_and_padding() {
        let lines = lay_out("| H | I |\n| - | - |\n| a | b |\n", None);

        assert_eq!(
            to_ansi(&lines, ColorMode::Plain),
            "┌─────┬─────┐\n│ H   │ I   │\n├─────┼─────┤\n│ a   │ b   │\n└─────┴─────┘"
        );
        let muted = Some(DARK_PALETTE.muted);
        let body = Some(DARK_PALETTE.body);
        for span in lines.iter().flatten() {
            if span.text.contains(['┌', '─', '│', '└', '├']) {
                assert_eq!(span.style.fg, muted, "{span:?}");
            } else if !span.text.trim().is_empty() {
                assert_eq!(span.style.fg, body, "{span:?}");
            }
        }
    }

    #[test]
    fn aligns_cells_left_center_and_right() {
        assert_eq!(
            plain("| HHHHH | HHHHH | HHHHH |\n| :- | :-: | -: |\n| a | a | a |\n", None)
                .lines()
                .nth(3),
            Some("│ a     │   a   │     a │")
        );
    }

    #[test]
    fn shrinks_columns_to_fit_the_width_and_wraps_cells() {
        let markdown = "| Crate | Responsibility |\n\
             | --- | --- |\n\
             | mp-preview | Block-to-terminal rendering with a trailing-newline guarantee |\n";

        let output = plain(markdown, Some(40));

        let lines: Vec<&str> = output.lines().collect();
        assert!(lines.len() > 5, "the long cell must wrap onto several lines: {output}");
        let table_width = lines[0].width();
        assert!(table_width <= 40, "{output}");
        assert!(lines.iter().all(|line| line.width() == table_width), "{output}");
        assert!(
            lines[1..lines.len() - 1]
                .iter()
                .all(|line| line.starts_with(['│', '├']) && line.ends_with(['│', '┤'])),
            "every row line must be closed by borders: {output}"
        );
    }

    #[test]
    fn wrapped_cell_styles_stay_inside_the_cell() {
        let lines = lay_out("| H | H |\n| - | - |\n| ~~abcdef~~ | ok |\n", Some(13));

        assert_eq!(
            to_ansi(&lines[3..5], ColorMode::Plain),
            "│ abc │ ok  │\n│ def │     │",
            "{lines:?}"
        );
        for span in lines[3..5].iter().flatten() {
            let struck = matches!(span.text.as_str(), "abc" | "def");
            assert_eq!(span.style.strikethrough, struck, "{span:?}");
        }
    }

    #[test]
    fn measures_joined_emoji_from_numeric_references_as_one_cluster() {
        let output = plain("| H |\n| - |\n| &#x1F469;&#x200D;&#x1F4BB; |\n", None);

        let lines: Vec<&str> = output.lines().collect();
        assert_eq!(lines.first(), Some(&"┌─────┐"), "{output}");
        assert!(lines.iter().all(|line| line.width() == 7), "{output}");
    }

    #[test]
    fn aligns_cells_using_whole_line_widths() {
        let output = plain("| H |\n| -: |\n| \u{644}\u{627} |\n", None);

        assert_eq!(output.lines().nth(3), Some("│   \u{644}\u{627} │"), "{output}");
        assert!(output.lines().all(|line| line.width() == 7), "{output}");
    }

    #[test]
    fn table_inside_a_quote_fits_the_width_after_the_bar() {
        let output = plain("> | H | H |\n> | - | - |\n> | abcdef | ok |\n", Some(15));

        assert!(output.lines().all(|line| line.width() <= 15), "{output}");
        assert!(output.lines().all(|line| line.starts_with("│ ")), "{output}");
    }

    #[test]
    fn empty_table_cell_renders_as_padding() {
        let output = plain("| H | I |\n| - | - |\n| a |  |\n", None);

        assert_eq!(output.lines().nth(3), Some("│ a   │     │"), "{output}");
    }

    #[test]
    fn code_block_renders_fences_info_and_body() {
        let lines = lay_out("```rust\nlet x = 1;\n```\n", None);

        assert_eq!(to_ansi(&lines, ColorMode::Plain), "```rust\nlet x = 1;\n```");
        let fence = Style { fg: Some(DARK_PALETTE.code_fence), dim: true, ..Style::default() };
        assert_eq!(lines.first(), Some(&vec![span("```rust", fence)]));
        assert_eq!(lines.last(), Some(&vec![span("```", fence)]));
    }

    #[test]
    fn empty_code_block_renders_only_fences() {
        assert_eq!(plain("```\n```\n", None), "```\n```");
    }

    #[test]
    fn code_block_expands_tabs_to_four_column_stops() {
        let block = Block::CodeBlock { info: String::new(), code: "\tx\nab\tc\n".to_owned() };
        assert_eq!(
            to_ansi(&lay_out_block(&block, None, ColorMode::Plain, 0), ColorMode::Plain),
            "```\n    x\nab  c\n```"
        );
    }

    #[test]
    fn code_blocks_are_never_wrapped() {
        let code_line = "a ".repeat(50);

        let output = plain(&format!("```\n{code_line}\n```\n"), Some(20));

        assert_eq!(output, format!("```\n{code_line}\n```"));
    }

    #[test]
    fn code_block_tab_stops_ignore_list_and_quote_prefixes() {
        assert_eq!(plain("> ```\n> ab\tc\n> ```\n", None), "│ ```\n│ ab  c\n│ ```");
        assert_eq!(plain("- ```\n  ab\tc\n  ```\n", None), "• ```\n  ab  c\n  ```");
    }

    fn code_body_spans(markdown: &str, color: ColorMode) -> Vec<Span> {
        let lines: Vec<Line> =
            parse(markdown).iter().flat_map(|block| lay_out_block(block, None, color, 0)).collect();
        let body = lines.get(1..lines.len().saturating_sub(1)).unwrap_or_default();
        body.iter().flatten().cloned().collect()
    }

    #[test]
    fn highlights_known_languages_in_ansi_mode() {
        let spans = code_body_spans("```rust\nfn main() { let x = 1; }\n```\n", ColorMode::Ansi);

        let mut colors: Vec<_> = spans.iter().map(|span| span.style.fg).collect();
        colors.sort_unstable_by_key(|fg| fg.map(|rgb| (rgb.r, rgb.g, rgb.b)));
        colors.dedup();
        assert!(colors.len() > 1, "{spans:?}");
    }

    #[test]
    fn falls_back_to_inline_code_color_for_unknown_languages() {
        let spans = code_body_spans("```not-a-language\nfn main() {}\n```\n", ColorMode::Ansi);

        assert!(!spans.is_empty());
        assert!(spans.iter().all(|span| span.style.fg == Some(DARK_PALETTE.inline_code)));
    }

    #[test]
    fn plain_mode_code_body_uses_the_fallback_style() {
        let spans = code_body_spans("```rust\nfn main() { let x = 1; }\n```\n", ColorMode::Plain);

        let fallback = Style { fg: Some(DARK_PALETTE.inline_code), ..Style::default() };
        assert_eq!(spans, [span("fn main() { let x = 1; }", fallback)]);
    }

    #[test]
    fn highlighted_code_expands_tabs_across_span_boundaries() {
        let spans = code_body_spans("```rust\nlet x\t= 1;\n```\n", ColorMode::Ansi);

        // The tab must sit in a later span than `let` for the column to be carried over.
        assert_eq!(spans.first().map(|span| span.text.as_str()), Some("let"));
        let text: String = spans.iter().map(|span| span.text.as_str()).collect();
        assert_eq!(text, "let x   = 1;");
    }

    #[test]
    fn code_block_keeps_interior_blank_lines() {
        assert_eq!(plain("```\na\n\nb\n```\n", None), "```\na\n\nb\n```");
    }

    #[test]
    fn html_block_renders_verbatim_without_its_final_newline_or_wrapping() {
        assert_eq!(plain("<div>\n\tx\n</div>\n", Some(3)), "<div>\n    x\n</div>");
    }

    #[test]
    fn unclosed_html_block_drops_whitespace_only_trailing_lines() {
        assert_eq!(plain("<!--\nfoo\n  \n", None), "<!--\nfoo");
    }
}
