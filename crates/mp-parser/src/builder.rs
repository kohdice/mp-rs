use std::ops::Range;

use mp_ast::{
    Alignment, Block, BlockQuote, BlockQuoteKind, CodeBlock, Heading, HeadingLevel, Inline,
    LinkKind, List, ListItem, ListKind, Table, TaskState, Text,
};
use pulldown_cmark::{
    Alignment as MarkdownAlignment, BlockQuoteKind as MarkdownBlockQuoteKind, CodeBlockKind,
    CowStr, Event, HeadingLevel as MarkdownHeadingLevel, LinkType, Tag, TagEnd,
};

use crate::ParseError;
use crate::source::{
    gap_has_blank_line, gap_has_blockquote_blank_line, trim_trailing_blank_gap_end,
};

pub(crate) struct AstBuilder<'a> {
    source: &'a str,
    frames: Vec<Frame<'a>>,
}

impl<'a> AstBuilder<'a> {
    pub(crate) fn new(source: &'a str) -> Self {
        Self {
            source,
            frames: vec![Frame::Document { blocks: Vec::new(), last_end: 0, seen_block: false }],
        }
    }

    pub(crate) fn push_event(
        &mut self,
        event: Event<'a>,
        range: Range<usize>,
    ) -> Result<(), ParseError> {
        match event {
            Event::Start(tag) => self.start_tag(tag, range),
            Event::End(tag) => self.end_tag(tag, range),
            Event::Text(text) | Event::Html(text) | Event::InlineHtml(text) => {
                self.append_text(cow_str_to_text(text))
            }
            Event::Code(text) | Event::InlineMath(text) | Event::DisplayMath(text) => {
                self.append_inline(Inline::Code(cow_str_to_text(text)))
            }
            Event::FootnoteReference(text) => {
                self.append_inline(Inline::Text(cow_str_to_text(text)))
            }
            Event::SoftBreak => self.append_inline(Inline::SoftBreak),
            Event::HardBreak => self.append_inline(Inline::HardBreak),
            Event::Rule => self.append_block(Block::ThematicBreak, range),
            Event::TaskListMarker(checked) => self.set_task_marker(checked),
        }
    }

