use std::io::{self, Write};

use mp_ast::{
    Block, BlockQuote, BlockQuoteKind, CodeBlock, Inline, LinkKind, List, ListItem, Table,
    TaskState,
};

use crate::list::{ListMarkerDisplay, list_marker, marker_width, task_marker, write_list_marker};
use crate::style::{TextStyle, heading_style};
use crate::table::{BorderKind, table_layout, write_border_line, write_table_row};
use crate::theme::Palette;
use crate::tokens::{IMAGE_OPEN, LINK_TEXT_CLOSE, TITLE_SEPARATOR, URL_CLOSE, URL_OPEN};
use crate::writer::{LinePrefixWriter, write_repeated_str, write_spaces};

const THEMATIC_BREAK_WIDTH: usize = 32;

/// Configuration for a [`Renderer`].
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq)]
pub struct RenderOptions {
    /// How colors and text styles are written to the output.
    pub color: ColorMode,
    /// Colors used for each kind of content; solarized dark by default.
    pub palette: Palette,
}

/// How colors and text styles are written to the output.
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq)]
pub enum ColorMode {
    /// Emit ANSI escape sequences for colors and text styles.
    Ansi,
    /// Emit plain text without escape sequences.
    #[default]
    Plain,
}

/// Streaming render state threaded through [`Renderer::render_block`] calls.
///
/// Tracks whether any bytes have been written and whether the output currently ends
/// with a newline, so block separators and the final newline are emitted correctly.
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq)]
pub struct RenderState {
    has_rendered: bool,
    ended_with_newline: bool,
}

/// Renders [`Block`]s as terminal output.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Renderer {
    options: RenderOptions,
}

