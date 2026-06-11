use crate::{CodeBlock, Inline, List, Table, Text};

/// A top-level Markdown block.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Block<'a> {
    /// A paragraph of inline content.
    Paragraph(Vec<Inline<'a>>),
    /// A heading with its level and inline content.
    Heading(Heading<'a>),
    /// A block quote, optionally tagged with a GFM alert kind.
    BlockQuote(BlockQuote<'a>),
    /// An ordered or unordered list.
    List(List<'a>),
    /// A fenced or indented code block.
    CodeBlock(CodeBlock<'a>),
    /// A raw HTML block, passed through verbatim.
    HtmlBlock(Text<'a>),
    /// A GFM table.
    Table(Table<'a>),
    /// A thematic break (horizontal rule).
    ThematicBreak,
    /// A blank line between blocks; produces no visible content.
    BlankLine,
}

/// A heading and its inline content.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Heading<'a> {
    /// Heading level, from 1 (`#`) to 6 (`######`).
    pub level: u8,
    /// Inline content of the heading.
    pub children: Vec<Inline<'a>>,
}

/// A block quote and the blocks nested inside it.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct BlockQuote<'a> {
    /// GFM alert kind when the quote starts with a marker like `[!NOTE]`.
    pub kind: Option<BlockQuoteKind>,
    /// Blocks nested inside the quote.
    pub blocks: Vec<Block<'a>>,
}

/// GFM alert kinds that tag a block quote.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum BlockQuoteKind {
    /// A `[!NOTE]` alert.
    Note,
    /// A `[!TIP]` alert.
    Tip,
    /// An `[!IMPORTANT]` alert.
    Important,
    /// A `[!WARNING]` alert.
    Warning,
    /// A `[!CAUTION]` alert.
    Caution,
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::{Alignment, CodeBlock, ListItem, ListKind, Table};

    #[test]
    fn represents_block_nodes() {
        let document_blocks = [
            Block::Paragraph(vec![Inline::Text(Text::borrowed("paragraph"))]),
            Block::Heading(Heading {
                level: 2,
                children: vec![Inline::Text(Text::borrowed("heading"))],
            }),
            Block::List(List {
                kind: ListKind::Unordered,
                items: vec![ListItem {
                    task: None,
                    blocks: vec![Block::Paragraph(vec![Inline::Text(Text::borrowed("item"))])],
                }],
            }),
            Block::BlockQuote(BlockQuote {
                kind: None,
                blocks: vec![Block::Paragraph(vec![Inline::Text(Text::borrowed("quote"))])],
            }),
            Block::Table(Table {
                header: vec![vec![Inline::Text(Text::borrowed("name"))]],
                alignments: vec![Alignment::Left],
                rows: vec![vec![vec![Inline::Text(Text::borrowed("mp"))]]],
            }),
            Block::CodeBlock(CodeBlock {
                info: Some(Text::borrowed("rust")),
                text: Text::borrowed("fn main() {}\n"),
            }),
            Block::HtmlBlock(Text::borrowed("<div>raw</div>\n")),
            Block::ThematicBreak,
            Block::BlankLine,
        ];

        assert_eq!(document_blocks.len(), 9);
    }
}