    pub(crate) fn finish(mut self) -> Result<Vec<Block<'a>>, ParseError> {
        if self.frames.len() != 1 {
            return Err(ParseError::new("markdown parser ended with unclosed nodes"));
        }
        let frame = self.pop_frame("document")?;
        let Frame::Document { blocks, last_end: _, seen_block: _ } = frame else {
            return Err(ParseError::new("markdown parser ended without a document"));
        };
        Ok(blocks)
    }

    pub(crate) fn drain_document_blocks(
        &mut self,
    ) -> Result<std::vec::Drain<'_, Block<'a>>, ParseError> {
        let Some(Frame::Document { blocks, .. }) = self.frames.first_mut() else {
            return Err(ParseError::new("markdown parser lost the document frame"));
        };
        Ok(blocks.drain(..))
    }

    fn start_tag(&mut self, tag: Tag<'a>, range: Range<usize>) -> Result<(), ParseError> {
        if matches!(self.frames.last(), Some(Frame::Ignored)) {
            self.frames.push(Frame::Ignored);
            return Ok(());
        }

        match tag {
            Tag::Paragraph => self.frames.push(Frame::Paragraph { inlines: Vec::new() }),
            Tag::Heading { level, .. } => self.frames.push(Frame::Heading {
                level: markdown_heading_level_to_ast(level),
                inlines: Vec::new(),
            }),
            Tag::BlockQuote(kind) => {
                self.frames.push(Frame::BlockQuote {
                    kind: kind.map(markdown_blockquote_kind_to_ast),
                    blocks: Vec::new(),
                    last_end: range.start,
                });
            }
            Tag::CodeBlock(kind) => self
                .frames
                .push(Frame::CodeBlock { info: code_block_info(kind), text: Text::borrowed("") }),
            Tag::HtmlBlock => self.frames.push(Frame::HtmlBlock { text: Text::borrowed("") }),
            Tag::List(start) => self.frames.push(Frame::List {
                kind: match start {
                    Some(start) => ListKind::Ordered { start },
                    None => ListKind::Unordered,
                },
                items: Vec::new(),
                last_item_end: None,
                loose: false,
            }),
            Tag::Item => {
                self.frames.push(Frame::Item {
                    task: None,
                    blocks: Vec::new(),
                    last_end: range.start,
                    range,
                });
            }
            Tag::Table(alignments) => self.frames.push(Frame::Table {
                alignments: alignments.into_iter().map(markdown_alignment_to_ast).collect(),
                header: Vec::new(),
                rows: Vec::new(),
                in_header: false,
            }),
            Tag::TableHead => self.set_table_header(true)?,
            Tag::TableRow => {
                let is_header = self.current_table_is_header()?;
                self.frames.push(Frame::TableRow { is_header, cells: Vec::new() });
            }
            Tag::TableCell => self.frames.push(Frame::TableCell { inlines: Vec::new() }),
            Tag::Emphasis => {
                self.frames
                    .push(Frame::InlineSpan { kind: SpanKind::Emphasis, inlines: Vec::new() });
            }
            Tag::Strong => {
                self.frames.push(Frame::InlineSpan { kind: SpanKind::Strong, inlines: Vec::new() });
            }
            Tag::Strikethrough => self
                .frames
                .push(Frame::InlineSpan { kind: SpanKind::Strikethrough, inlines: Vec::new() }),
            Tag::Link { link_type, dest_url, title, .. } => self.frames.push(Frame::Link {
                destination: cow_str_to_text(dest_url),
                title: non_empty_title(title),
                kind: link_kind(link_type),
                inlines: Vec::new(),
            }),
            Tag::Image { dest_url, title, .. } => self.frames.push(Frame::Image {
                destination: cow_str_to_text(dest_url),
                title: non_empty_title(title),
                inlines: Vec::new(),
            }),
            Tag::FootnoteDefinition(_)
            | Tag::DefinitionList
            | Tag::DefinitionListTitle
            | Tag::DefinitionListDefinition
            | Tag::Superscript
            | Tag::Subscript
            | Tag::MetadataBlock(_) => self.frames.push(Frame::Ignored),
        }
        Ok(())
    }

    fn end_tag(&mut self, tag: TagEnd, range: Range<usize>) -> Result<(), ParseError> {
        // Everything started inside an ignored container pushes an `Ignored`
        // frame (see `start_tag`), so any end tag while one is on top simply
        // unwinds it, whatever the specific tag is.
        if matches!(self.frames.last(), Some(Frame::Ignored)) {
            self.frames.pop();
            return Ok(());
        }

        match tag {
            TagEnd::Paragraph => {
                let Frame::Paragraph { inlines } = self.pop_frame("paragraph")? else {
                    return Err(ParseError::new("markdown paragraph ended out of order"));
                };
                self.append_block(Block::Paragraph(inlines), range)
            }
            TagEnd::Heading(_) => {
                let Frame::Heading { level, inlines } = self.pop_frame("heading")? else {
                    return Err(ParseError::new("markdown heading ended out of order"));
                };
                self.append_block(Block::Heading(Heading { level, children: inlines }), range)
            }
            TagEnd::BlockQuote(_) => {
                let Frame::BlockQuote { kind, blocks, last_end: _ } =
                    self.pop_frame("blockquote")?
                else {
                    return Err(ParseError::new("markdown blockquote ended out of order"));
                };
                self.append_block(Block::BlockQuote(BlockQuote { kind, blocks }), range)
            }
            TagEnd::CodeBlock => {
                let Frame::CodeBlock { info, text } = self.pop_frame("code block")? else {
                    return Err(ParseError::new("markdown code block ended out of order"));
                };
                self.append_block(Block::CodeBlock(CodeBlock { info, text }), range)
            }
            TagEnd::HtmlBlock => {
                let Frame::HtmlBlock { text } = self.pop_frame("html block")? else {
                    return Err(ParseError::new("markdown html block ended out of order"));
                };
                self.append_block(Block::HtmlBlock(text), range)
            }
            TagEnd::List(_) => {
                let Frame::List { kind, items, last_item_end: _, loose } =
                    self.pop_frame("list")?
                else {
                    return Err(ParseError::new("markdown list ended out of order"));
                };
                self.append_block(Block::List(List { kind, items, loose }), range)
            }
            TagEnd::Item => {
                let Frame::Item { task, blocks, last_end: _, range } =
                    self.pop_frame("list item")?
                else {
                    return Err(ParseError::new("markdown list item ended out of order"));
                };
                self.append_list_item(ListItem { task, blocks }, range)
            }
            TagEnd::Table => {
                let Frame::Table { alignments, header, rows, in_header: _ } =
                    self.pop_frame("table")?
                else {
                    return Err(ParseError::new("markdown table ended out of order"));
                };
                self.append_block(Block::Table(Table { header, alignments, rows }), range)
            }
            TagEnd::TableHead => self.set_table_header(false),
            TagEnd::TableRow => {
                let Frame::TableRow { is_header, cells } = self.pop_frame("table row")? else {
                    return Err(ParseError::new("markdown table row ended out of order"));
                };
                self.append_table_row(is_header, cells)
            }
            TagEnd::TableCell => {
                let Frame::TableCell { inlines } = self.pop_frame("table cell")? else {
                    return Err(ParseError::new("markdown table cell ended out of order"));
                };
                self.append_table_cell(inlines)
            }
            TagEnd::Emphasis
            | TagEnd::Strong
            | TagEnd::Strikethrough
            | TagEnd::Link
            | TagEnd::Image => self.end_inline_tag(tag),
            TagEnd::FootnoteDefinition
            | TagEnd::DefinitionList
            | TagEnd::DefinitionListTitle
            | TagEnd::DefinitionListDefinition
            | TagEnd::Superscript
            | TagEnd::Subscript
            | TagEnd::MetadataBlock(_) => self.pop_ignored(),
        }
    }

    fn end_inline_tag(&mut self, tag: TagEnd) -> Result<(), ParseError> {
        match tag {
            TagEnd::Emphasis | TagEnd::Strong | TagEnd::Strikethrough => {
                let Frame::InlineSpan { kind, inlines } = self.pop_frame("inline span")? else {
                    return Err(ParseError::new("markdown inline span ended out of order"));
                };
                self.append_inline(kind.into_inline(inlines))
            }
            TagEnd::Link => {
                let Frame::Link { destination, title, kind, inlines } = self.pop_frame("link")?
                else {
                    return Err(ParseError::new("markdown link ended out of order"));
                };
                self.append_inline(Inline::Link { destination, title, kind, children: inlines })
            }
            TagEnd::Image => {
                let Frame::Image { destination, title, inlines } = self.pop_frame("image")? else {
                    return Err(ParseError::new("markdown image ended out of order"));
                };
                self.append_inline(Inline::Image { destination, title, alt: inlines })
            }
            _ => Err(ParseError::new("markdown inline node ended with a block tag")),
        }
    }

    fn append_text(&mut self, text: Text<'a>) -> Result<(), ParseError> {
        match self.frames.last_mut() {
            Some(
                Frame::CodeBlock { text: code_text, .. } | Frame::HtmlBlock { text: code_text },
            ) => {
                append_text_node(code_text, text);
                Ok(())
            }
            Some(Frame::Ignored) => Ok(()),
            _ => self.append_inline(Inline::Text(text)),
        }
    }

    fn append_inline(&mut self, inline: Inline<'a>) -> Result<(), ParseError> {
        match self.frames.last_mut() {
            Some(
                Frame::Paragraph { inlines }
                | Frame::Heading { inlines, .. }
                | Frame::TableCell { inlines }
                | Frame::InlineSpan { inlines, .. }
                | Frame::Link { inlines, .. }
                | Frame::Image { inlines, .. },
            ) => {
                inlines.push(inline);
                Ok(())
            }
            Some(
                Frame::Document { blocks, .. }
                | Frame::BlockQuote { blocks, .. }
                | Frame::Item { blocks, .. },
            ) => {
                append_inline_to_blocks(blocks, inline);
                Ok(())
            }
            Some(Frame::Ignored) => Ok(()),
            _ => Err(ParseError::new("markdown inline node appeared outside an inline container")),
        }
    }

    fn append_block(&mut self, block: Block<'a>, range: Range<usize>) -> Result<(), ParseError> {
        let source = self.source;
        match self.frames.last_mut() {
            Some(Frame::Document { blocks, last_end, seen_block }) => {
                push_block_with_blank_gap(
                    source,
                    blocks,
                    block,
                    last_end,
                    range,
                    *seen_block,
                    gap_has_blank_line,
                );
                *seen_block = true;
                Ok(())
            }
            Some(Frame::Item { blocks, last_end, .. }) => {
                let has_prior = !blocks.is_empty();
                push_block_with_blank_gap(
                    source,
                    blocks,
                    block,
                    last_end,
                    range,
                    has_prior,
                    gap_has_blank_line,
                );
                Ok(())
            }
            Some(Frame::BlockQuote { blocks, last_end, .. }) => {
                let has_prior = !blocks.is_empty();
                push_block_with_blank_gap(
                    source,
                    blocks,
                    block,
                    last_end,
                    range,
                    has_prior,
                    gap_has_blockquote_blank_line,
                );
                Ok(())
            }
            Some(Frame::Ignored) => Ok(()),
            _ => Err(ParseError::new("markdown block node appeared outside a block container")),
        }
    }

    fn append_list_item(
        &mut self,
        item: ListItem<'a>,
        range: Range<usize>,
    ) -> Result<(), ParseError> {
        let source = self.source;
        match self.frames.last_mut() {
            Some(Frame::List { items, last_item_end, loose, .. }) => {
                if let Some(previous_end) = *last_item_end
                    && range.start >= previous_end
                    && gap_has_blank_line(&source[previous_end..range.start])
                {
                    *loose = true;
                }
                if item.blocks.iter().any(|block| matches!(block, Block::BlankLine)) {
                    *loose = true;
                }
                items.push(item);
                *last_item_end = Some(trim_trailing_blank_gap_end(source, &range));
                Ok(())
            }
            _ => Err(ParseError::new("markdown list item appeared outside a list")),
        }
    }

    fn append_table_cell(&mut self, inlines: Vec<Inline<'a>>) -> Result<(), ParseError> {
        match self.frames.last_mut() {
            Some(Frame::TableRow { cells, .. }) => {
                cells.push(inlines);
                Ok(())
            }
            Some(Frame::Table { header, in_header, .. }) if *in_header => {
                header.push(inlines);
                Ok(())
            }
            _ => Err(ParseError::new("markdown table cell appeared outside a table row")),
        }
    }

    fn append_table_row(
        &mut self,
        is_header: bool,
        cells: Vec<Vec<Inline<'a>>>,
    ) -> Result<(), ParseError> {
        match self.frames.last_mut() {
            Some(Frame::Table { header, rows, .. }) => {
                if is_header {
                    *header = cells;
                } else {
                    rows.push(cells);
                }
                Ok(())
            }
            _ => Err(ParseError::new("markdown table row appeared outside a table")),
        }
    }

    fn set_task_marker(&mut self, checked: bool) -> Result<(), ParseError> {
        for frame in self.frames.iter_mut().rev() {
            if let Frame::Item { task, .. } = frame {
                *task = Some(if checked { TaskState::Checked } else { TaskState::Unchecked });
                return Ok(());
            }
        }
        Err(ParseError::new("markdown task marker appeared outside a list item"))
    }

    fn current_table_is_header(&self) -> Result<bool, ParseError> {
        self.frames
            .iter()
            .rev()
            .find_map(|frame| match frame {
                Frame::Table { in_header, .. } => Some(*in_header),
                _ => None,
            })
            .ok_or_else(|| ParseError::new("markdown table row appeared outside a table"))
    }

    fn set_table_header(&mut self, in_header: bool) -> Result<(), ParseError> {
        for frame in self.frames.iter_mut().rev() {
            if let Frame::Table { in_header: current, .. } = frame {
                *current = in_header;
                return Ok(());
            }
        }
        Err(ParseError::new("markdown table header appeared outside a table"))
    }

    fn pop_frame(&mut self, expected: &'static str) -> Result<Frame<'a>, ParseError> {
        self.frames.pop().ok_or_else(|| {
            ParseError::new(format!("markdown {expected} ended with an empty stack"))
        })
    }

    fn pop_ignored(&mut self) -> Result<(), ParseError> {
        let Frame::Ignored = self.pop_frame("ignored node")? else {
            return Err(ParseError::new("markdown ignored node ended out of order"));
        };
        Ok(())
    }
}

