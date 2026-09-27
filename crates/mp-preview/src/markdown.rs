//! Converts Markdown into the owned [`Block`] model. The only module that names comrak.

use comrak::nodes::{AlertType, ListType, NodeValue, TableAlignment};
use comrak::{Arena, Node, Options, parse_document};

use crate::model::{Align, Block, Inline, ListItem};

/// Parses a whole document. The comrak arena is dropped before returning, so callers
/// never hold a comrak type.
pub(crate) fn parse(markdown: &str) -> Vec<Block> {
    let arena = Arena::new();
    let root = parse_document(&arena, markdown, &comrak_options());
    convert_blocks(root)
}

fn convert_blocks(parent: Node<'_>) -> Vec<Block> {
    parent.children().filter_map(convert_block).collect()
}

fn convert_block(node: Node<'_>) -> Option<Block> {
    match &node.data().value {
        NodeValue::Paragraph => Some(Block::Paragraph(convert_inlines(node))),
        NodeValue::Heading(heading) => {
            Some(Block::Heading { level: heading.level, inlines: convert_inlines(node) })
        }
        NodeValue::BlockQuote => Some(Block::BlockQuote(convert_blocks(node))),
        NodeValue::Alert(alert) => {
            let title = match &alert.title {
                Some(title) => visualize_controls(title),
                None => alert_label(alert.alert_type).to_owned(),
            };
            let label = Block::Paragraph(vec![Inline::Strong(vec![Inline::Text(title)])]);
            Some(Block::BlockQuote(
                std::iter::once(label).chain(node.children().filter_map(convert_block)).collect(),
            ))
        }
        NodeValue::CodeBlock(code) => Some(Block::CodeBlock {
            info: visualize_controls(&code.info),
            code: visualize_controls(&normalize_line_endings(&code.literal)),
        }),
        NodeValue::HtmlBlock(html) => {
            Some(Block::Html(visualize_controls(&normalize_line_endings(&html.literal))))
        }
        NodeValue::ThematicBreak => Some(Block::ThematicBreak),
        NodeValue::List(list) => Some(Block::List {
            start: match list.list_type {
                ListType::Ordered => Some(list.start as u64),
                ListType::Bullet => None,
            },
            tight: list.tight,
            items: node.children().map(convert_item).collect(),
        }),
        NodeValue::Table(table) => Some(convert_table(node, &table.alignments)),
        _ => None,
    }
}

fn convert_table(node: Node<'_>, alignments: &[TableAlignment]) -> Block {
    // A GFM table always starts with its header row.
    let mut rows = node.children().map(|row| row.children().map(convert_inlines).collect());
    let header = rows.next().unwrap_or_default();
    Block::Table {
        align: alignments.iter().map(convert_align).collect(),
        header,
        rows: rows.collect(),
    }
}

fn convert_align(alignment: &TableAlignment) -> Align {
    match alignment {
        TableAlignment::None => Align::None,
        TableAlignment::Left => Align::Left,
        TableAlignment::Center => Align::Center,
        TableAlignment::Right => Align::Right,
    }
}

fn alert_label(alert_type: AlertType) -> &'static str {
    match alert_type {
        AlertType::Note => "NOTE",
        AlertType::Tip => "TIP",
        AlertType::Important => "IMPORTANT",
        AlertType::Warning => "WARNING",
        AlertType::Caution => "CAUTION",
    }
}

fn convert_item(node: Node<'_>) -> ListItem {
    let task = match &node.data().value {
        NodeValue::TaskItem(task) => Some(task.symbol.is_some()),
        _ => None,
    };
    ListItem { task, blocks: convert_blocks(node) }
}

fn convert_inlines(parent: Node<'_>) -> Vec<Inline> {
    parent.children().filter_map(convert_inline).collect()
}

