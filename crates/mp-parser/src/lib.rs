//! Markdown-to-block parsing.
//!
//! Wraps pulldown-cmark's event stream and assembles it into [`mp_ast`] blocks, exposed
//! as the streaming [`blocks`] iterator so callers can render without buffering the
//! whole document.

mod builder;
mod error;
mod source;

use std::collections::VecDeque;

use builder::AstBuilder;
use mp_ast::Block;
use pulldown_cmark::{DefaultBrokenLinkCallback, OffsetIter, Options, Parser};

pub use error::ParseError;

/// Creates an iterator over top-level Markdown blocks.
///
/// This is the primary entry point: it streams the document one top-level [`Block`] at a
/// time without buffering the whole AST.
///
/// # Errors
///
/// Each yielded item is an error if the underlying Markdown event stream is structurally
/// inconsistent.
#[must_use]
pub fn blocks(input: &str) -> Blocks<'_> {
    Blocks {
        parser: Parser::new_ext(input, parser_options()).into_offset_iter(),
        builder: Some(AstBuilder::new(input)),
        pending: VecDeque::new(),
        finished: false,
    }
}

/// Streaming iterator over top-level Markdown blocks, created by [`blocks`].
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
                Ok(blocks) => {
                    self.pending.extend(blocks);
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
        Alignment, Block, BlockQuote, BlockQuoteKind, CodeBlock, Heading, HeadingLevel, Inline,
        LinkKind, ListKind, TaskState, Text,
    };

    use super::*;

    #[test]
    fn converts_empty_markdown_input_into_an_empty_document() -> Result<(), ParseError> {
        let blocks = blocks("").collect::<Result<Vec<_>, _>>()?;

        assert!(blocks.is_empty());
        Ok(())
    }

    #[test]
    fn converts_a_paragraph_and_atx_heading_into_ast_blocks() -> Result<(), ParseError> {
        let blocks = blocks("# Title\n\nHello, world!").collect::<Result<Vec<_>, _>>()?;

        assert_eq!(
            blocks,
            vec![
                Block::Heading(Heading {
                    level: HeadingLevel::H1,
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
        let blocks =
            blocks("```mermaid\ngraph TD;\nA-->B;\n```\n").collect::<Result<Vec<_>, _>>()?;

        assert_eq!(
            blocks,
            vec![Block::CodeBlock(CodeBlock {
                info: Some(Text::borrowed("mermaid")),
                text: Text::borrowed("graph TD;\nA-->B;\n"),
            })],
        );
        Ok(())
    }

    #[test]
    fn converts_unordered_ordered_and_task_lists_into_list_ast_nodes() -> Result<(), ParseError> {
        let blocks = blocks("- plain\n- [x] done\n- [ ] todo\n\n3. ordered\n")
            .collect::<Result<Vec<_>, _>>()?;

        assert_eq!(blocks.len(), 3);
        let Block::List(unordered) = &blocks[0] else {
            panic!("expected unordered list");
        };
        assert_eq!(unordered.kind, ListKind::Unordered);
        assert_eq!(unordered.items.len(), 3);
        assert_eq!(unordered.items[0].task, None);
        assert_eq!(unordered.items[1].task, Some(TaskState::Checked));
        assert_eq!(unordered.items[2].task, Some(TaskState::Unchecked));

        assert_eq!(blocks[1], Block::BlankLine);

        let Block::List(ordered) = &blocks[2] else {
            panic!("expected ordered list");
        };
        assert_eq!(ordered.kind, ListKind::Ordered { start: 3 });
        assert_eq!(ordered.items.len(), 1);
        Ok(())
    }

    #[test]
    fn marks_a_list_with_a_blank_line_between_items_as_loose() -> Result<(), ParseError> {
        let blocks = blocks("- a\n\n- b\n").collect::<Result<Vec<_>, _>>()?;

        let Block::List(list) = &blocks[0] else {
            panic!("expected list");
        };
        assert!(list.loose);
        Ok(())
    }

    #[test]
    fn marks_a_list_without_blank_separation_as_tight() -> Result<(), ParseError> {
        let blocks = blocks("- a\n- b\n").collect::<Result<Vec<_>, _>>()?;

        let Block::List(list) = &blocks[0] else {
            panic!("expected list");
        };
        assert!(!list.loose);
        Ok(())
    }

    #[test]
    fn marks_a_list_with_a_blank_gap_between_blocks_inside_an_item_as_loose()
    -> Result<(), ParseError> {
        let blocks = blocks("- a\n\n  b\n- c\n").collect::<Result<Vec<_>, _>>()?;

        let Block::List(list) = &blocks[0] else {
            panic!("expected list");
        };
        assert!(list.loose);
        Ok(())
    }

    #[test]
    fn converts_blockquotes_into_nested_block_ast_nodes() -> Result<(), ParseError> {
        let blocks = blocks("> quoted\n>\n> - item\n").collect::<Result<Vec<_>, _>>()?;

        assert_eq!(blocks.len(), 1);
        let Block::BlockQuote(blockquote) = &blocks[0] else {
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
    fn blockquote_blank_line_separation_matches_top_level_separation() -> Result<(), ParseError> {
        let top_level = blocks("a\n\nb\n").collect::<Result<Vec<_>, _>>()?;
        let quoted = blocks("> a\n>\n> b\n").collect::<Result<Vec<_>, _>>()?;

        assert_eq!(
            top_level,
            vec![
                Block::Paragraph(vec![Inline::Text(Text::borrowed("a"))]),
                Block::BlankLine,
                Block::Paragraph(vec![Inline::Text(Text::borrowed("b"))]),
            ],
        );
        let Block::BlockQuote(blockquote) = &quoted[0] else {
            panic!("expected blockquote");
        };
        assert_eq!(blockquote.blocks, top_level);
        Ok(())
    }

    #[test]
    fn keeps_blank_line_before_an_indented_paragraph_continuation() -> Result<(), ParseError> {
        let blocks = blocks("a\n\n  b\n").collect::<Result<Vec<_>, _>>()?;

        assert_eq!(
            blocks,
            vec![
                Block::Paragraph(vec![Inline::Text(Text::borrowed("a"))]),
                Block::BlankLine,
                Block::Paragraph(vec![Inline::Text(Text::borrowed("b"))]),
            ],
        );
        Ok(())
    }

    #[test]
    fn converts_tables_into_table_ast_nodes_with_header_rows_and_alignments()
    -> Result<(), ParseError> {
        let blocks = blocks("| Name | Count |\n| :--- | ---: |\n| mp | 1 |\n")
            .collect::<Result<Vec<_>, _>>()?;

        assert_eq!(blocks.len(), 1);
        let Block::Table(table) = &blocks[0] else {
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
    fn normalizes_ragged_table_rows_to_the_header_width() -> Result<(), ParseError> {
        let blocks = blocks("| a | b |\n| - | - |\n| 1 |\n| 2 | 3 | 4 |\n")
            .collect::<Result<Vec<_>, _>>()?;

        assert_eq!(blocks.len(), 1);
        let Block::Table(table) = &blocks[0] else {
            panic!("expected table");
        };
        assert_eq!(table.header.len(), 2);
        assert_eq!(
            table.rows,
            vec![
                vec![vec![Inline::Text(Text::borrowed("1"))], vec![]],
                vec![
                    vec![Inline::Text(Text::borrowed("2"))],
                    vec![Inline::Text(Text::borrowed("3"))],
                ],
            ],
            "short rows gain empty cells and long rows drop extra cells (GFM)",
        );
        Ok(())
    }

    #[test]
    fn converts_inline_markdown_into_inline_ast_nodes() -> Result<(), ParseError> {
        let blocks = blocks(
            "text *em* **strong** ~~strike~~ `code` [link](https://example.com \"Title\") \
             ![alt](image.png \"Image\")  \nhard\nsoft",
        )
        .collect::<Result<Vec<_>, _>>()?;

        assert_eq!(blocks.len(), 1);
        let Block::Paragraph(inlines) = &blocks[0] else {
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
            title: Some(Text::borrowed("Title")),
            kind: LinkKind::Regular,
            children: vec![Inline::Text(Text::borrowed("link"))],
        }));
        assert!(inlines.contains(&Inline::Image {
            destination: Text::borrowed("image.png"),
            title: Some(Text::borrowed("Image")),
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
                    level: HeadingLevel::H1,
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
        let blocks = blocks("```\nabc\n").collect::<Result<Vec<_>, _>>()?;

        assert_eq!(
            blocks,
            vec![Block::CodeBlock(CodeBlock { info: None, text: Text::borrowed("abc\n") })],
        );
        Ok(())
    }

    #[test]
    fn preserves_html_blocks_as_raw_markdown_content() -> Result<(), ParseError> {
        let blocks = blocks("<div>\nhello\n</div>\n").collect::<Result<Vec<_>, _>>()?;

        assert_eq!(blocks, vec![Block::HtmlBlock(Text::borrowed("<div>\nhello\n</div>\n"))]);
        Ok(())
    }

    #[test]
    fn preserves_gfm_blockquote_kind_without_dropping_the_marker_semantics()
    -> Result<(), ParseError> {
        let blocks = blocks("> [!NOTE]\n> Read this\n").collect::<Result<Vec<_>, _>>()?;

        assert_eq!(
            blocks,
            vec![Block::BlockQuote(BlockQuote {
                kind: Some(BlockQuoteKind::Note),
                blocks: vec![Block::Paragraph(vec![Inline::Text(Text::borrowed("Read this"))])],
            })],
        );
        Ok(())
    }
}
