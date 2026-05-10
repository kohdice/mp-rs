mod builder;
mod error;
mod source;

use std::collections::VecDeque;

use builder::AstBuilder;
use mp_ast::{Block, Document};
use pulldown_cmark::{DefaultBrokenLinkCallback, OffsetIter, Options, Parser};

pub use error::ParseError;

/// Parses a complete Markdown document into an AST.
///
/// # Errors
///
/// Returns an error if the underlying Markdown event stream is structurally inconsistent.
pub fn parse(input: &str) -> Result<Document<'_>, ParseError> {
    let mut builder = AstBuilder::new(input);
    for (event, range) in Parser::new_ext(input, parser_options()).into_offset_iter() {
        builder.push_event(event, range)?;
    }
    builder.finish()
}

/// Creates an iterator over top-level Markdown blocks.
#[must_use]
pub fn blocks(input: &str) -> Blocks<'_> {
    Blocks {
        parser: Parser::new_ext(input, parser_options()).into_offset_iter(),
        builder: Some(AstBuilder::new(input)),
        pending: VecDeque::new(),
        finished: false,
    }
}

pub struct Blocks<'a> {
    parser: OffsetIter<'a, DefaultBrokenLinkCallback>,
    builder: Option<AstBuilder<'a>>,
    pending: VecDeque<Block<'a>>,
    finished: bool,
}

impl<'a> Iterator for Blocks<'a> {
    type Item = Result<Block<'a>, ParseError>;

    fn next(&mut self) -> Option<Self::Item> {
        if let Some(block) = self.pending.pop_front() {
            return Some(Ok(block));
        }
        if self.finished {
            return None;
        }

        loop {
            if let Some((event, range)) = self.parser.next() {
                let Some(builder) = self.builder.as_mut() else {
                    self.finished = true;
                    return Some(Err(ParseError::new(
                        "markdown parser continued after finishing the document",
                    )));
                };
                if let Err(error) = builder.push_event(event, range) {
                    self.finished = true;
                    return Some(Err(error));
                }
                match builder.drain_document_blocks() {
                    Ok(blocks) => self.pending.extend(blocks),
                    Err(error) => {
                        self.finished = true;
                        return Some(Err(error));
                    }
                }
                if let Some(block) = self.pending.pop_front() {
                    return Some(Ok(block));
                }
                continue;
            }

            self.finished = true;
            let builder = self.builder.take()?;
            return match builder.finish() {
                Ok(document) => {
                    self.pending.extend(document.blocks);
                    self.pending.pop_front().map(Ok)
                }
                Err(error) => Some(Err(error)),
            };
        }
    }
}

fn parser_options() -> Options {
    let mut options = Options::empty();
    options.insert(Options::ENABLE_STRIKETHROUGH);
    options.insert(Options::ENABLE_TABLES);
    options.insert(Options::ENABLE_TASKLISTS);
    options.insert(Options::ENABLE_GFM);
    options
}

#[cfg(test)]
mod tests {
    use mp_ast::{
        Alignment, Block, BlockQuote, BlockQuoteKind, CodeBlock, Heading, Inline, LinkKind,
        ListKind, TaskState, Text,
    };

    use super::*;

    #[test]
    fn converts_empty_markdown_input_into_an_empty_document() -> Result<(), ParseError> {
        let document = parse("")?;

        assert!(document.blocks.is_empty());
        Ok(())
    }

    #[test]
    fn converts_a_paragraph_and_atx_heading_into_ast_blocks() -> Result<(), ParseError> {
        let document = parse("# Title\n\nHello, world!")?;

        assert_eq!(
            document.blocks,
            vec![
                Block::Heading(Heading {
                    level: 1,
                    children: vec![Inline::Text(Text::borrowed("Title"))],
                }),
                Block::BlankLine,
                Block::Paragraph(vec![Inline::Text(Text::borrowed("Hello, world!"))]),
            ],
        );
        Ok(())
    }

    #[test]
    fn preserves_fenced_code_block_info_and_body_content() -> Result<(), ParseError> {
        let document = parse("```mermaid\ngraph TD;\nA-->B;\n```\n")?;

        assert_eq!(
            document.blocks,
            vec![Block::CodeBlock(CodeBlock {
                info: Some(Text::borrowed("mermaid")),
                text: Text::borrowed("graph TD;\nA-->B;\n"),
            })],
        );
        Ok(())
    }

    #[test]
    fn converts_unordered_ordered_and_task_lists_into_list_ast_nodes() -> Result<(), ParseError> {
        let document = parse("- plain\n- [x] done\n- [ ] todo\n\n3. ordered\n")?;

        assert_eq!(document.blocks.len(), 3);
        let Block::List(unordered) = &document.blocks[0] else {
            panic!("expected unordered list");
        };
        assert_eq!(unordered.kind, ListKind::Unordered);
        assert_eq!(unordered.items.len(), 3);
        assert_eq!(unordered.items[0].task, None);
        assert_eq!(unordered.items[1].task, Some(TaskState::Checked));
        assert_eq!(unordered.items[2].task, Some(TaskState::Unchecked));

        assert_eq!(document.blocks[1], Block::BlankLine);

        let Block::List(ordered) = &document.blocks[2] else {
            panic!("expected ordered list");
        };
        assert_eq!(ordered.kind, ListKind::Ordered { start: 3 });
        assert_eq!(ordered.items.len(), 1);
        Ok(())
    }

