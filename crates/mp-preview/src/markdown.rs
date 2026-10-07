//! Converts Markdown into the owned [`Block`] model. The only module that names comrak.

use comrak::nodes::{AlertType, ListType, NodeValue, TableAlignment};
use comrak::{Arena, Node, Options, parse_document};

use crate::control::visualize_control;
use crate::model::{AlertKind, Align, Block, Inline, ListItem};

/// Parses a whole document into blocks; empty or whitespace-only input gives no blocks,
/// and parsing never fails. The comrak arena is dropped before returning, so callers
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
            let kind = convert_alert_kind(alert.alert_type);
            let title = match &alert.title {
                Some(title) => sanitize_inline(title),
                None => kind.label().to_owned(),
            };
            Some(Block::Alert { kind, title, blocks: convert_blocks(node) })
        }
        NodeValue::CodeBlock(code) => Some(Block::CodeBlock {
            info: sanitize_inline(&code.info),
            code: sanitize_block_body(&code.literal),
        }),
        NodeValue::HtmlBlock(html) => Some(Block::Html(sanitize_block_body(&html.literal))),
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

fn convert_alert_kind(alert_type: AlertType) -> AlertKind {
    match alert_type {
        AlertType::Note => AlertKind::Note,
        AlertType::Tip => AlertKind::Tip,
        AlertType::Important => AlertKind::Important,
        AlertType::Warning => AlertKind::Warning,
        AlertType::Caution => AlertKind::Caution,
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
        NodeValue::Text(text) => Some(Inline::Text(sanitize_inline(text))),
        NodeValue::HtmlInline(html) => Some(Inline::Text(sanitize_inline(html))),
        NodeValue::Code(code) => Some(Inline::Code(sanitize_inline(&code.literal))),
        NodeValue::Emph => Some(Inline::Emphasis(convert_inlines(node))),
        NodeValue::Strong => Some(Inline::Strong(convert_inlines(node))),
        NodeValue::Strikethrough => Some(Inline::Strikethrough(convert_inlines(node))),
        NodeValue::SoftBreak => Some(Inline::SoftBreak),
        NodeValue::LineBreak => Some(Inline::HardBreak),
        NodeValue::Link(link) => {
            let children = convert_inlines(node);
            let url = sanitize_inline(&link.url);
            Some(Inline::Link {
                show_url: shows_url(&children, &url),
                url,
                title: convert_title(&link.title),
                children,
            })
        }
        NodeValue::Image(image) => Some(Inline::Image {
            url: sanitize_inline(&image.url),
            title: convert_title(&image.title),
            alt: convert_inlines(node),
        }),
        _ => None,
    }
}

fn convert_title(title: &str) -> Option<String> {
    (!title.is_empty()).then(|| sanitize_inline(title))
}

fn shows_url(children: &[Inline], url: &str) -> bool {
    let [Inline::Text(visible)] = children else {
        return true;
    };
    visible != url && Some(visible.as_str()) != url.strip_prefix("mailto:")
}

/// Makes text that layout keeps on one line safe to print verbatim. Each line break
/// (LF, CR, or a CRLF pair) and each HT become one space: layout wraps words and
/// prefixes quote bars and list indents per line, so a line break or tab decoded from
/// `&#10;` or `&#9;` would escape both. Every other control is replaced as
/// [`visualize_control`] describes.
fn sanitize_inline(text: &str) -> String {
    text.replace("\r\n", "\n")
        .chars()
        .map(|character| match character {
            '\n' | '\r' | '\t' => ' ',
            _ => visualize_control(character),
        })
        .collect()
}