fn convert_inline(node: Node<'_>) -> Option<Inline> {
    match &node.data().value {
        NodeValue::Text(text) => Some(Inline::Text(visualize_controls(text))),
        NodeValue::HtmlInline(html) => {
            // A tag may span a line ending, where it is only whitespace; a newline kept
            // in the text would break the line outside layout's prefixes and styles.
            Some(Inline::Text(visualize_controls(&normalize_line_endings(html).replace('\n', " "))))
        }
        NodeValue::Code(code) => Some(Inline::Code(visualize_controls(&code.literal))),
        NodeValue::Emph => Some(Inline::Emphasis(convert_inlines(node))),
        NodeValue::Strong => Some(Inline::Strong(convert_inlines(node))),
        NodeValue::Strikethrough => Some(Inline::Strikethrough(convert_inlines(node))),
        NodeValue::SoftBreak => Some(Inline::SoftBreak),
        NodeValue::LineBreak => Some(Inline::HardBreak),
        NodeValue::Link(link) => {
            let children = convert_inlines(node);
            let url = visualize_controls(&link.url);
            Some(Inline::Link {
                show_url: shows_url(&children, &url),
                url,
                title: convert_title(&link.title),
                children,
            })
        }
        NodeValue::Image(image) => Some(Inline::Image {
            url: visualize_controls(&image.url),
            title: convert_title(&image.title),
            alt: convert_inlines(node),
        }),
        _ => None,
    }
}

fn convert_title(title: &str) -> Option<String> {
    (!title.is_empty()).then(|| visualize_controls(title))
}

fn shows_url(children: &[Inline], url: &str) -> bool {
    let [Inline::Text(visible)] = children else {
        return true;
    };
    visible != url && Some(visible.as_str()) != url.strip_prefix("mailto:")
}

/// Rewrites CRLF and lone CR line endings as LF. comrak keeps the source line
/// endings in code, HTML block, and inline HTML literals, and a CR left there
/// would otherwise be shown as a Control Picture at the end of every line.
fn normalize_line_endings(text: &str) -> String {
    text.replace("\r\n", "\n").replace('\r', "\n")
}

/// Replaces C0 controls other than LF and HT, and DEL, with their Unicode Control
/// Pictures so decoded input cannot drive the terminal.
fn visualize_controls(text: &str) -> String {
    let mut visible = String::with_capacity(text.len());
    for character in text.chars() {
        visible.push(match character {
            '\n' | '\t' => character,
            '\0'..='\x1f' => char::from_u32(0x2400 + u32::from(character)).unwrap_or(character),
            '\x7f' => '\u{2421}',
            _ => character,
        });
    }
    visible
}

