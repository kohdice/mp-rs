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
    /// Heading level, from [`HeadingLevel::H1`] (`#`) to [`HeadingLevel::H6`] (`######`).
    pub level: HeadingLevel,
    /// Inline content of the heading.
    pub children: Vec<Inline<'a>>,
}

/// Heading depth, restricted to CommonMark's six levels.
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord)]
pub enum HeadingLevel {
    /// Level 1 (`#`).
    H1,
    /// Level 2 (`##`).
    H2,
    /// Level 3 (`###`).
    H3,
    /// Level 4 (`####`).
    H4,
    /// Level 5 (`#####`).
    H5,
    /// Level 6 (`######`).
    H6,
}

impl HeadingLevel {
    /// The number of heading levels (CommonMark's six). Kept here next to the
    /// enum so any code sizing per-level data cannot drift from the variants.
    pub const COUNT: usize = 6;

    /// Returns the numeric depth, from 1 for [`HeadingLevel::H1`] to 6 for
    /// [`HeadingLevel::H6`].
    #[must_use]
    pub const fn depth(self) -> u8 {
        match self {
            Self::H1 => 1,
            Self::H2 => 2,
            Self::H3 => 3,
            Self::H4 => 4,
            Self::H5 => 5,
            Self::H6 => 6,
        }
    }
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
                level: HeadingLevel::H2,
                children: vec![Inline::Text(Text::borrowed("heading"))],
            }),
            Block::List(List {
                kind: ListKind::Unordered,
                items: vec![ListItem {
                    task: None,
                    blocks: vec![Block::Paragraph(vec![Inline::Text(Text::borrowed("item"))])],
                }],
                loose: false,
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
