use crate::{CodeBlock, Inline, List, Table, Text};

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Block<'a> {
    Paragraph(Vec<Inline<'a>>),
    Heading(Heading<'a>),
    BlockQuote(BlockQuote<'a>),
    List(List<'a>),
    CodeBlock(CodeBlock<'a>),
    HtmlBlock(Text<'a>),
    Table(Table<'a>),
    ThematicBreak,
    BlankLine,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Heading<'a> {
    pub level: u8,
    pub children: Vec<Inline<'a>>,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct BlockQuote<'a> {
    pub kind: Option<BlockQuoteKind>,
    pub blocks: Vec<Block<'a>>,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum BlockQuoteKind {
    Note,
    Tip,
    Important,
    Warning,
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
