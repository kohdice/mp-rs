use std::io::{self, Write};

use mp_ast::{
    Block, BlockQuote, BlockQuoteKind, CodeBlock, Inline, LinkKind, List, ListItem, Table,
    TaskState,
};

use crate::list::{ListMarkerDisplay, list_marker, marker_width, task_marker, write_list_marker};
use crate::style::{TextStyle, heading_style};
use crate::table::{BorderKind, table_layout, write_border_line, write_table_row};
use crate::theme::{Palette, solarized};
use crate::writer::{LinePrefixWriter, write_repeated_str, write_spaces};

const THEMATIC_BREAK_WIDTH: usize = 32;

#[derive(Debug, Clone, Copy, Default, PartialEq, Eq)]
pub struct RenderOptions {
    pub ansi: bool,
}

#[derive(Debug, Clone, Copy, Default, PartialEq, Eq)]
pub struct RenderState {
    rendered_blocks: usize,
    ended_with_newline: bool,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Renderer {
    options: RenderOptions,
    palette: Palette,
}

impl Renderer {
    #[must_use]
    pub fn new(options: RenderOptions) -> Self {
        Self { options, palette: solarized::DARK_PALETTE }
    }

    /// Renders a complete Markdown AST to a writer.
    ///
    /// # Errors
    ///
    /// Returns any I/O error reported by the writer.
    pub fn render<W>(&self, writer: &mut W, document: &mp_ast::Document<'_>) -> io::Result<()>
    where
        W: Write + ?Sized,
    {
        let mut state = RenderState::default();
        self.render_blocks(writer, &document.blocks, 0, &mut state)?;
        self.finish(writer, &state, document.has_trailing_newline)
    }

    /// Renders one top-level block and updates the streaming render state.
    ///
    /// # Errors
    ///
    /// Returns any I/O error reported by the writer.
    pub fn render_block<W>(
        &self,
        writer: &mut W,
        block: &Block<'_>,
        state: &mut RenderState,
    ) -> io::Result<()>
    where
        W: Write + ?Sized,
    {
        self.render_block_with_state(writer, block, 0, state)
    }

    /// Completes streaming rendering by writing any required trailing newline.
    ///
    /// # Errors
    ///
    /// Returns any I/O error reported by the writer.
    pub fn finish<W>(
        &self,
        writer: &mut W,
        state: &RenderState,
        has_trailing_newline: bool,
    ) -> io::Result<()>
    where
        W: Write + ?Sized,
    {
        if has_trailing_newline && !state.ended_with_newline {
            writer.write_all(b"\n")?;
        }
        Ok(())
    }

    fn render_blocks<W>(
        &self,
        writer: &mut W,
        blocks: &[Block<'_>],
        depth: usize,
        state: &mut RenderState,
    ) -> io::Result<()>
    where
        W: Write + ?Sized,
    {
        for block in blocks {
            self.render_block_with_state(writer, block, depth, state)?;
        }
        Ok(())
    }

    fn render_block_with_state<W>(
        &self,
        writer: &mut W,
        block: &Block<'_>,
        depth: usize,
        state: &mut RenderState,
    ) -> io::Result<()>
    where
        W: Write + ?Sized,
    {
        let mut writer = NewlineTrackingWriter::new(writer, state.ended_with_newline);
        if state.rendered_blocks > 0 {
            writer.write_all(b"\n")?;
        }
        self.render_block_at_depth(&mut writer, depth, block)?;
        state.rendered_blocks += 1;
        state.ended_with_newline = writer.ended_with_newline();
        Ok(())
    }

    fn render_block_at_depth(
        &self,
        writer: &mut dyn Write,
        depth: usize,
        block: &Block<'_>,
    ) -> io::Result<()> {
        match block {
            Block::Paragraph(inlines) => self.render_paragraph(writer, inlines, None),
            Block::Heading(heading) => self.render_inlines(
                writer,
                &heading.children,
                heading_style(heading.level, self.palette),
                None,
            ),
            Block::BlockQuote(blockquote) => self.render_blockquote(writer, blockquote, depth),
            Block::List(list) => self.render_list(writer, list, depth, 0),
            Block::CodeBlock(code_block) => self.render_code_block(writer, code_block),
            Block::HtmlBlock(text) => writer.write_all(text.as_bytes()),
            Block::Table(table) => self.render_table(writer, table),
            Block::ThematicBreak => {
                self.write_style_start(writer, TextStyle::default().fg(self.palette.muted))?;
                write_repeated_str(writer, "─", THEMATIC_BREAK_WIDTH)?;
                self.write_style_end(writer)
            }
            Block::BlankLine => Ok(()),
        }
    }

    fn render_paragraph(
        &self,
        writer: &mut dyn Write,
        inlines: &[Inline<'_>],
        break_prefix: Option<usize>,
    ) -> io::Result<()> {
        self.render_inlines(
            writer,
            inlines,
            TextStyle::default().fg(self.palette.body),
            break_prefix,
        )
    }

    fn render_blockquote(
        &self,
        writer: &mut dyn Write,
        blockquote: &BlockQuote<'_>,
        depth: usize,
    ) -> io::Result<()> {
        let options = self.options;
        let palette = self.palette;
        let mut prefixed = LinePrefixWriter::new(writer, move |writer| {
            write_styled_text(writer, options, TextStyle::default().fg(palette.muted).dim(), "│ ")
        });
        let mut state = RenderState::default();
        if let Some(kind) = blockquote.kind {
            self.write_styled_text(
                &mut prefixed,
                TextStyle::default().fg(self.palette.list_marker).bold(),
                blockquote_kind_label(kind),
            )?;
            state.rendered_blocks = 1;
        }
        self.render_blocks(&mut prefixed, &blockquote.blocks, depth, &mut state)?;
        prefixed.finish()
    }

    fn render_list(
        &self,
        writer: &mut dyn Write,
        list: &List<'_>,
        depth: usize,
        indent: usize,
    ) -> io::Result<()> {
        for (index, item) in list.items.iter().enumerate() {
            if index > 0 {
                writer.write_all(b"\n")?;
            }
            let marker = list_marker(list, index, depth)?;
            self.render_list_item(writer, marker, item, depth, indent)?;
        }
        Ok(())
    }

    fn render_list_item(
        &self,
        writer: &mut dyn Write,
        marker: ListMarkerDisplay<'_>,
        item: &ListItem<'_>,
        depth: usize,
        indent: usize,
    ) -> io::Result<()> {
        write_spaces(writer, indent)?;
        self.write_style_start(writer, TextStyle::default().fg(self.palette.list_marker).bold())?;
        write_list_marker(writer, marker)?;
        self.write_style_end(writer)?;
        writer.write_all(b" ")?;
        self.write_task_marker(writer, item.task)?;

        let content_width = indent + marker_width(marker, item.task);
        if item.blocks.is_empty() {
            return Ok(());
        }

        for (index, block) in item.blocks.iter().enumerate() {
            if index > 0 {
                writer.write_all(b"\n")?;
            }
            match block {
                Block::Paragraph(inlines) if index == 0 => {
                    self.render_paragraph(writer, inlines, Some(content_width))?;
                }
                Block::Paragraph(inlines) => {
                    write_spaces(writer, content_width)?;
                    self.render_paragraph(writer, inlines, Some(content_width))?;
                }
                Block::List(list) => self.render_list(writer, list, depth + 1, content_width)?,
                Block::BlankLine => {}
                child => {
                    if index == 0 {
                        self.render_block_at_depth(writer, depth + 1, child)?;
                    } else {
                        self.render_indented_block(writer, child, content_width, depth + 1)?;
                    }
                }
            }
        }
        Ok(())
    }

    fn render_indented_block(
        &self,
        writer: &mut dyn Write,
        block: &Block<'_>,
        indent: usize,
        depth: usize,
    ) -> io::Result<()> {
        let mut prefixed =
            LinePrefixWriter::new(writer, move |writer| write_spaces(writer, indent));
        self.render_block_at_depth(&mut prefixed, depth, block)?;
        prefixed.finish()
    }

    fn render_code_block(
        &self,
        writer: &mut dyn Write,
        code_block: &CodeBlock<'_>,
    ) -> io::Result<()> {
        let fence_style = TextStyle::default().fg(self.palette.code_fence).dim();
        let code_style = TextStyle::default().fg(self.palette.inline_code);
        self.write_style_start(writer, fence_style)?;
        writer.write_all(b"```")?;
        if let Some(info) = &code_block.info {
            writer.write_all(info.as_bytes())?;
        }
        self.write_style_end(writer)?;
        if !code_block.text.is_empty() {
            writer.write_all(b"\n")?;
            self.write_styled_text(writer, code_style, &code_block.text)?;
        }
        if !code_block.text.ends_with('\n') {
            writer.write_all(b"\n")?;
        }
        self.write_styled_text(writer, fence_style, "```")
    }

    fn render_table(&self, writer: &mut dyn Write, table: &Table<'_>) -> io::Result<()> {
        let layout = table_layout(table);
        if layout.widths.is_empty() {
            return Ok(());
        }

        self.write_table_border(writer, &layout.widths, BorderKind::Top)?;
        if !table.header.is_empty() {
            writer.write_all(b"\n")?;
            self.write_table_row(
                writer,
                &table.header,
                &layout.header,
                &layout.widths,
                &table.alignments,
            )?;
            writer.write_all(b"\n")?;
            self.write_table_border(writer, &layout.widths, BorderKind::Middle)?;
        }
        for (index, row) in table.rows.iter().enumerate() {
            writer.write_all(b"\n")?;
            if index > 0 {
                self.write_table_border(writer, &layout.widths, BorderKind::Middle)?;
                writer.write_all(b"\n")?;
            }
            self.write_table_row(
                writer,
                row,
                &layout.rows[index],
                &layout.widths,
                &table.alignments,
            )?;
        }
        writer.write_all(b"\n")?;
        self.write_table_border(writer, &layout.widths, BorderKind::Bottom)?;
        Ok(())
    }

    fn render_inlines(
        &self,
        writer: &mut dyn Write,
        inlines: &[Inline<'_>],
        current_style: TextStyle,
        break_prefix: Option<usize>,
    ) -> io::Result<()> {
        self.write_style_start(writer, current_style)?;
        self.render_inlines_inner(writer, inlines, current_style, break_prefix)?;
        self.write_style_end(writer)
    }

    fn render_inlines_inner(
        &self,
        writer: &mut dyn Write,
        inlines: &[Inline<'_>],
        current_style: TextStyle,
        break_prefix: Option<usize>,
    ) -> io::Result<()> {
        for inline in inlines {
            match inline {
                Inline::Text(text) => writer.write_all(text.as_bytes())?,
                Inline::Emphasis(children) => {
                    let child_style = current_style.italic();
                    self.write_style_start(writer, child_style)?;
                    self.render_inlines_inner(writer, children, child_style, break_prefix)?;
                    self.restore_style(writer, current_style)?;
                }
                Inline::Strong(children) => {
                    let child_style = current_style.bold();
                    self.write_style_start(writer, child_style)?;
                    self.render_inlines_inner(writer, children, child_style, break_prefix)?;
                    self.restore_style(writer, current_style)?;
                }
                Inline::Strikethrough(children) => {
                    let child_style = current_style.strikethrough();
                    self.write_style_start(writer, child_style)?;
                    self.render_inlines_inner(writer, children, child_style, break_prefix)?;
                    self.restore_style(writer, current_style)?;
                }
                Inline::Code(text) => {
                    let style = TextStyle::default().fg(self.palette.inline_code);
                    self.write_style_start(writer, style)?;
                    writer.write_all(b"`")?;
                    writer.write_all(text.as_bytes())?;
                    writer.write_all(b"`")?;
                    self.restore_style(writer, current_style)?;
                }
                Inline::Link { destination, title, kind, children } => {
                    let child_style = current_style.fg(self.palette.link).underline();
                    self.write_style_start(writer, child_style)?;
                    self.render_inlines_inner(writer, children, child_style, break_prefix)?;
                    self.restore_style(writer, current_style)?;
                    if *kind == LinkKind::Regular {
                        self.write_url_display(writer, destination, title, current_style)?;
                    }
                }
                Inline::Image { destination, title, alt } => {
                    let image_style = TextStyle::default().fg(self.palette.muted).italic();
                    self.write_style_start(writer, image_style)?;
                    writer.write_all(b"[img: ")?;
                    self.render_inlines_inner(writer, alt, image_style, break_prefix)?;
                    writer.write_all(b"]")?;
                    self.restore_style(writer, current_style)?;
                    self.write_url_display(writer, destination, title, current_style)?;
                }
                Inline::HardBreak | Inline::SoftBreak => {
                    writer.write_all(b"\n")?;
                    if let Some(prefix) = break_prefix {
                        write_spaces(writer, prefix)?;
                    }
                }
            }
        }
        Ok(())
    }

    fn write_table_border(
        &self,
        writer: &mut dyn Write,
        widths: &[usize],
        kind: BorderKind,
    ) -> io::Result<()> {
        self.write_style_start(writer, TextStyle::default().fg(self.palette.muted))?;
        write_border_line(writer, widths, kind)?;
        self.write_style_end(writer)
    }

    fn write_table_row(
        &self,
        writer: &mut dyn Write,
        row: &[Vec<Inline<'_>>],
        row_layout: &crate::table::RowLayout,
        widths: &[usize],
        alignments: &[mp_ast::Alignment],
    ) -> io::Result<()> {
        self.write_style_start(writer, TextStyle::default().fg(self.palette.body))?;
        write_table_row(writer, row, row_layout, widths, alignments)?;
        self.write_style_end(writer)
    }

    fn write_url_display(
        &self,
        writer: &mut dyn Write,
        destination: &str,
        title: &str,
        restore_style: TextStyle,
    ) -> io::Result<()> {
        let muted_dim = TextStyle::default().fg(self.palette.muted).dim();
        self.write_style_start(writer, muted_dim)?;
        writer.write_all(b"(")?;
        writer.write_all(destination.as_bytes())?;
        writer.write_all(b")")?;
        if !title.is_empty() {
            let title_style = TextStyle::default().fg(self.palette.muted).dim().italic();
            self.write_style_start(writer, title_style)?;
            writer.write_all(" — ".as_bytes())?;
            writer.write_all(title.as_bytes())?;
        }
        self.restore_style(writer, restore_style)?;
        Ok(())
    }

    fn write_task_marker(&self, writer: &mut dyn Write, task: Option<TaskState>) -> io::Result<()> {
        if let Some(marker) = task_marker(task) {
            let style = match task {
                Some(TaskState::Checked) => TextStyle::default().fg(self.palette.list_marker),
                Some(TaskState::Unchecked) => TextStyle::default().fg(self.palette.muted).dim(),
                None => TextStyle::default(),
            };
            self.write_styled_text(writer, style, marker)?;
        }
        Ok(())
    }

    fn write_styled_text(
        &self,
        writer: &mut dyn Write,
        style: TextStyle,
        text: &str,
    ) -> io::Result<()> {
        write_styled_text(writer, self.options, style, text)
    }

    fn write_style_start(&self, writer: &mut dyn Write, style: TextStyle) -> io::Result<()> {
        write_style_start(writer, self.options, style)
    }

    fn write_style_end(&self, writer: &mut dyn Write) -> io::Result<()> {
        write_style_end(writer, self.options)
    }

    fn restore_style(&self, writer: &mut dyn Write, style: TextStyle) -> io::Result<()> {
        self.write_style_end(writer)?;
        self.write_style_start(writer, style)
    }
}

fn blockquote_kind_label(kind: BlockQuoteKind) -> &'static str {
    match kind {
        BlockQuoteKind::Note => "NOTE",
        BlockQuoteKind::Tip => "TIP",
        BlockQuoteKind::Important => "IMPORTANT",
        BlockQuoteKind::Warning => "WARNING",
        BlockQuoteKind::Caution => "CAUTION",
    }
}

struct NewlineTrackingWriter<'a, W>
where
    W: Write + ?Sized,
{
    inner: &'a mut W,
    ended_with_newline: bool,
}

impl<'a, W> NewlineTrackingWriter<'a, W>
where
    W: Write + ?Sized,
{
    fn new(inner: &'a mut W, ended_with_newline: bool) -> Self {
        Self { inner, ended_with_newline }
    }

    fn ended_with_newline(&self) -> bool {
        self.ended_with_newline
    }

    fn track_bytes(&mut self, bytes: &[u8]) {
        if let Some(last) = bytes.last() {
            self.ended_with_newline = *last == b'\n';
        }
    }
}

impl<W> Write for NewlineTrackingWriter<'_, W>
where
    W: Write + ?Sized,
{
    fn write(&mut self, buffer: &[u8]) -> io::Result<usize> {
        let written = self.inner.write(buffer)?;
        self.track_bytes(&buffer[..written]);
        Ok(written)
    }

    fn write_all(&mut self, buffer: &[u8]) -> io::Result<()> {
        self.inner.write_all(buffer)?;
        self.track_bytes(buffer);
        Ok(())
    }

    fn flush(&mut self) -> io::Result<()> {
        self.inner.flush()
    }
}

fn write_styled_text<W>(
    writer: &mut W,
    options: RenderOptions,
    style: TextStyle,
    text: &str,
) -> io::Result<()>
where
    W: Write + ?Sized,
{
    if text.is_empty() {
        return Ok(());
    }
    write_style_start(writer, options, style)?;
    writer.write_all(text.as_bytes())?;
    write_style_end(writer, options)
}

fn write_style_start<W>(writer: &mut W, options: RenderOptions, style: TextStyle) -> io::Result<()>
where
    W: Write + ?Sized,
{
    if options.ansi {
        let mut buffer = [0; 48];
        let mut length = 0;
        push_sgr_param(&mut buffer, &mut length, b"1", style.is_bold());
        push_sgr_param(&mut buffer, &mut length, b"2", style.is_dim());
        push_sgr_param(&mut buffer, &mut length, b"3", style.is_italic());
        push_sgr_param(&mut buffer, &mut length, b"4", style.is_underline());
        push_sgr_param(&mut buffer, &mut length, b"9", style.is_strikethrough());
        if let Some(fg) = style.fg {
            push_sgr_prefix(&mut buffer, &mut length);
            push_bytes(&mut buffer, &mut length, b"38;2;");
            push_decimal_u8(&mut buffer, &mut length, fg.r);
            push_bytes(&mut buffer, &mut length, b";");
            push_decimal_u8(&mut buffer, &mut length, fg.g);
            push_bytes(&mut buffer, &mut length, b";");
            push_decimal_u8(&mut buffer, &mut length, fg.b);
        }
        if length > 0 {
            push_bytes(&mut buffer, &mut length, b"m");
            writer.write_all(&buffer[..length])?;
        }
    }
    Ok(())
}

fn write_style_end<W>(writer: &mut W, options: RenderOptions) -> io::Result<()>
where
    W: Write + ?Sized,
{
    if options.ansi {
        writer.write_all(b"\x1b[0m")?;
    }
    Ok(())
}

fn push_sgr_param(buffer: &mut [u8; 48], length: &mut usize, parameter: &[u8], enabled: bool) {
    if enabled {
        push_sgr_prefix(buffer, length);
        push_bytes(buffer, length, parameter);
    }
}

fn push_sgr_prefix(buffer: &mut [u8; 48], length: &mut usize) {
    if *length == 0 {
        push_bytes(buffer, length, b"\x1b[");
    } else {
        push_bytes(buffer, length, b";");
    }
}

fn push_decimal_u8(buffer: &mut [u8; 48], length: &mut usize, value: u8) {
    if value >= 100 {
        push_byte(buffer, length, b'0' + value / 100);
        push_byte(buffer, length, b'0' + value / 10 % 10);
    } else if value >= 10 {
        push_byte(buffer, length, b'0' + value / 10);
    }
    push_byte(buffer, length, b'0' + value % 10);
}

fn push_bytes(buffer: &mut [u8; 48], length: &mut usize, bytes: &[u8]) {
    let end = *length + bytes.len();
    buffer[*length..end].copy_from_slice(bytes);
    *length = end;
}

fn push_byte(buffer: &mut [u8; 48], length: &mut usize, byte: u8) {
    buffer[*length] = byte;
    *length += 1;
}

#[cfg(test)]
fn utf8(bytes: Vec<u8>) -> io::Result<String> {
    String::from_utf8(bytes).map_err(|error| io::Error::new(io::ErrorKind::InvalidData, error))
}

#[cfg(test)]
mod tests {
    use std::io::{self, Write};

    use mp_ast::{
        Alignment, Block, BlockQuote, BlockQuoteKind, CodeBlock, Document, Heading, Inline,
        LinkKind, List, ListItem, ListKind, Table, TaskState, Text,
    };

    use super::*;

    #[test]
    fn renders_reference_style_block_spacing_and_markers() -> io::Result<()> {
        let document = Document {
            blocks: vec![
                Block::Heading(Heading {
                    level: 1,
                    children: vec![Inline::Text(Text::borrowed("Title"))],
                }),
                Block::BlankLine,
                Block::List(List {
                    kind: ListKind::Unordered,
                    items: vec![list_item(None, "item")],
                }),
                Block::BlockQuote(BlockQuote {
                    kind: None,
                    blocks: vec![Block::Paragraph(vec![Inline::Text(Text::borrowed("quoted"))])],
                }),
            ],
            has_trailing_newline: true,
        };

        assert_eq!(render_plain(&document)?, "Title\n\n• item\n│ quoted\n");
        Ok(())
    }

    #[test]
    fn renders_task_lists_with_checkbox_glyphs() -> io::Result<()> {
        let document = Document {
            blocks: vec![Block::List(List {
                kind: ListKind::Unordered,
                items: vec![
                    list_item(Some(TaskState::Checked), "done"),
                    list_item(Some(TaskState::Unchecked), "todo"),
                ],
            })],
            has_trailing_newline: true,
        };

        assert_eq!(render_plain(&document)?, "• ☑ done\n• ☐ todo\n");
        Ok(())
    }

    #[test]
    fn indents_nested_lists_to_the_parent_content_column() -> io::Result<()> {
        let document = Document {
            blocks: vec![Block::List(List {
                kind: ListKind::Ordered { start: 9 },
                items: vec![
                    ListItem {
                        task: None,
                        blocks: vec![
                            Block::Paragraph(vec![Inline::Text(Text::borrowed("item"))]),
                            Block::List(List {
                                kind: ListKind::Ordered { start: 1 },
                                items: vec![list_item(None, "child")],
                            }),
                        ],
                    },
                    ListItem {
                        task: Some(TaskState::Checked),
                        blocks: vec![
                            Block::Paragraph(vec![Inline::Text(Text::borrowed("next"))]),
                            Block::List(List {
                                kind: ListKind::Unordered,
                                items: vec![list_item(None, "child")],
                            }),
                        ],
                    },
                ],
            })],
            has_trailing_newline: true,
        };

        assert_eq!(render_plain(&document)?, "9. item\n   1. child\n10. ☑ next\n      ◦ child\n",);
        Ok(())
    }

    #[test]
    fn renders_links_images_and_titles_like_markdown_preview() -> io::Result<()> {
        let document = Document {
            blocks: vec![Block::Paragraph(vec![
                Inline::Link {
                    destination: Text::borrowed("https://example.com"),
                    title: Text::borrowed("Example"),
                    kind: LinkKind::Regular,
                    children: vec![Inline::Text(Text::borrowed("link"))],
                },
                Inline::Text(Text::borrowed(" ")),
                Inline::Image {
                    destination: Text::borrowed("image.png"),
                    title: Text::borrowed("Logo"),
                    alt: vec![Inline::Text(Text::borrowed("alt"))],
                },
            ])],
            has_trailing_newline: false,
        };

        assert_eq!(
            render_plain(&document)?,
            "link(https://example.com) — Example [img: alt](image.png) — Logo",
        );
        Ok(())
    }

    #[test]
    fn renders_tables_with_box_drawing_borders() -> io::Result<()> {
        let document = Document {
            blocks: vec![Block::Table(Table {
                header: vec![
                    vec![Inline::Text(Text::borrowed("A"))],
                    vec![Inline::Text(Text::borrowed("B"))],
                ],
                alignments: vec![Alignment::Left, Alignment::Left],
                rows: vec![vec![
                    vec![Inline::Text(Text::borrowed("1"))],
                    vec![Inline::Text(Text::borrowed("2"))],
                ]],
            })],
            has_trailing_newline: true,
        };

        assert_eq!(
            render_plain(&document)?,
            "┌─────┬─────┐\n│ A   │ B   │\n├─────┼─────┤\n│ 1   │ 2   │\n└─────┴─────┘\n",
        );
        Ok(())
    }

    #[test]
    fn renders_fenced_code_blocks_with_fences() -> io::Result<()> {
        let document = Document {
            blocks: vec![Block::CodeBlock(CodeBlock {
                info: Some(Text::borrowed("rust")),
                text: Text::borrowed("fn main() {}\n"),
            })],
            has_trailing_newline: true,
        };

        assert_eq!(render_plain(&document)?, "```rust\nfn main() {}\n```\n");
        Ok(())
    }

    #[test]
    fn renders_synthetic_closing_fence_for_code_blocks() -> io::Result<()> {
        let document = Document {
            blocks: vec![Block::CodeBlock(CodeBlock { info: None, text: Text::borrowed("abc\n") })],
            has_trailing_newline: true,
        };

        assert_eq!(render_plain(&document)?, "```\nabc\n```\n");
        Ok(())
    }

    #[test]
    fn renders_html_blocks_without_dropping_raw_content() -> io::Result<()> {
        let document = Document {
            blocks: vec![Block::HtmlBlock(Text::borrowed("<div>\nhello\n</div>\n"))],
            has_trailing_newline: true,
        };

        assert_eq!(render_plain(&document)?, "<div>\nhello\n</div>\n");
        Ok(())
    }

    #[test]
    fn renders_gfm_blockquote_kind_as_a_visible_label() -> io::Result<()> {
        let document = Document {
            blocks: vec![Block::BlockQuote(BlockQuote {
                kind: Some(BlockQuoteKind::Note),
                blocks: vec![Block::Paragraph(vec![Inline::Text(Text::borrowed("Read this"))])],
            })],
            has_trailing_newline: true,
        };

        assert_eq!(render_plain(&document)?, "│ NOTE\n│ Read this\n");
        Ok(())
    }

    #[test]
    fn emits_ansi_styling_when_enabled() -> io::Result<()> {
        let document = Document {
            blocks: vec![Block::Heading(Heading {
                level: 1,
                children: vec![Inline::Text(Text::borrowed("Title"))],
            })],
            has_trailing_newline: false,
        };

        let mut output = Vec::new();
        Renderer::new(RenderOptions { ansi: true }).render(&mut output, &document)?;

        assert_eq!(utf8(output)?, "\u{1b}[1;4;38;2;181;137;0mTitle\u{1b}[0m",);
        Ok(())
    }

    #[test]
    fn resets_ansi_style_before_restoring_parent_inline_style() -> io::Result<()> {
        let document = Document {
            blocks: vec![Block::Paragraph(vec![
                Inline::Text(Text::borrowed("a ")),
                Inline::Strong(vec![Inline::Text(Text::borrowed("b"))]),
                Inline::Text(Text::borrowed(" c")),
            ])],
            has_trailing_newline: false,
        };
        let mut output = Vec::new();

        Renderer::new(RenderOptions { ansi: true }).render(&mut output, &document)?;

        assert_eq!(
            utf8(output)?,
            "\u{1b}[38;2;131;148;150ma \u{1b}[1;38;2;131;148;150mb\u{1b}[0m\u{1b}[38;2;131;148;150m c\u{1b}[0m",
        );
        Ok(())
    }

    #[test]
    fn writes_to_the_provided_writer() -> io::Result<()> {
        let document = Document {
            blocks: vec![Block::Paragraph(vec![Inline::Text(Text::borrowed("stream"))])],
            has_trailing_newline: true,
        };
        let mut writer = CountingWriter::default();

        Renderer::new(RenderOptions { ansi: false }).render(&mut writer, &document)?;

        assert!(writer.write_count > 0);
        assert_eq!(writer.flush_count, 0);
        assert_eq!(utf8(writer.bytes)?, "stream\n");
        Ok(())
    }

    fn render_plain(document: &Document<'_>) -> io::Result<String> {
        let mut output = Vec::new();
        Renderer::new(RenderOptions { ansi: false }).render(&mut output, document)?;
        utf8(output)
    }

    fn list_item(task: Option<TaskState>, text: &'static str) -> ListItem<'static> {
        ListItem { task, blocks: vec![Block::Paragraph(vec![Inline::Text(Text::borrowed(text))])] }
    }

    #[derive(Default)]
    struct CountingWriter {
        bytes: Vec<u8>,
        write_count: usize,
        flush_count: usize,
    }

    impl Write for CountingWriter {
        fn write(&mut self, buffer: &[u8]) -> io::Result<usize> {
            self.write_count += 1;
            self.bytes.write(buffer)
        }

        fn flush(&mut self) -> io::Result<()> {
            self.flush_count += 1;
            self.bytes.flush()
        }
    }
}