/// Makes a code or HTML block body safe to print verbatim while keeping its lines and
/// indentation. CRLF and lone CR become LF, LF and HT are kept, and every other control
/// is replaced as [`visualize_control`] describes. comrak keeps the source line endings
/// in these literals, so a CR left in would show as a control picture on every line.
fn sanitize_block_body(text: &str) -> String {
    text.replace("\r\n", "\n")
        .replace('\r', "\n")
        .chars()
        .map(|character| match character {
            '\n' | '\t' => character,
            _ => visualize_control(character),
        })
        .collect()
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

    fn text(value: &str) -> Inline {
        Inline::Text(value.to_owned())
    }

    /// Parses a single paragraph made only of text inlines and joins them: inline HTML
    /// is converted to its own `Inline::Text`, so it never merges with the surrounding
    /// text.
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

    fn paragraph(value: &str) -> Block {
        Block::Paragraph(vec![text(value)])
    }

    fn item(blocks: Vec<Block>) -> ListItem {
        ListItem { task: None, blocks }
    }

    fn bullet_list(tight: bool, items: Vec<ListItem>) -> Block {
        Block::List { start: None, tight, items }
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
    fn parse_returns_no_blocks_for_empty_input() {
        assert_eq!(parse(""), Vec::new());
        assert_eq!(parse("\n\n"), Vec::new());
        assert_eq!(parse("  \n\t\n"), Vec::new());
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

    #[test]
    fn parse_keeps_inline_html_as_text() {
        assert_eq!(paragraph_text("a <b>x</b> c"), "a <b>x</b> c");
    }

    #[test]
    fn parse_visualizes_control_characters_in_inline_html() {
        assert_eq!(paragraph_text("a <b x=\"\u{1b}\"> c"), "a <b x=\"␛\"> c");
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

    #[test]
    fn parse_marks_simple_lists_tight_and_blank_separated_lists_loose() {
        let items = || vec![item(vec![paragraph("a")]), item(vec![paragraph("b")])];

        assert_eq!(parse("- a\n- b\n"), vec![bullet_list(true, items())]);
        assert_eq!(parse("- a\n\n- b\n"), vec![bullet_list(false, items())]);
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
    fn parse_converts_alerts_to_alert_blocks_with_their_kind() {
        for (label, kind) in [
            ("NOTE", AlertKind::Note),
            ("TIP", AlertKind::Tip),
            ("IMPORTANT", AlertKind::Important),
            ("WARNING", AlertKind::Warning),
            ("CAUTION", AlertKind::Caution),
        ] {
            assert_eq!(
                parse(&format!("> [!{label}]\n> body\n")),
                vec![Block::Alert {
                    kind,
                    title: label.to_owned(),
                    blocks: vec![paragraph("body")],
                }],
                "alert kind {label}"
            );
        }
    }

    #[test]
    fn parse_uses_custom_alert_titles_in_place_of_the_label() {
        assert_eq!(
            parse("> [!NOTE] Custom &#27;title\n> body\n"),
            vec![Block::Alert {
                kind: AlertKind::Note,
                title: "Custom ␛title".to_owned(),
                blocks: vec![paragraph("body")],
            }]
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
    }

    #[test]
    fn parse_visualizes_c1_control_characters_in_text() {
        assert_eq!(parse("a&#155;b\u{85}c\n"), vec![paragraph("a\u{FFFD}b\u{FFFD}c")]);
    }

    #[test]
    fn parse_replaces_line_breaks_in_titles_urls_and_info_strings() {
        assert_eq!(
            parse("[t](a&#10;b \"x\ny\")\n"),
            vec![Block::Paragraph(vec![Inline::Link {
                url: "a b".to_owned(),
                title: Some("x y".to_owned()),
                children: vec![text("t")],
                show_url: true,
            }])]
        );
        assert_eq!(
            parse("```a&#10;b\nx\n```\n"),
            vec![Block::CodeBlock { info: "a b".to_owned(), code: "x\n".to_owned() }]
        );
    }

    #[test]
    fn parse_replaces_line_breaks_and_tabs_in_inline_text_with_spaces() {
        assert_eq!(parse("a&#10;b&#13;c&#9;d\n"), vec![paragraph("a b c d")]);
    }

    #[test]
    fn parse_replaces_crlf_inside_inline_html_with_one_space() {
        assert_eq!(paragraph_text("a <b\r\nx=\"y\"> c\r\n"), "a <b x=\"y\"> c");
    }
}