impl Renderer {
    /// Creates a renderer with the given options.
    #[must_use]
    pub fn new(options: RenderOptions) -> Self {
        Self { options }
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

    /// Completes streaming rendering by terminating the output with a newline.
    ///
    /// Output that wrote any bytes ends with exactly one trailing newline; if nothing
    /// was written, `finish` writes nothing.
    ///
    /// # Errors
    ///
    /// Returns any I/O error reported by the writer.
    pub fn finish<W>(&self, writer: &mut W, state: &RenderState) -> io::Result<()>
    where
        W: Write + ?Sized,
    {
        if state.has_rendered && !state.ended_with_newline {
            writer.write_all(b"\n")?;
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
        if state.has_rendered {
            writer.write_all(b"\n")?;
        }
        self.render_block_at_depth(&mut writer, depth, block)?;
        state.has_rendered |= writer.wrote_bytes();
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
                heading_style(heading.level, self.options.palette),
                None,
            ),
            Block::BlockQuote(blockquote) => self.render_blockquote(writer, blockquote, depth),
            Block::List(list) => self.render_list(writer, list, depth, 0),
            Block::CodeBlock(code_block) => self.render_code_block(writer, code_block),
            Block::HtmlBlock(text) => writer.write_all(text.as_bytes()),
            Block::Table(table) => self.render_table(writer, table),
            Block::ThematicBreak => {
                self.write_style_start(
                    writer,
                    TextStyle::default().fg(self.options.palette.muted),
                )?;
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
            TextStyle::default().fg(self.options.palette.body),
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
        let palette = self.options.palette;
        let mut prefixed = LinePrefixWriter::new(&mut *writer, move |writer| {
            write_styled_text(writer, options, TextStyle::default().fg(palette.muted).dim(), "│ ")
        });
        let mut state = RenderState::default();
        if let Some(kind) = blockquote.kind {
            self.write_styled_text(
                &mut prefixed,
                TextStyle::default().fg(self.options.palette.list_marker).bold(),
                blockquote_kind_label(kind),
            )?;
            state.has_rendered = true;
        }
        for block in &blockquote.blocks {
            self.render_block_with_state(&mut prefixed, block, depth, &mut state)?;
        }
        if prefixed.wrote_anything() {
            return Ok(());
        }
        // An empty quote still shows its bar, without the prefix's trailing space.
        self.write_styled_text(
            writer,
            TextStyle::default().fg(self.options.palette.muted).dim(),
            "│",
        )
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
        self.write_style_start(
            writer,
            TextStyle::default().fg(self.options.palette.list_marker).bold(),
        )?;
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
                Block::Paragraph(inlines) => {
                    if index > 0 {
                        write_spaces(writer, content_width)?;
                    }
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
        self.render_block_at_depth(&mut prefixed, depth, block)
    }

    fn render_code_block(
        &self,
        writer: &mut dyn Write,
        code_block: &CodeBlock<'_>,
    ) -> io::Result<()> {
        let fence_style = TextStyle::default().fg(self.options.palette.code_fence).dim();
        let code_style = TextStyle::default().fg(self.options.palette.inline_code);
        self.write_style_start(writer, fence_style)?;
        writer.write_all(b"```")?;
        if let Some(info) = &code_block.info {
            writer.write_all(info.as_bytes())?;
        }
        self.write_style_end(writer)?;
        if !code_block.text.is_empty() {
            writer.write_all(b"\n")?;
            self.render_code_body(writer, code_block, code_style)?;
        }
        if !code_block.text.ends_with('\n') {
            writer.write_all(b"\n")?;
        }
        self.write_styled_text(writer, fence_style, "```")
    }

    fn render_code_body(
        &self,
        writer: &mut dyn Write,
        code_block: &CodeBlock<'_>,
        fallback_style: TextStyle,
    ) -> io::Result<()> {
        if self.options.color == ColorMode::Ansi {
            let info = code_block.info.as_ref().map(|info| info.as_str());
            if let Some(ranges) = crate::syntax::highlighted_ranges(info, &code_block.text) {
                for range in ranges {
                    self.write_styled_text(writer, range.style, range.text)?;
                }
                return Ok(());
            }
        }
        self.write_styled_text(writer, fallback_style, &code_block.text)
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
                    self.render_wrapped_inlines(
                        writer,
                        children,
                        current_style,
                        child_style,
                        break_prefix,
                    )?;
                }
                Inline::Strong(children) => {
                    let child_style = current_style.bold();
                    self.render_wrapped_inlines(
                        writer,
                        children,
                        current_style,
                        child_style,
                        break_prefix,
                    )?;
                }
                Inline::Strikethrough(children) => {
                    let child_style = current_style.strikethrough();
                    self.render_wrapped_inlines(
                        writer,
                        children,
                        current_style,
                        child_style,
                        break_prefix,
                    )?;
                }
                Inline::Code(text) => {
                    let style = TextStyle::default().fg(self.options.palette.inline_code);
                    self.write_style_start(writer, style)?;
                    writer.write_all(b"`")?;
                    writer.write_all(text.as_bytes())?;
                    writer.write_all(b"`")?;
                    self.restore_style(writer, current_style)?;
                }
                Inline::Link { destination, title, kind, children } => {
                    let child_style = current_style.fg(self.options.palette.link).underline();
                    self.render_wrapped_inlines(
                        writer,
                        children,
                        current_style,
                        child_style,
                        break_prefix,
                    )?;
                    if *kind == LinkKind::Regular {
                        self.write_url_display(
                            writer,
                            destination,
                            title.as_deref(),
                            current_style,
                        )?;
                    }
                }
                Inline::Image { destination, title, alt } => {
                    let image_style = TextStyle::default().fg(self.options.palette.muted).italic();
                    self.write_style_start(writer, image_style)?;
                    writer.write_all(IMAGE_OPEN.as_bytes())?;
                    self.render_inlines_inner(writer, alt, image_style, break_prefix)?;
                    writer.write_all(LINK_TEXT_CLOSE.as_bytes())?;
                    self.restore_style(writer, current_style)?;
                    self.write_url_display(writer, destination, title.as_deref(), current_style)?;
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

    fn render_wrapped_inlines(
        &self,
        writer: &mut dyn Write,
        children: &[Inline<'_>],
        current_style: TextStyle,
        child_style: TextStyle,
        break_prefix: Option<usize>,
    ) -> io::Result<()> {
        self.write_style_start(writer, child_style)?;
        self.render_inlines_inner(writer, children, child_style, break_prefix)?;
        self.restore_style(writer, current_style)
    }

    fn write_table_border(
        &self,
        writer: &mut dyn Write,
        widths: &[usize],
        kind: BorderKind,
    ) -> io::Result<()> {
        self.write_style_start(writer, TextStyle::default().fg(self.options.palette.muted))?;
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
        let body_style = TextStyle::default().fg(self.options.palette.body);
        self.write_style_start(writer, body_style)?;
        write_table_row(writer, row, row_layout, widths, alignments, &|writer, cell| {
            self.render_inlines_inner(writer, cell, body_style, None)
        })?;
        self.write_style_end(writer)
    }

    fn write_url_display(
        &self,
        writer: &mut dyn Write,
        destination: &str,
        title: Option<&str>,
        restore_style: TextStyle,
    ) -> io::Result<()> {
        let muted_dim = TextStyle::default().fg(self.options.palette.muted).dim();
        self.write_style_start(writer, muted_dim)?;
        writer.write_all(URL_OPEN.as_bytes())?;
        writer.write_all(destination.as_bytes())?;
        writer.write_all(URL_CLOSE.as_bytes())?;
        if let Some(title) = title {
            let title_style = TextStyle::default().fg(self.options.palette.muted).dim().italic();
            self.write_style_start(writer, title_style)?;
            writer.write_all(TITLE_SEPARATOR.as_bytes())?;
            writer.write_all(title.as_bytes())?;
        }
        self.restore_style(writer, restore_style)?;
        Ok(())
    }

    fn write_task_marker(&self, writer: &mut dyn Write, task: Option<TaskState>) -> io::Result<()> {
        if let Some(marker) = task_marker(task) {
            let style = match task {
                Some(TaskState::Checked) => {
                    TextStyle::default().fg(self.options.palette.list_marker)
                }
                Some(TaskState::Unchecked) => {
                    TextStyle::default().fg(self.options.palette.muted).dim()
                }
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
    wrote_bytes: bool,
}

impl<'a, W> NewlineTrackingWriter<'a, W>
where
    W: Write + ?Sized,
{
    fn new(inner: &'a mut W, ended_with_newline: bool) -> Self {
        Self { inner, ended_with_newline, wrote_bytes: false }
    }

    fn ended_with_newline(&self) -> bool {
        self.ended_with_newline
    }

    fn wrote_bytes(&self) -> bool {
        self.wrote_bytes
    }

    fn track_bytes(&mut self, bytes: &[u8]) {
        if let Some(last) = bytes.last() {
            self.ended_with_newline = *last == b'\n';
            self.wrote_bytes = true;
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
    if options.color == ColorMode::Ansi {
        // Worst case is all five attributes plus a truecolor foreground:
        // "\x1b[" (2) + "1;2;3;4;9" (9) + ";38;2;255;255;255" (17) + "m" (1) = 29 bytes.
        // 48 leaves comfortable headroom while staying allocation-free on the stack.
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
    if options.color == ColorMode::Ansi {
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
    use std::collections::BTreeSet;
    use std::io::{self, Write};

    use mp_ast::{
        Alignment, Block, BlockQuote, BlockQuoteKind, CodeBlock, Heading, HeadingLevel, Inline,
        LinkKind, List, ListItem, ListKind, Table, TaskState, Text,
    };

    use super::*;
    use crate::theme::solarized;

    #[test]
    fn renders_reference_style_block_spacing_and_markers() -> io::Result<()> {
        let blocks = vec![
            Block::Heading(Heading {
                level: HeadingLevel::H1,
                children: vec![Inline::Text(Text::borrowed("Title"))],
            }),
            Block::BlankLine,
            Block::List(List { kind: ListKind::Unordered, items: vec![list_item(None, "item")] }),
            Block::BlockQuote(BlockQuote {
                kind: None,
                blocks: vec![Block::Paragraph(vec![Inline::Text(Text::borrowed("quoted"))])],
            }),
        ];

        assert_eq!(render_plain(&blocks)?, "Title\n\n• item\n│ quoted\n");
        Ok(())
    }

    #[test]
    fn renders_task_lists_with_checkbox_glyphs() -> io::Result<()> {
        let blocks = vec![Block::List(List {
            kind: ListKind::Unordered,
            items: vec![
                list_item(Some(TaskState::Checked), "done"),
                list_item(Some(TaskState::Unchecked), "todo"),
            ],
        })];

        assert_eq!(render_plain(&blocks)?, "• ☑ done\n• ☐ todo\n");
        Ok(())
    }

    #[test]
    fn indents_nested_lists_to_the_parent_content_column() -> io::Result<()> {
        let blocks = vec![Block::List(List {
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
        })];

        assert_eq!(render_plain(&blocks)?, "9. item\n   1. child\n10. ☑ next\n      ◦ child\n",);
        Ok(())
    }

    #[test]
    fn renders_links_images_and_titles_like_markdown_preview() -> io::Result<()> {
        let blocks = vec![Block::Paragraph(vec![
            Inline::Link {
                destination: Text::borrowed("https://example.com"),
                title: Some(Text::borrowed("Example")),
                kind: LinkKind::Regular,
                children: vec![Inline::Text(Text::borrowed("link"))],
            },
            Inline::Text(Text::borrowed(" ")),
            Inline::Image {
                destination: Text::borrowed("image.png"),
                title: Some(Text::borrowed("Logo")),
                alt: vec![Inline::Text(Text::borrowed("alt"))],
            },
        ])];

        assert_eq!(
            render_plain(&blocks)?,
            "link(https://example.com) — Example [img: alt](image.png) — Logo\n",
        );
        Ok(())
    }

    #[test]
    fn renders_tables_with_box_drawing_borders() -> io::Result<()> {
        let blocks = vec![Block::Table(Table {
            header: vec![
                vec![Inline::Text(Text::borrowed("A"))],
                vec![Inline::Text(Text::borrowed("B"))],
            ],
            alignments: vec![Alignment::Left, Alignment::Left],
            rows: vec![vec![
                vec![Inline::Text(Text::borrowed("1"))],
                vec![Inline::Text(Text::borrowed("2"))],
            ]],
        })];

        assert_eq!(
            render_plain(&blocks)?,
            "┌─────┬─────┐\n│ A   │ B   │\n├─────┼─────┤\n│ 1   │ 2   │\n└─────┴─────┘\n",
        );
        Ok(())
    }

    #[test]
    fn table_cell_inline_code_uses_inline_code_color_when_ansi_enabled() -> io::Result<()> {
        let blocks = vec![Block::Table(Table {
            header: vec![vec![Inline::Text(Text::borrowed("H"))]],
            alignments: vec![Alignment::Left],
            rows: vec![vec![vec![Inline::Code(Text::borrowed("mp"))]]],
        })];
        let output = render_ansi(&blocks)?;
        let colors = ansi_rgb_colors(&output);

        assert!(
            colors.contains("42;161;152"),
            "expected inline code color in table output, got {colors:?}",
        );
        Ok(())
    }

    #[test]
    fn table_cell_text_after_inline_code_restores_body_color() -> io::Result<()> {
        let blocks = vec![Block::Table(Table {
            header: vec![vec![Inline::Text(Text::borrowed("H"))]],
            alignments: vec![Alignment::Left],
            rows: vec![vec![vec![
                Inline::Code(Text::borrowed("mp")),
                Inline::Text(Text::borrowed("after")),
            ]]],
        })];
        let output = render_ansi(&blocks)?;

        assert!(
            output.contains("\u{1b}[38;2;131;148;150mafter"),
            "expected body color restored before trailing text, got {output:?}",
        );
        Ok(())
    }

    #[test]
    fn table_cell_strong_renders_bold_when_ansi_enabled() -> io::Result<()> {
        let blocks = vec![Block::Table(Table {
            header: vec![vec![Inline::Strong(vec![Inline::Text(Text::borrowed("H"))])]],
            alignments: vec![Alignment::Left],
            rows: vec![vec![vec![Inline::Text(Text::borrowed("body"))]]],
        })];
        let output = render_ansi(&blocks)?;

        assert!(
            output.contains("\u{1b}[1;38;2;131;148;150mH"),
            "expected bold styling around strong header cell, got {output:?}",
        );
        Ok(())
    }

    #[test]
    fn table_cell_emphasis_and_strikethrough_render_italic_and_strikethrough() -> io::Result<()> {
        let blocks = vec![Block::Table(Table {
            header: vec![vec![Inline::Text(Text::borrowed("H"))]],
            alignments: vec![Alignment::Left],
            rows: vec![
                vec![vec![Inline::Emphasis(vec![Inline::Text(Text::borrowed("em"))])]],
                vec![vec![Inline::Strikethrough(vec![Inline::Text(Text::borrowed("st"))])]],
            ],
        })];
        let output = render_ansi(&blocks)?;

        assert!(
            output.contains("\u{1b}[3;38;2;131;148;150mem"),
            "expected italic styling around emphasis cell, got {output:?}",
        );
        assert!(
            output.contains("\u{1b}[9;38;2;131;148;150mst"),
            "expected strikethrough styling around strikethrough cell, got {output:?}",
        );
        Ok(())
    }

    #[test]
    fn table_cell_link_renders_underline_link_color_and_muted_url() -> io::Result<()> {
        let blocks = vec![Block::Table(Table {
            header: vec![vec![Inline::Text(Text::borrowed("H"))]],
            alignments: vec![Alignment::Left],
            rows: vec![vec![vec![Inline::Link {
                destination: Text::borrowed("example.com"),
                title: None,
                kind: LinkKind::Regular,
                children: vec![Inline::Text(Text::borrowed("site"))],
            }]]],
        })];
        let output = render_ansi(&blocks)?;

        assert!(
            output.contains("\u{1b}[4;38;2;108;113;196msite"),
            "expected underlined link color around link text, got {output:?}",
        );
        assert!(
            output.contains("\u{1b}[2;38;2;88;110;117m(example.com)"),
            "expected muted dim styling around link url, got {output:?}",
        );
        Ok(())
    }

    #[test]
    fn styled_table_output_strips_to_the_same_layout_as_plain_output() -> io::Result<()> {
        let blocks = vec![Block::Table(Table {
            header: vec![
                vec![Inline::Text(Text::borrowed("Command"))],
                vec![Inline::Text(Text::borrowed("Note"))],
            ],
            alignments: vec![Alignment::Left, Alignment::Right],
            rows: vec![
                vec![
                    vec![Inline::Code(Text::borrowed("mp"))],
                    vec![Inline::Strong(vec![Inline::Text(Text::borrowed("bold"))])],
                ],
                vec![
                    vec![Inline::Link {
                        destination: Text::borrowed("example.com"),
                        title: None,
                        kind: LinkKind::Regular,
                        children: vec![Inline::Text(Text::borrowed("site"))],
                    }],
                    vec![Inline::Emphasis(vec![Inline::Text(Text::borrowed("x"))])],
                ],
            ],
        })];

        assert_eq!(strip_ansi(&render_ansi(&blocks)?), render_plain(&blocks)?);
        Ok(())
    }

    #[test]
    fn styled_link_cell_with_title_keeps_plain_layout() -> io::Result<()> {
        let blocks = vec![Block::Table(Table {
            header: vec![vec![Inline::Text(Text::borrowed("H"))]],
            alignments: vec![Alignment::Left],
            rows: vec![vec![vec![Inline::Link {
                destination: Text::borrowed("https://example.com"),
                title: Some(Text::borrowed("Example")),
                kind: LinkKind::Regular,
                children: vec![Inline::Text(Text::borrowed("link"))],
            }]]],
        })];

        assert_eq!(strip_ansi(&render_ansi(&blocks)?), render_plain(&blocks)?);
        Ok(())
    }

    #[test]
    fn styled_image_cell_with_title_keeps_plain_layout() -> io::Result<()> {
        let blocks = vec![Block::Table(Table {
            header: vec![vec![Inline::Text(Text::borrowed("H"))]],
            alignments: vec![Alignment::Left],
            rows: vec![vec![vec![Inline::Image {
                destination: Text::borrowed("image.png"),
                title: Some(Text::borrowed("Logo")),
                alt: vec![Inline::Text(Text::borrowed("alt"))],
            }]]],
        })];

        assert_eq!(strip_ansi(&render_ansi(&blocks)?), render_plain(&blocks)?);
        Ok(())
    }

    #[test]
    fn ansi_disabled_table_cells_stay_plain() -> io::Result<()> {
        let blocks = vec![Block::Table(Table {
            header: vec![vec![Inline::Text(Text::borrowed("H"))]],
            alignments: vec![Alignment::Left],
            rows: vec![vec![vec![Inline::Code(Text::borrowed("mp"))]]],
        })];
        let output = render_plain(&blocks)?;

        assert!(!output.contains('\u{1b}'), "expected no escape sequences, got {output:?}");
        assert!(output.contains("`mp`"), "expected backtick-wrapped code text, got {output:?}");
        Ok(())
    }

    #[test]
    fn renders_fenced_code_blocks_with_fences() -> io::Result<()> {
        let blocks = vec![Block::CodeBlock(CodeBlock {
            info: Some(Text::borrowed("rust")),
            text: Text::borrowed("fn main() {}\n"),
        })];

        assert_eq!(render_plain(&blocks)?, "```rust\nfn main() {}\n```\n");
        Ok(())
    }

    #[test]
    fn ansi_disabled_keeps_fenced_code_blocks_plain() -> io::Result<()> {
        let blocks = rust_code_blocks("rust", "fn main() {}\n");

        assert_eq!(render_plain(&blocks)?, "```rust\nfn main() {}\n```\n");
        Ok(())
    }

    #[test]
    fn highlights_rust_code_without_rewriting_source_text() -> io::Result<()> {
        let blocks = rust_code_blocks("rust", "fn main() {}\n");
        let output = render_ansi(&blocks)?;

        assert_eq!(strip_ansi(&output), "```rust\nfn main() {}\n```\n");
        assert_code_body_has_distinct_syntax_colors(&output)?;
        Ok(())
    }

    #[test]
    fn highlights_rust_code_from_rs_language_alias() -> io::Result<()> {
        let blocks = rust_code_blocks("rs", "fn main() {}\n");
        let output = render_ansi(&blocks)?;

        assert_eq!(strip_ansi(&output), "```rs\nfn main() {}\n```\n");
        assert_code_body_has_distinct_syntax_colors(&output)?;
        Ok(())
    }

    #[test]
    fn unsupported_languages_fall_back_to_single_style_code_body() -> io::Result<()> {
        let blocks = rust_code_blocks("not-a-language", "fn main() {}\n");

        assert_eq!(
            render_ansi(&blocks)?,
            "\u{1b}[2;38;2;88;110;117m```not-a-language\u{1b}[0m\n\
             \u{1b}[38;2;42;161;152mfn main() {}\n\u{1b}[0m\
             \u{1b}[2;38;2;88;110;117m```\u{1b}[0m\n",
        );
        Ok(())
    }

    #[test]
    fn oversized_code_blocks_fall_back_to_single_style_code_body() -> io::Result<()> {
        let oversized = "x".repeat(512 * 1024 + 1);
        let blocks = vec![Block::CodeBlock(CodeBlock {
            info: Some(Text::borrowed("rust")),
            text: Text::owned(oversized.clone()),
        })];
        let output = render_ansi(&blocks)?;

        assert!(output.contains(&format!(
            "\u{1b}[38;2;42;161;152m{oversized}\u{1b}[0m\n\
             \u{1b}[2;38;2;88;110;117m```"
        )));
        Ok(())
    }

    #[test]
    fn highlighting_errors_fall_back_to_plain_code_body() -> io::Result<()> {
        let blocks = rust_code_blocks("rust", "fn main() {}\n");
        crate::syntax::force_next_highlight_error_for_test();

        assert_eq!(
            render_ansi(&blocks)?,
            "\u{1b}[2;38;2;88;110;117m```rust\u{1b}[0m\n\
             \u{1b}[38;2;42;161;152mfn main() {}\n\u{1b}[0m\
             \u{1b}[2;38;2;88;110;117m```\u{1b}[0m\n",
        );
        Ok(())
    }

    #[test]
    fn nested_code_blocks_keep_blockquote_prefixes_while_highlighting() -> io::Result<()> {
        let blocks = vec![Block::BlockQuote(BlockQuote {
            kind: None,
            blocks: vec![Block::CodeBlock(CodeBlock {
                info: Some(Text::borrowed("rust")),
                text: Text::borrowed("fn main() {}\n"),
            })],
        })];
        let output = render_ansi(&blocks)?;
        crate::syntax::force_next_highlight_error_for_test();
        let fallback = render_ansi(&blocks)?;

        assert_eq!(strip_ansi(&output), "│ ```rust\n│ fn main() {}\n│ ```\n");
        assert_ne!(output, fallback);
        assert_code_body_has_distinct_syntax_colors(&output)?;
        Ok(())
    }

    #[test]
    fn renders_synthetic_closing_fence_for_code_blocks() -> io::Result<()> {
        let blocks =
            vec![Block::CodeBlock(CodeBlock { info: None, text: Text::borrowed("abc\n") })];

        assert_eq!(render_plain(&blocks)?, "```\nabc\n```\n");
        Ok(())
    }

    #[test]
    fn renders_html_blocks_without_dropping_raw_content() -> io::Result<()> {
        let blocks = vec![Block::HtmlBlock(Text::borrowed("<div>\nhello\n</div>\n"))];

        assert_eq!(render_plain(&blocks)?, "<div>\nhello\n</div>\n");
        Ok(())
    }

    #[test]
    fn renders_gfm_blockquote_kind_as_a_visible_label() -> io::Result<()> {
        let blocks = vec![Block::BlockQuote(BlockQuote {
            kind: Some(BlockQuoteKind::Note),
            blocks: vec![Block::Paragraph(vec![Inline::Text(Text::borrowed("Read this"))])],
        })];

        assert_eq!(render_plain(&blocks)?, "│ NOTE\n│ Read this\n");
        Ok(())
    }

    #[test]
    fn emits_ansi_styling_when_enabled() -> io::Result<()> {
        let blocks = vec![Block::Heading(Heading {
            level: HeadingLevel::H1,
            children: vec![Inline::Text(Text::borrowed("Title"))],
        })];

        assert_eq!(render_ansi(&blocks)?, "\u{1b}[1;4;38;2;181;137;0mTitle\u{1b}[0m\n",);
        Ok(())
    }

    #[test]
    fn resets_ansi_style_before_restoring_parent_inline_style() -> io::Result<()> {
        let blocks = vec![Block::Paragraph(vec![
            Inline::Text(Text::borrowed("a ")),
            Inline::Strong(vec![Inline::Text(Text::borrowed("b"))]),
            Inline::Text(Text::borrowed(" c")),
        ])];

        assert_eq!(
            render_ansi(&blocks)?,
            "\u{1b}[38;2;131;148;150ma \u{1b}[1;38;2;131;148;150mb\u{1b}[0m\u{1b}[38;2;131;148;150m c\u{1b}[0m\n",
        );
        Ok(())
    }

    #[test]
    fn nested_inline_styles_restore_outer_style_after_a_wrapped_child_closes() -> io::Result<()> {
        let blocks = vec![Block::Paragraph(vec![Inline::Emphasis(vec![
            Inline::Text(Text::borrowed("outer ")),
            Inline::Strong(vec![Inline::Text(Text::borrowed("inner"))]),
            Inline::Text(Text::borrowed(" outer")),
        ])])];

        assert_eq!(
            render_ansi(&blocks)?,
            "\u{1b}[38;2;131;148;150m\u{1b}[3;38;2;131;148;150mouter \
             \u{1b}[1;3;38;2;131;148;150minner\u{1b}[0m\u{1b}[3;38;2;131;148;150m outer\
             \u{1b}[0m\u{1b}[38;2;131;148;150m\u{1b}[0m\n",
        );
        Ok(())
    }

    #[test]
    fn writes_to_the_provided_writer() -> io::Result<()> {
        let blocks = vec![Block::Paragraph(vec![Inline::Text(Text::borrowed("stream"))])];
        let mut writer = CountingWriter::default();

        let renderer = Renderer::new(RenderOptions::default());
        let mut state = RenderState::default();
        for block in &blocks {
            renderer.render_block(&mut writer, block, &mut state)?;
        }
        renderer.finish(&mut writer, &state)?;

        assert!(writer.write_count > 0);
        assert_eq!(writer.flush_count, 0);
        assert_eq!(utf8(writer.bytes)?, "stream\n");
        Ok(())
    }

    #[test]
    fn finish_appends_a_newline_when_nonempty_output_lacks_one() -> io::Result<()> {
        let blocks = vec![Block::Heading(Heading {
            level: HeadingLevel::H1,
            children: vec![Inline::Text(Text::borrowed("Title"))],
        })];

        assert_eq!(render_plain(&blocks)?, "Title\n");
        Ok(())
    }

    #[test]
    fn finish_writes_nothing_for_an_empty_document() -> io::Result<()> {
        assert_eq!(render_plain(&[])?, "");
        Ok(())
    }

    #[test]
    fn rendering_only_a_blank_line_produces_no_output() -> io::Result<()> {
        assert_eq!(render_plain(&[Block::BlankLine])?, "");
        Ok(())
    }

    #[test]
    fn leading_blank_line_does_not_emit_a_separator() -> io::Result<()> {
        assert_eq!(render_plain(&[Block::BlankLine, paragraph("a")])?, "a\n");
        Ok(())
    }

    #[test]
    fn trailing_blank_line_keeps_exactly_one_trailing_newline() -> io::Result<()> {
        assert_eq!(render_plain(&[paragraph("a"), Block::BlankLine])?, "a\n");
        Ok(())
    }

    #[test]
    fn blank_line_between_paragraphs_still_renders_a_blank_separator_line() -> io::Result<()> {
        assert_eq!(render_plain(&[paragraph("a"), Block::BlankLine, paragraph("b")])?, "a\n\nb\n",);
        Ok(())
    }

    #[test]
    fn finish_does_not_duplicate_an_existing_trailing_newline() -> io::Result<()> {
        let blocks = vec![Block::List(List {
            kind: ListKind::Unordered,
            items: vec![list_item(None, "item")],
        })];

        assert_eq!(render_plain(&blocks)?, "• item\n");
        Ok(())
    }

    #[test]
    fn renders_an_empty_blockquote_as_a_single_bar_line() -> io::Result<()> {
        let blocks = vec![Block::BlockQuote(BlockQuote { kind: None, blocks: vec![] })];

        assert_eq!(render_plain(&blocks)?, "│\n", "bare bar without a trailing space");
        Ok(())
    }

    #[test]
    fn blockquote_ending_with_a_newline_emits_no_dangling_prefix() -> io::Result<()> {
        let blocks = vec![Block::BlockQuote(BlockQuote {
            kind: None,
            blocks: vec![Block::HtmlBlock(Text::borrowed("<div>\n"))],
        })];

        assert_eq!(render_plain(&blocks)?, "│ <div>\n");
        Ok(())
    }

    #[test]
    fn indented_child_block_emits_no_trailing_whitespace() -> io::Result<()> {
        let blocks = vec![Block::List(List {
            kind: ListKind::Unordered,
            items: vec![ListItem {
                task: None,
                blocks: vec![
                    Block::Paragraph(vec![Inline::Text(Text::borrowed("item"))]),
                    Block::CodeBlock(CodeBlock { info: None, text: Text::borrowed("code\n") }),
                ],
            }],
        })];

        let output = render_plain(&blocks)?;

        assert!(
            output.lines().all(|line| line == line.trim_end()),
            "no line may carry trailing whitespace: {output:?}"
        );
        Ok(())
    }

    #[test]
    fn default_render_options_use_the_solarized_dark_palette() {
        assert_eq!(RenderOptions::default().palette, solarized::DARK_PALETTE);
        assert_eq!(Palette::default(), solarized::DARK_PALETTE);
    }

    #[test]
    fn renderer_uses_the_palette_from_render_options() -> io::Result<()> {
        let sentinel = crate::theme::Rgb { r: 1, g: 2, b: 3 };
        let palette = Palette { heading_colors: [sentinel; 6], ..Palette::default() };
        let blocks = vec![Block::Heading(Heading {
            level: HeadingLevel::H1,
            children: vec![Inline::Text(Text::borrowed("Title"))],
        })];

        let output = render_to_string(&blocks, RenderOptions { color: ColorMode::Ansi, palette })?;

        assert!(
            output.contains("38;2;1;2;3"),
            "the custom heading color must reach the SGR output: {output:?}"
        );
        Ok(())
    }

    fn render_plain(blocks: &[Block<'_>]) -> io::Result<String> {
        render_to_string(blocks, RenderOptions::default())
    }

    fn render_ansi(blocks: &[Block<'_>]) -> io::Result<String> {
        render_to_string(blocks, RenderOptions { color: ColorMode::Ansi, ..Default::default() })
    }

    fn render_to_string(blocks: &[Block<'_>], options: RenderOptions) -> io::Result<String> {
        let renderer = Renderer::new(options);
        let mut output = Vec::new();
        let mut state = RenderState::default();
        for block in blocks {
            renderer.render_block(&mut output, block, &mut state)?;
        }
        renderer.finish(&mut output, &state)?;
        utf8(output)
    }

    fn rust_code_blocks(info: &'static str, text: &'static str) -> Vec<Block<'static>> {
        vec![Block::CodeBlock(CodeBlock {
            info: Some(Text::borrowed(info)),
            text: Text::borrowed(text),
        })]
    }

    fn assert_code_body_has_distinct_syntax_colors(output: &str) -> io::Result<()> {
        let body_line = output
            .split('\n')
            .nth(1)
            .ok_or_else(|| io::Error::other("rendered code block did not contain a body line"))?;
        let colors = ansi_rgb_colors(body_line);

        assert!(
            colors.len() > 1,
            "expected more than one syntax color in code body, got {colors:?}",
        );
        Ok(())
    }

    fn ansi_rgb_colors(text: &str) -> BTreeSet<String> {
        let mut colors = BTreeSet::new();
        for sequence in text.split("\u{1b}[").skip(1) {
            if let Some((parameters, _text)) = sequence.split_once('m')
                && let Some(color) = rgb_color_parameter(parameters)
            {
                colors.insert(color);
            }
        }
        colors
    }

    fn rgb_color_parameter(parameters: &str) -> Option<String> {
        let mut parts = parameters.split(';');
        while let Some(part) = parts.next() {
            if part == "38" && parts.next() == Some("2") {
                let red = parts.next()?;
                let green = parts.next()?;
                let blue = parts.next()?;
                return Some(format!("{red};{green};{blue}"));
            }
        }
        None
    }

    fn strip_ansi(text: &str) -> String {
        let mut output = String::with_capacity(text.len());
        let mut chars = text.chars().peekable();
        while let Some(ch) = chars.next() {
            if ch == '\u{1b}' && chars.next_if_eq(&'[').is_some() {
                for parameter in chars.by_ref() {
                    if parameter.is_ascii_alphabetic() {
                        break;
                    }
                }
            } else {
                output.push(ch);
            }
        }
        output
    }

    fn paragraph(text: &'static str) -> Block<'static> {
        Block::Paragraph(vec![Inline::Text(Text::borrowed(text))])
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
