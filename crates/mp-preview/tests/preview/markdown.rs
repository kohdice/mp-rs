//! Markdown rendering, checked through the output of [`preview`].

use std::io;

use crate::common::plain;
use mp_preview::{ColorMode, Options, preview};
use unicode_width::UnicodeWidthStr;

#[test]
fn preview_writes_nothing_for_empty_input() -> io::Result<()> {
    let mut output = Vec::new();

    preview("", &Options::default(), &mut output)?;

    assert!(output.is_empty(), "expected no bytes, got {output:?}");
    Ok(())
}

#[test]
fn preview_separates_top_level_blocks_with_one_blank_line() {
    assert_eq!(plain("# Title\n\n\n\nHello\n", None), "Title\n\nHello\n");
}

#[test]
fn preview_ends_non_empty_output_with_exactly_one_newline() {
    assert_eq!(plain("Hello", None), "Hello\n");
    assert_eq!(plain("Hello\n\n\n", None), "Hello\n");
}

#[test]
fn preview_skips_empty_headings_between_blocks() {
    assert_eq!(plain("a\n\n#\n\nb\n", None), "a\n\nb\n");
}

#[test]
fn wraps_words_that_exactly_fit_the_width() {
    assert_eq!(plain("hello world\n", Some(5)), "hello\nworld\n");
}

#[test]
fn preview_emits_escape_sequences_in_ansi_mode() {
    let render = |color| {
        let mut output = Vec::new();
        preview("hello world\n", &Options { width: Some(5), color }, &mut output)
            .unwrap_or_else(|error| panic!("preview failed: {error}"));
        String::from_utf8(output).unwrap_or_else(|error| panic!("{error}"))
    };

    let ansi = render(ColorMode::Ansi);
    let plain = render(ColorMode::Plain);

    assert!(ansi.contains("\x1b["), "{ansi:?}");
    assert!(!plain.contains('\x1b'), "{plain:?}");
}

#[test]
fn preview_colors_a_styled_mermaid_node_in_ansi_mode() {
    let mut output = Vec::new();
    preview(
        "```mermaid\nflowchart LR\n    A --> B\n    style A stroke:#ff0000\n```\n",
        &Options { width: None, color: ColorMode::Ansi },
        &mut output,
    )
    .unwrap_or_else(|error| panic!("preview failed: {error}"));
    let text = String::from_utf8(output).unwrap_or_else(|error| panic!("{error}"));

    assert!(text.contains("38;2;255;0;0"), "{text:?}");
}

#[test]
fn preview_writes_earlier_blocks_before_later_ones_fail() {
    let mut writer = FirstWriteOnly { bytes: Vec::new(), writes: 0 };

    let result = preview("a\n\nb\n", &Options::default(), &mut writer);

    assert!(result.is_err());
    assert_eq!(writer.bytes, b"a\n");
}

/// Accepts its first `write` call in full and fails every later one.
struct FirstWriteOnly {
    bytes: Vec<u8>,
    writes: usize,
}

impl io::Write for FirstWriteOnly {
    fn write(&mut self, buffer: &[u8]) -> io::Result<usize> {
        self.writes += 1;
        if self.writes > 1 {
            return Err(io::Error::other("closed writer"));
        }
        self.bytes.extend_from_slice(buffer);
        Ok(buffer.len())
    }

    fn flush(&mut self) -> io::Result<()> {
        Ok(())
    }
}

#[test]
fn soft_breaks_start_new_lines_without_a_width_limit() {
    assert_eq!(plain("a\nb\n", None), "a\nb\n");
}

#[test]
fn soft_breaks_become_spaces_when_reflowing() {
    assert_eq!(plain("a\nb\n", Some(80)), "a b\n");
}

#[test]
fn hard_breaks_start_new_lines_when_reflowing() {
    assert_eq!(plain("a  \nb\n", Some(80)), "a\nb\n");
}

#[test]
fn hard_breaks_start_new_lines_without_a_width_limit() {
    assert_eq!(plain("a  \nb\n", None), "a\nb\n");
}

#[test]
fn wraps_whole_words_that_do_not_fit() {
    assert_eq!(plain("hello world\n", Some(10)), "hello\nworld\n");
}

#[test]
fn keeps_text_that_exactly_fills_the_width_on_one_line() {
    assert_eq!(plain("hello world\n", Some(11)), "hello world\n");
}

#[test]
fn drops_separator_spaces_at_wrap_points() {
    assert_eq!(plain("a  b\n", Some(1)), "a\nb\n");
}

#[test]
fn keeps_runs_of_spaces_between_words_that_fit() {
    assert_eq!(plain("a  b\n", Some(80)), "a  b\n");
}

#[test]
fn splits_an_oversized_ascii_word_into_width_sized_chunks() {
    assert_eq!(plain("abcdefghijk\n", Some(5)), "abcde\nfghij\nk\n");
}

#[test]
fn splits_oversized_japanese_text_without_breaking_wide_characters() {
    assert_eq!(plain("あいうえおかきくけこ\n", Some(5)), "あい\nうえ\nおか\nきく\nけこ\n");
}

#[test]
fn never_splits_a_grapheme_cluster_inside_an_oversized_word() {
    assert_eq!(
        plain("aaaa\u{1f469}\u{200d}\u{1f4bb}bbbb\n", Some(5)),
        "aaaa\n\u{1f469}\u{200d}\u{1f4bb}bbb\nb\n"
    );
}

#[test]
fn measures_words_by_whole_string_width() {
    assert_eq!(plain("\u{644}\u{627}\n", Some(1)), "\u{644}\u{627}\n");
}

