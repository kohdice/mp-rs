//! Converts Markdown into the owned [`Block`] model. The only module that names comrak.

use comrak::nodes::{AlertType, ListType, NodeValue, TableAlignment};
use comrak::{Arena, Node, Options, parse_document};

use crate::control::visualize_control;
use crate::model::{AlertKind, Align, Block, Inline, ListItem};

/// The deepest nesting of containers the model holds, counted together: block quotes,
/// alerts, and lists, then emphasis, strong, strikethrough, links, and images inside
/// them. Every later stage, and dropping the model, recurses once per level, so an
/// unbounded depth would overflow the stack. comrak bounds none of them across lines.
const MAX_NESTING_DEPTH: usize = 128;

/// Parses a whole document into blocks; empty or whitespace-only input gives no blocks,
/// and parsing never fails. The comrak arena is dropped before returning, so callers
/// never hold a comrak type. Containers nested deeper than [`MAX_NESTING_DEPTH`] are
/// flattened into the deepest allowed one.
pub(crate) fn parse(markdown: &str) -> Vec<Block> {
    let arena = Arena::new();
    let root = parse_document(&arena, markdown, &comrak_options());
    convert_blocks(root, 0)
}

/// Converts the children of a container `depth` containers deep.
fn convert_blocks(parent: Node<'_>, depth: usize) -> Vec<Block> {
    parent.children().filter_map(|child| convert_block(child, depth)).collect()
}

/// Converts the children of a container that is itself at `depth`. At the bound, the
/// leaf blocks of every container below it become its children, in document order;
/// `descendants` walks the tree without recursion.
fn convert_contents(container: Node<'_>, depth: usize) -> Vec<Block> {
    if depth < MAX_NESTING_DEPTH {
        return convert_blocks(container, depth);
    }
    container
        .descendants()
        .skip(1)
        .filter(|node| !is_container(node))
        .filter_map(|node| convert_block(node, depth))
        .collect()
}

fn is_container(node: Node<'_>) -> bool {
    matches!(
        node.data().value,
        NodeValue::BlockQuote
            | NodeValue::Alert(_)
            | NodeValue::List(_)
            | NodeValue::Item(_)
            | NodeValue::TaskItem(_)
    )
}

fn convert_block(node: Node<'_>, depth: usize) -> Option<Block> {
    match &node.data().value {
        NodeValue::Paragraph => Some(Block::Paragraph(convert_inline_contents(node, depth))),
        NodeValue::Heading(heading) => Some(Block::Heading {
            level: heading.level,
            inlines: convert_inline_contents(node, depth),
        }),
        NodeValue::BlockQuote => Some(Block::BlockQuote(convert_contents(node, depth + 1))),
        NodeValue::Alert(alert) => {
            let kind = convert_alert_kind(alert.alert_type);
            let title = match &alert.title {
                Some(title) => sanitize_inline(title),
                None => kind.label().to_owned(),
            };
            Some(Block::Alert { kind, title, blocks: convert_contents(node, depth + 1) })
        }
        NodeValue::CodeBlock(code) => Some(Block::CodeBlock {
            info: sanitize_inline(&code.info),
            code: sanitize_block_body(&code.literal),
        }),
        NodeValue::HtmlBlock(html) => Some(Block::Html(sanitize_block_body(&html.literal))),
        NodeValue::ThematicBreak => Some(Block::ThematicBreak),
        NodeValue::List(list) => Some(Block::List {
            start: match list.list_type {
                ListType::Ordered => Some(u64::try_from(list.start).unwrap_or(u64::MAX)),
                ListType::Bullet => None,
            },
            tight: list.tight,
            items: node.children().map(|item| convert_item(item, depth + 1)).collect(),
        }),
        NodeValue::Table(table) => Some(convert_table(node, &table.alignments, depth)),
        _ => None,
    }
}

fn convert_table(node: Node<'_>, alignments: &[TableAlignment], depth: usize) -> Block {
    // A GFM table always starts with its header row.
    let mut rows = node
        .children()
        .map(|row| row.children().map(|cell| convert_inline_contents(cell, depth)).collect());
    let header = rows.next().unwrap_or_default();
    Block::Table {
        align: alignments.iter().copied().map(convert_align).collect(),
        header,
        rows: rows.collect(),
    }
}