    #[test]
    fn converts_blockquotes_into_nested_block_ast_nodes() -> Result<(), ParseError> {
        let document = parse("> quoted\n>\n> - item\n")?;

        assert_eq!(document.blocks.len(), 1);
        let Block::BlockQuote(blockquote) = &document.blocks[0] else {
            panic!("expected blockquote");
        };
        assert_eq!(blockquote.kind, None);
        let blocks = &blockquote.blocks;
        assert_eq!(blocks.len(), 3);
        assert_eq!(blocks[0], Block::Paragraph(vec![Inline::Text(Text::borrowed("quoted"))]),);
        assert_eq!(blocks[1], Block::BlankLine);
        assert!(matches!(blocks[2], Block::List(_)));
        Ok(())
    }

    #[test]
    fn converts_tables_into_table_ast_nodes_with_header_rows_and_alignments()
    -> Result<(), ParseError> {
        let document = parse("| Name | Count |\n| :--- | ---: |\n| mp | 1 |\n")?;

        assert_eq!(document.blocks.len(), 1);
        let Block::Table(table) = &document.blocks[0] else {
            panic!("expected table");
        };
        assert_eq!(table.alignments, vec![Alignment::Left, Alignment::Right]);
        assert_eq!(
            table.header,
            vec![
                vec![Inline::Text(Text::borrowed("Name"))],
                vec![Inline::Text(Text::borrowed("Count"))],
            ],
        );
        assert_eq!(
            table.rows,
            vec![vec![
                vec![Inline::Text(Text::borrowed("mp"))],
                vec![Inline::Text(Text::borrowed("1"))],
            ]],
        );
        Ok(())
    }

    #[test]
    fn converts_inline_markdown_into_inline_ast_nodes() -> Result<(), ParseError> {
        let document = parse(
            "text *em* **strong** ~~strike~~ `code` [link](https://example.com \"Title\") \
             ![alt](image.png \"Image\")  \nhard\nsoft",
        )?;

        assert_eq!(document.blocks.len(), 1);
        let Block::Paragraph(inlines) = &document.blocks[0] else {
            panic!("expected paragraph");
        };
        assert!(inlines.contains(&Inline::Text(Text::borrowed("text "))));
        assert!(inlines.contains(&Inline::Emphasis(vec![Inline::Text(Text::borrowed("em"))])));
        assert!(inlines.contains(&Inline::Strong(vec![Inline::Text(Text::borrowed("strong"))])));
        assert!(
            inlines.contains(&Inline::Strikethrough(vec![Inline::Text(Text::borrowed("strike",))]))
        );
        assert!(inlines.contains(&Inline::Code(Text::borrowed("code"))));
        assert!(inlines.contains(&Inline::Link {
            destination: Text::borrowed("https://example.com"),
            title: Text::borrowed("Title"),
            kind: LinkKind::Regular,
            children: vec![Inline::Text(Text::borrowed("link"))],
        }));
        assert!(inlines.contains(&Inline::Image {
            destination: Text::borrowed("image.png"),
            title: Text::borrowed("Image"),
            alt: vec![Inline::Text(Text::borrowed("alt"))],
        }));
        assert!(inlines.contains(&Inline::HardBreak));
        assert!(inlines.contains(&Inline::SoftBreak));
        Ok(())
    }

    #[test]
    fn streams_top_level_blocks_without_buffering_the_complete_document() -> Result<(), ParseError>
    {
        let blocks = blocks("# Title\n\nHello\n").collect::<Result<Vec<_>, _>>()?;

        assert_eq!(
            blocks,
            vec![
                Block::Heading(Heading {
                    level: 1,
                    children: vec![Inline::Text(Text::borrowed("Title"))],
                }),
                Block::BlankLine,
                Block::Paragraph(vec![Inline::Text(Text::borrowed("Hello"))]),
            ],
        );
        Ok(())
    }

    #[test]
    fn parses_unclosed_fenced_code_blocks_as_code_blocks() -> Result<(), ParseError> {
        let document = parse("```\nabc\n")?;

        assert_eq!(
            document.blocks,
            vec![Block::CodeBlock(CodeBlock { info: None, text: Text::borrowed("abc\n") })],
        );
        Ok(())
    }

    #[test]
    fn preserves_html_blocks_as_raw_markdown_content() -> Result<(), ParseError> {
        let document = parse("<div>\nhello\n</div>\n")?;

        assert_eq!(
            document.blocks,
            vec![Block::HtmlBlock(Text::borrowed("<div>\nhello\n</div>\n"))]
        );
        Ok(())
    }

    #[test]
    fn preserves_gfm_blockquote_kind_without_dropping_the_marker_semantics()
    -> Result<(), ParseError> {
        let document = parse("> [!NOTE]\n> Read this\n")?;

        assert_eq!(
            document.blocks,
            vec![Block::BlockQuote(BlockQuote {
                kind: Some(BlockQuoteKind::Note),
                blocks: vec![Block::Paragraph(vec![Inline::Text(Text::borrowed("Read this"))])],
            })],
        );
        Ok(())
    }
}