enum Frame<'a> {
    Document {
        blocks: Vec<Block<'a>>,
        last_end: usize,
        seen_block: bool,
    },
    Paragraph {
        inlines: Vec<Inline<'a>>,
    },
    Heading {
        level: HeadingLevel,
        inlines: Vec<Inline<'a>>,
    },
    BlockQuote {
        kind: Option<BlockQuoteKind>,
        blocks: Vec<Block<'a>>,
        last_end: usize,
    },
    List {
        kind: ListKind,
        items: Vec<ListItem<'a>>,
        last_item_end: Option<usize>,
        loose: bool,
    },
    Item {
        task: Option<TaskState>,
        blocks: Vec<Block<'a>>,
        last_end: usize,
        range: Range<usize>,
    },
    CodeBlock {
        info: Option<Text<'a>>,
        text: Text<'a>,
    },
    HtmlBlock {
        text: Text<'a>,
    },
    Table {
        alignments: Vec<Alignment>,
        header: Vec<Vec<Inline<'a>>>,
        rows: Vec<Vec<Vec<Inline<'a>>>>,
        in_header: bool,
    },
    TableRow {
        is_header: bool,
        cells: Vec<Vec<Inline<'a>>>,
    },
    TableCell {
        inlines: Vec<Inline<'a>>,
    },
    InlineSpan {
        kind: SpanKind,
        inlines: Vec<Inline<'a>>,
    },
    Link {
        destination: Text<'a>,
        title: Option<Text<'a>>,
        kind: LinkKind,
        inlines: Vec<Inline<'a>>,
    },
    Image {
        destination: Text<'a>,
        title: Option<Text<'a>>,
        inlines: Vec<Inline<'a>>,
    },
    Ignored,
}