#[test]
fn shows_link_titles_even_when_the_url_is_hidden() {
    assert_eq!(
        plain("[https://example.com](https://example.com \"Example site\")\n", None),
        "https://example.com — Example site\n"
    );
}

#[test]
fn renders_images_as_alt_text_with_url() {
    assert_eq!(plain("![alt](i.png)\n", None), "[img: alt](i.png)\n");
}

#[test]
fn shows_image_titles_after_the_url() {
    assert_eq!(plain("![a](i.png \"T\")\n", None), "[img: a](i.png) — T\n");
}

#[test]
fn headings_wrap_like_paragraphs() {
    assert_eq!(plain("# hello world again\n", Some(10)), "hello\nworld\nagain\n");
}

#[test]
fn tight_unordered_list_has_one_line_per_item() {
    assert_eq!(plain("- a\n- b\n", None), "• a\n• b\n");
}

#[test]
fn nested_lists_cycle_bullets_and_indent_to_the_content_column() {
    assert_eq!(plain("- a\n  - b\n    - c\n", None), "• a\n  ◦ b\n    ▪ c\n");
}

#[test]
fn ordered_lists_number_from_start() {
    assert_eq!(plain("9. a\n10. b\n", None), "9. a\n10. b\n");
}

#[test]
fn loose_lists_separate_items_and_child_blocks_with_one_blank_line() {
    assert_eq!(plain("- a\n\n  b\n- c\n", None), "• a\n\n  b\n\n• c\n");
}

#[test]
fn tight_list_item_with_lazy_text_and_nested_list_has_no_blank_lines() {
    assert_eq!(plain("- a\n  b\n  - c\n- d\n", None), "• a\n  b\n  ◦ c\n• d\n");
}

#[test]
fn empty_list_item_renders_only_its_marker() {
    assert_eq!(plain("-\n", None), "•\n");
}

#[test]
fn wrapped_task_item_lines_indent_past_the_check_box() {
    assert_eq!(plain("- [ ] one two three\n", Some(10)), "• ☐ one\n    two\n    three\n");
}

#[test]
fn quote_reflows_soft_breaks_like_paragraphs() {
    assert_eq!(plain("> a\n> b\n", Some(80)), "│ a b\n");
}

#[test]
fn quote_separates_child_blocks_with_a_bare_bar_line() {
    assert_eq!(plain("> # T\n> text\n", None), "│ T\n│\n│ text\n");
}

#[test]
fn quote_skips_empty_headings_between_child_blocks() {
    assert_eq!(plain("> a\n>\n> #\n>\n> b\n", None), "│ a\n│\n│ b\n");
}

#[test]
fn nested_quotes_repeat_the_bar() {
    assert_eq!(plain("> > a\n> >\n> > b\n", None), "│ │ a\n│ │\n│ │ b\n");
}

#[test]
fn loose_list_inside_a_quote_keeps_the_bar_on_blank_lines() {
    assert_eq!(plain("> - a\n>\n> - b\n", None), "│ • a\n│\n│ • b\n");
}

#[test]
fn wrapped_quote_lines_keep_the_bar_and_lose_two_columns() {
    assert_eq!(plain("> aaaa bbbbb cc dddddddd\n", Some(10)), "│ aaaa\n│ bbbbb cc\n│ dddddddd\n");
}

#[test]
fn zero_content_width_still_places_one_grapheme_per_line() {
    assert_eq!(plain("> abc", Some(2)), "│ a\n│ b\n│ c\n");
}

#[test]
fn quote_inside_a_list_item_hangs_after_the_marker() {
    assert_eq!(plain("- > a\n  > b\n", None), "• │ a\n  │ b\n");
}

#[test]
fn empty_quote_renders_a_single_bar() {
    assert_eq!(plain(">\n", None), "│\n");
}

#[test]
fn aligns_cells_left_center_and_right() {
    assert_eq!(
        plain("| HHHHH | HHHHH | HHHHH |\n| :- | :-: | -: |\n| a | a | a |\n", None).lines().nth(3),
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
fn empty_code_block_renders_only_fences() {
    assert_eq!(plain("```\n```\n", None), "```\n```\n");
}

#[test]
fn code_blocks_are_never_wrapped() {
    let code_line = "a ".repeat(50);

    let output = plain(&format!("```\n{code_line}\n```\n"), Some(20));

    assert_eq!(output, format!("```\n{code_line}\n```\n"));
}

#[test]
fn code_block_tab_stops_ignore_list_and_quote_prefixes() {
    assert_eq!(plain("> ```\n> ab\tc\n> ```\n", None), "│ ```\n│ ab  c\n│ ```\n");
    assert_eq!(plain("- ```\n  ab\tc\n  ```\n", None), "• ```\n  ab  c\n  ```\n");
}

#[test]
fn code_block_keeps_interior_blank_lines() {
    assert_eq!(plain("```\na\n\nb\n```\n", None), "```\na\n\nb\n```\n");
}

#[test]
fn html_block_renders_verbatim_without_its_final_newline_or_wrapping() {
    assert_eq!(plain("<div>\n  x y\n</div>\n", Some(3)), "<div>\n  x y\n</div>\n");
}

#[test]
fn html_block_expands_tabs_to_four_column_stops() {
    assert_eq!(plain("<div>\n\tx\nab\tc\n</div>\n", None), "<div>\n    x\nab  c\n</div>\n");
}

#[test]
fn unclosed_html_block_drops_whitespace_only_trailing_lines() {
    assert_eq!(plain("<!--\nfoo\n  \n", None), "<!--\nfoo\n");
}