fn convert_align(alignment: TableAlignment) -> Align {
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

fn convert_item(node: Node<'_>, depth: usize) -> ListItem {
    let task = match &node.data().value {
        NodeValue::TaskItem(task) => Some(task.symbol.is_some()),
        _ => None,
    };
    ListItem { task, blocks: convert_contents(node, depth) }
}

fn convert_inlines(parent: Node<'_>, depth: usize) -> Vec<Inline> {
    parent.children().filter_map(|child| convert_inline(child, depth)).collect()
}

/// Converts the inlines of a block or inline container that is itself at `depth`, the
/// way [`convert_contents`] converts blocks: at the bound, the leaf inlines below it
/// become its children, in document order.
fn convert_inline_contents(container: Node<'_>, depth: usize) -> Vec<Inline> {
    if depth < MAX_NESTING_DEPTH {
        return convert_inlines(container, depth);
    }
    container
        .descendants()
        .skip(1)
        .filter(|node| !is_inline_container(node))
        .filter_map(|node| convert_inline(node, depth))
        .collect()
}

fn is_inline_container(node: Node<'_>) -> bool {
    matches!(
        node.data().value,
        NodeValue::Emph
            | NodeValue::Strong
            | NodeValue::Strikethrough
            | NodeValue::Link(_)
            | NodeValue::Image(_)
    )
}

fn convert_inline(node: Node<'_>, depth: usize) -> Option<Inline> {
    match &node.data().value {
        NodeValue::Text(text) => Some(Inline::Text(sanitize_inline(text))),
        NodeValue::HtmlInline(html) => Some(Inline::Text(sanitize_inline(html))),
        NodeValue::Code(code) => Some(Inline::Code(sanitize_inline(&code.literal))),
        NodeValue::Emph => Some(Inline::Emphasis(convert_inline_contents(node, depth + 1))),
        NodeValue::Strong => Some(Inline::Strong(convert_inline_contents(node, depth + 1))),
        NodeValue::Strikethrough => {
            Some(Inline::Strikethrough(convert_inline_contents(node, depth + 1)))
        }
        NodeValue::SoftBreak => Some(Inline::SoftBreak),
        NodeValue::LineBreak => Some(Inline::HardBreak),
        NodeValue::Link(link) => {
            let children = convert_inline_contents(node, depth + 1);
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
            alt: convert_inline_contents(node, depth + 1),
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
    fold_crlf(text)
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
    fold_crlf(text)
        .map(|character| match character {
            '\r' => '\n',
            '\n' | '\t' => character,
            _ => visualize_control(character),
        })
        .collect()
}

/// Yields the characters of `text` with each CRLF pair folded into one LF, so the pair
/// counts as one line break like a lone LF or CR.
fn fold_crlf(text: &str) -> impl Iterator<Item = char> {
    let mut characters = text.chars().peekable();
    std::iter::from_fn(move || {
        let character = characters.next()?;
        let crlf = character == '\r' && characters.next_if_eq(&'\n').is_some();
        Some(if crlf { '\n' } else { character })
    })
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

    #[test]
    fn parse_keeps_inline_html_as_text() {
        assert_eq!(paragraph_text("a <b>x</b> c"), "a <b>x</b> c");
    }

    #[test]
    fn parse_visualizes_control_characters_in_inline_html() {
        assert_eq!(paragraph_text("a <b x=\"\u{1b}\"> c"), "a <b x=\"␛\"> c");
    }

    #[test]
    fn shows_url_is_false_only_when_the_single_text_child_spells_the_url() {
        assert!(!shows_url(&[text("https://x.test")], "https://x.test"));
        assert!(!shows_url(&[text("a@b.test")], "mailto:a@b.test"));
        assert!(shows_url(&[text("www.example.com")], "http://www.example.com"));
        assert!(shows_url(&[text("https://"), text("x.test")], "https://x.test"));
    }

    #[test]
    fn parse_visualizes_decoded_control_characters_in_text() {
        assert_eq!(parse("&#27;[2J&#1;x&#127;\n"), vec![paragraph("␛[2J␁x␡")]);
    }

    #[test]
    fn parse_visualizes_control_characters_in_code_html_destinations_and_titles() {
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
        assert_eq!(
            parse("![a](i&#1;.png \"c&#27;d\")\n"),
            vec![Block::Paragraph(vec![Inline::Image {
                url: "i␁.png".to_owned(),
                title: Some("c␛d".to_owned()),
                alt: vec![text("a")],
            }])]
        );
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