/// The kind of inline emphasis span an [`Frame::InlineSpan`] is collecting.
#[derive(Debug, Clone, Copy)]
enum SpanKind {
    Emphasis,
    Strong,
    Strikethrough,
}

impl SpanKind {
    fn into_inline(self, inlines: Vec<Inline<'_>>) -> Inline<'_> {
        match self {
            Self::Emphasis => Inline::Emphasis(inlines),
            Self::Strong => Inline::Strong(inlines),
            Self::Strikethrough => Inline::Strikethrough(inlines),
        }
    }
}

fn cow_str_to_text(text: CowStr<'_>) -> Text<'_> {
    match text {
        CowStr::Borrowed(text) => Text::borrowed(text),
        CowStr::Boxed(text) => Text::owned(text),
        // `Inlined` is pulldown-cmark's stack-allocated small string; keep it
        // allocation-free by storing it inline instead of promoting to a `String`.
        CowStr::Inlined(text) => Text::inline(&text),
    }
}

fn non_empty_title(title: CowStr<'_>) -> Option<Text<'_>> {
    if title.is_empty() { None } else { Some(cow_str_to_text(title)) }
}

fn code_block_info(kind: CodeBlockKind<'_>) -> Option<Text<'_>> {
    match kind {
        CodeBlockKind::Indented => None,
        CodeBlockKind::Fenced(info) if info.is_empty() => None,
        CodeBlockKind::Fenced(info) => Some(cow_str_to_text(info)),
    }
}

fn markdown_heading_level_to_ast(level: MarkdownHeadingLevel) -> HeadingLevel {
    match level {
        MarkdownHeadingLevel::H1 => HeadingLevel::H1,
        MarkdownHeadingLevel::H2 => HeadingLevel::H2,
        MarkdownHeadingLevel::H3 => HeadingLevel::H3,
        MarkdownHeadingLevel::H4 => HeadingLevel::H4,
        MarkdownHeadingLevel::H5 => HeadingLevel::H5,
        MarkdownHeadingLevel::H6 => HeadingLevel::H6,
    }
}

fn markdown_alignment_to_ast(alignment: MarkdownAlignment) -> Alignment {
    match alignment {
        MarkdownAlignment::None => Alignment::None,
        MarkdownAlignment::Left => Alignment::Left,
        MarkdownAlignment::Center => Alignment::Center,
        MarkdownAlignment::Right => Alignment::Right,
    }
}

fn markdown_blockquote_kind_to_ast(kind: MarkdownBlockQuoteKind) -> BlockQuoteKind {
    match kind {
        MarkdownBlockQuoteKind::Note => BlockQuoteKind::Note,
        MarkdownBlockQuoteKind::Tip => BlockQuoteKind::Tip,
        MarkdownBlockQuoteKind::Important => BlockQuoteKind::Important,
        MarkdownBlockQuoteKind::Warning => BlockQuoteKind::Warning,
        MarkdownBlockQuoteKind::Caution => BlockQuoteKind::Caution,
    }
}

fn link_kind(link_type: LinkType) -> LinkKind {
    match link_type {
        LinkType::Autolink | LinkType::Email => LinkKind::Autolink,
        LinkType::Inline
        | LinkType::Reference
        | LinkType::ReferenceUnknown
        | LinkType::Collapsed
        | LinkType::CollapsedUnknown
        | LinkType::Shortcut
        | LinkType::ShortcutUnknown
        | LinkType::WikiLink { .. } => LinkKind::Regular,
    }
}

fn append_text_node<'a>(target: &mut Text<'a>, addition: Text<'a>) {
    if target.is_empty() {
        *target = addition;
    } else {
        target.push_str(addition.as_ref());
    }
}

fn append_inline_to_blocks<'a>(blocks: &mut Vec<Block<'a>>, inline: Inline<'a>) {
    match blocks.last_mut() {
        Some(Block::Paragraph(inlines)) => inlines.push(inline),
        _ => blocks.push(Block::Paragraph(vec![inline])),
    }
}

fn push_block_with_blank_gap<'a>(
    source: &str,
    blocks: &mut Vec<Block<'a>>,
    block: Block<'a>,
    last_end: &mut usize,
    range: Range<usize>,
    has_prior: bool,
    gap_has_blank: fn(&str) -> bool,
) {
    if has_prior && range.start >= *last_end && gap_has_blank(&source[*last_end..range.start]) {
        blocks.push(Block::BlankLine);
    }
    blocks.push(block);
    *last_end = trim_trailing_blank_gap_end(source, &range);
}