fn comrak_options() -> Options<'static> {
    let mut options = Options::default();
    options.extension.table = true;
    options.extension.strikethrough = true;
    options.extension.tasklist = true;
    options.extension.autolink = true;
    options.extension.alerts = true;
    options
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn parse_returns_no_blocks_for_empty_input() {
        assert_eq!(parse(""), Vec::new());
        assert_eq!(parse("\n\n"), Vec::new());
    }

    fn text(value: &str) -> Inline {
        Inline::Text(value.to_owned())
    }

    #[test]
    fn parse_converts_a_single_paragraph() {
        assert_eq!(parse("Hello\n"), vec![Block::Paragraph(vec![text("Hello")])]);
    }

    #[test]
    fn parse_nests_emphasis_strong_strikethrough_and_code() {
        assert_eq!(
            parse("*a **b** ~~c~~ `d`*\n"),
            vec![Block::Paragraph(vec![Inline::Emphasis(vec![
                text("a "),
                Inline::Strong(vec![text("b")]),
                text(" "),
                Inline::Strikethrough(vec![text("c")]),
                text(" "),
                Inline::Code("d".to_owned()),
            ])])]
        );
    }

    /// Parses a single paragraph made only of text inlines and joins them, since comrak
    /// may split inline HTML into several text nodes.
    fn paragraph_text(markdown: &str) -> String {
        let blocks = parse(markdown);
        let [Block::Paragraph(inlines)] = blocks.as_slice() else {
            panic!("expected one paragraph, got {blocks:?}");
        };
        let mut joined = String::new();
        for inline in inlines {
            let Inline::Text(value) = inline else {
                panic!("expected only text inlines, got {inline:?}");
            };
            joined.push_str(value);
        }
        joined
    }

    #[test]
    fn parse_keeps_inline_html_as_text() {
        assert_eq!(paragraph_text("a <b>x</b> c"), "a <b>x</b> c");
    }

    #[test]
    fn parse_visualizes_control_characters_in_inline_html() {
        assert_eq!(paragraph_text("a <b x=\"\u{1b}\"> c"), "a <b x=\"␛\"> c");
    }

    #[test]
    fn parse_merges_numeric_references_into_one_text() {
        assert_eq!(
            parse("&#x1F469;&#x200D;&#x1F4BB;\n"),
            vec![Block::Paragraph(vec![text("👩\u{200d}💻")])]
        );
    }

    #[test]
    fn parse_records_heading_levels() {
        assert_eq!(parse("## a\n"), vec![Block::Heading { level: 2, inlines: vec![text("a")] }]);
    }

    #[test]
    fn parse_converts_hard_breaks() {
        assert_eq!(
            parse("a  \nb\n"),
            vec![Block::Paragraph(vec![text("a"), Inline::HardBreak, text("b")])]
        );
    }

    fn paragraph(value: &str) -> Block {
        Block::Paragraph(vec![text(value)])
    }

    fn item(blocks: Vec<Block>) -> ListItem {
        ListItem { task: None, blocks }
    }

    fn bullet_list(tight: bool, items: Vec<ListItem>) -> Block {
        Block::List { start: None, tight, items }
    }

    #[test]
    fn parse_marks_simple_lists_tight_and_blank_separated_lists_loose() {
        let items = || vec![item(vec![paragraph("a")]), item(vec![paragraph("b")])];

        assert_eq!(parse("- a\n- b\n"), vec![bullet_list(true, items())]);
        assert_eq!(parse("- a\n\n- b\n"), vec![bullet_list(false, items())]);
    }

    #[test]
    fn parse_marks_a_single_item_with_separated_paragraphs_loose() {
        assert_eq!(
            parse("- a\n\n  b\n"),
            vec![bullet_list(false, vec![item(vec![paragraph("a"), paragraph("b")])])]
        );
    }

    #[test]
    fn parse_marks_heading_only_and_quote_only_lists_loose() {
        let heading = |value: &str| Block::Heading { level: 1, inlines: vec![text(value)] };
        let quote = |value: &str| Block::BlockQuote(vec![paragraph(value)]);

        assert_eq!(
            parse("- # a\n\n- # b\n"),
            vec![bullet_list(false, vec![item(vec![heading("a")]), item(vec![heading("b")])])]
        );
        assert_eq!(
            parse("- > a\n\n- > b\n"),
            vec![bullet_list(false, vec![item(vec![quote("a")]), item(vec![quote("b")])])]
        );
    }

    #[test]
    fn parse_keeps_lists_with_lazy_continuation_and_nested_list_tight() {
        let first = item(vec![
            Block::Paragraph(vec![text("a"), Inline::SoftBreak, text("b")]),
            bullet_list(true, vec![item(vec![paragraph("c")])]),
        ]);

        assert_eq!(
            parse("- a\n  b\n  - c\n- d\n"),
            vec![bullet_list(true, vec![first, item(vec![paragraph("d")])])]
        );
    }

    #[test]
    fn parse_marks_a_quoted_blank_separated_list_loose() {
        assert_eq!(
            parse("> - a\n>\n> - b\n"),
            vec![Block::BlockQuote(vec![bullet_list(
                false,
                vec![item(vec![paragraph("a")]), item(vec![paragraph("b")])]
            )])]
        );
    }

    #[test]
    fn parse_keeps_paragraphs_of_a_nested_quote_separate() {
        assert_eq!(
            parse("> > a\n> >\n> > b\n"),
            vec![Block::BlockQuote(vec![Block::BlockQuote(vec![paragraph("a"), paragraph("b")])])]
        );
    }

    #[test]
    fn parse_records_ordered_list_start_and_unordered_as_none() {
        assert_eq!(
            parse("3. a\n4. b\n"),
            vec![Block::List {
                start: Some(3),
                tight: true,
                items: vec![item(vec![paragraph("a")]), item(vec![paragraph("b")])],
            }]
        );
        assert_eq!(parse("- a\n"), vec![bullet_list(true, vec![item(vec![paragraph("a")])])]);
    }

    #[test]
    fn parse_records_task_list_states() {
        let task = |state, value| ListItem { task: state, blocks: vec![paragraph(value)] };

        assert_eq!(
            parse("- [ ] a\n- [x] b\n- c\n"),
            vec![bullet_list(
                true,
                vec![task(Some(false), "a"), task(Some(true), "b"), task(None, "c")]
            )]
        );
    }

    #[test]
    fn parse_keeps_code_block_info_code_and_tabs() {
        let code_block = |info: &str, code: &str| Block::CodeBlock {
            info: info.to_owned(),
            code: code.to_owned(),
        };

        assert_eq!(parse("```rust\n\tx\n```\n"), vec![code_block("rust", "\tx\n")]);
        assert_eq!(parse("    x\n"), vec![code_block("", "x\n")]);
    }

    #[test]
    fn parse_keeps_html_blocks_and_thematic_breaks() {
        assert_eq!(
            parse("<div>\n\n---\n"),
            vec![Block::Html("<div>\n".to_owned()), Block::ThematicBreak]
        );
    }

    #[test]
    fn parse_converts_tables_with_alignment() {
        let cells = |values: &[&str]| values.iter().map(|value| vec![text(value)]).collect();

        assert_eq!(
            parse("| a | b | c |\n| :-- | :-: | --: |\n| 1 | 2 | 3 |\n| 4 | 5 | 6 |\n"),
            vec![Block::Table {
                align: vec![Align::Left, Align::Center, Align::Right],
                header: cells(&["a", "b", "c"]),
                rows: vec![cells(&["1", "2", "3"]), cells(&["4", "5", "6"])],
            }]
        );
        assert_eq!(
            parse("| a |\n| --- |\n"),
            vec![Block::Table { align: vec![Align::None], header: cells(&["a"]), rows: vec![] }]
        );
    }

    #[test]
    fn parse_shows_url_for_inline_links_with_distinct_text() {
        assert_eq!(
            parse("[text](https://x.test \"T\")\n"),
            vec![Block::Paragraph(vec![Inline::Link {
                url: "https://x.test".to_owned(),
                title: Some("T".to_owned()),
                children: vec![text("text")],
                show_url: true,
            }])]
        );
    }

    fn untitled_link(url: &str, visible: &str, show_url: bool) -> Block {
        Block::Paragraph(vec![Inline::Link {
            url: url.to_owned(),
            title: None,
            children: vec![text(visible)],
            show_url,
        }])
    }

    #[test]
    fn parse_hides_url_when_link_text_equals_destination() {
        let expected = vec![untitled_link("https://x.test", "https://x.test", false)];

        assert_eq!(parse("<https://x.test>\n"), expected);
        assert_eq!(parse("https://x.test\n"), expected);
        assert_eq!(parse("[https://x.test][r]\n\n[r]: https://x.test\n"), expected);
    }

    #[test]
    fn parse_hides_mailto_prefix_for_email_autolinks() {
        let expected = vec![untitled_link("mailto:a@b.test", "a@b.test", false)];

        assert_eq!(parse("<a@b.test>\n"), expected);
        assert_eq!(parse("a@b.test\n"), expected);
    }

    #[test]
    fn parse_shows_generated_destination_for_www_autolinks() {
        assert_eq!(
            parse("www.example.com\n"),
            vec![untitled_link("http://www.example.com", "www.example.com", true)]
        );
    }

    #[test]
    fn parse_converts_images_with_alt_and_title() {
        assert_eq!(
            parse("![alt *x*](i.png \"t\")\n"),
            vec![Block::Paragraph(vec![Inline::Image {
                url: "i.png".to_owned(),
                title: Some("t".to_owned()),
                alt: vec![text("alt "), Inline::Emphasis(vec![text("x")])],
            }])]
        );
    }

    #[test]
    fn parse_visualizes_control_characters_in_image_urls() {
        assert_eq!(
            parse("![a](i&#1;.png)"),
            vec![Block::Paragraph(vec![Inline::Image {
                url: "i␁.png".to_owned(),
                title: None,
                alt: vec![text("a")],
            }])]
        );
    }

    #[test]
    fn parse_converts_alerts_to_quotes_with_bold_labels() {
        for kind in ["NOTE", "TIP", "IMPORTANT", "WARNING", "CAUTION"] {
            assert_eq!(
                parse(&format!("> [!{kind}]\n> body\n")),
                vec![Block::BlockQuote(vec![
                    Block::Paragraph(vec![Inline::Strong(vec![text(kind)])]),
                    paragraph("body"),
                ])],
                "alert kind {kind}"
            );
        }
    }

    #[test]
    fn parse_uses_custom_alert_titles_in_place_of_the_label() {
        assert_eq!(
            parse("> [!NOTE] Custom &#27;title\n> body\n"),
            vec![Block::BlockQuote(vec![
                Block::Paragraph(vec![Inline::Strong(vec![text("Custom ␛title")])]),
                paragraph("body"),
            ])]
        );
    }

    #[test]
    fn parse_visualizes_decoded_control_characters_in_text() {
        assert_eq!(parse("&#27;[2J&#1;x&#127;\n"), vec![paragraph("␛[2J␁x␡")]);
    }

    #[test]
    fn parse_visualizes_control_characters_in_code_html_urls_and_titles() {
        assert_eq!(
            parse("`a\u{1b}b`\n"),
            vec![Block::Paragraph(vec![Inline::Code("a␛b".to_owned())])]
        );
        assert_eq!(
            parse("```\n\tx\u{7f}\ny\n```\n"),
            vec![Block::CodeBlock { info: String::new(), code: "\tx␡\ny\n".to_owned() }]
        );
        assert_eq!(parse("<div>\u{1b}</div>\n"), vec![Block::Html("<div>␛</div>\n".to_owned())]);
        assert_eq!(
            parse("[t](a&#1;b \"c&#27;d\")\n"),
            vec![Block::Paragraph(vec![Inline::Link {
                url: "a␁b".to_owned(),
                title: Some("c␛d".to_owned()),
                children: vec![text("t")],
                show_url: true,
            }])]
        );
    }

    #[test]
    fn parse_normalizes_crlf_and_cr_line_endings_in_code_and_html_blocks() {
        let code = || Block::CodeBlock { info: "rust".to_owned(), code: "a\nb\n".to_owned() };
        assert_eq!(parse("```rust\r\na\r\nb\r\n```\r\n"), vec![code()]);
        assert_eq!(parse("```rust\ra\rb\r```\r"), vec![code()]);
        assert_eq!(
            parse("<div>\r\nx\r\n</div>\r\n"),
            vec![Block::Html("<div>\nx\n</div>\n".to_owned())]
        );
        assert_eq!(parse("a&#13;b\n"), vec![Block::Paragraph(vec![text("a␍b")])]);
    }

    #[test]
    fn parse_visualizes_control_characters_in_code_block_info_strings() {
        assert_eq!(
            parse("```&#27;x\ny\n```\n"),
            vec![Block::CodeBlock { info: "\u{241b}x".to_owned(), code: "y\n".to_owned() }]
        );
    }
}
