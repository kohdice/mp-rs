//! Word-wrapping writer adapter and the shared display-width primitives.

use std::io::{self, Write};

use unicode_segmentation::UnicodeSegmentation;
use unicode_width::UnicodeWidthStr;

use crate::utf8::utf8_complete_prefix;
use crate::writer::write_spaces;

/// A [`Write`] adapter that wraps UTF-8 text written through it at a maximum
/// display width, breaking lines at word boundaries.
///
/// Runs of spaces and tabs between words are kept verbatim when they fit on a
/// line and dropped only at a wrap point, so inline code spans (whose interior
/// spaces are significant) are not rewritten. Tabs count as one column.
///
/// Callers must call [`WordWrapWriter::finish`] after the last write so any
/// buffered content reaches the inner writer.
pub(crate) struct WordWrapWriter<'a, W>
where
    W: Write + ?Sized,
{
    inner: &'a mut W,
    width: usize,
    line_width: usize,
    /// Pending non-whitespace bytes of the current word (may hold zero-width
    /// ANSI escape sequences and a trailing incomplete UTF-8 sequence).
    word: Vec<u8>,
    /// Separator columns accumulated since the last word was placed; emitted as
    /// spaces before the next word when it stays on the line.
    gap: usize,
}

impl<'a, W> WordWrapWriter<'a, W>
where
    W: Write + ?Sized,
{
    pub(crate) fn new(inner: &'a mut W, width: usize) -> Self {
        Self { inner, width, line_width: 0, word: Vec::new(), gap: 0 }
    }

    /// Writes any buffered content to the inner writer and ends the wrapping.
    pub(crate) fn finish(mut self) -> io::Result<()> {
        self.place_word()
    }

    /// Emits the buffered word, preceded by the pending separator when it fits
    /// on the current line or by a newline when it does not. A word wider than
    /// the whole limit is split at grapheme boundaries across as many lines as
    /// it needs. A leading or trailing separator at a line edge is dropped.
    fn place_word(&mut self) -> io::Result<()> {
        if self.word.is_empty() {
            return Ok(());
        }
        let word_width = visible_width(&self.word);
        if self.line_width == 0 {
            self.gap = 0;
        } else if self.line_width + self.gap + word_width > self.width {
            self.inner.write_all(b"\n")?;
            self.line_width = 0;
            self.gap = 0;
        } else {
            write_spaces(&mut *self.inner, self.gap)?;
            self.line_width += self.gap;
            self.gap = 0;
        }
        if self.line_width + word_width > self.width {
            self.write_word_in_chunks()?;
        } else {
            self.inner.write_all(&self.word)?;
            self.line_width += word_width;
        }
        self.word.clear();
        Ok(())
    }

    /// Writes the buffered word split at grapheme-cluster boundaries, breaking
    /// to a new line whenever the next cluster would not fit. Every line takes
    /// at least one cluster so the writer always makes progress.
    fn write_word_in_chunks(&mut self) -> io::Result<()> {
        let mut rest = self.word.as_slice();
        while !rest.is_empty() {
            let (consumed, segment_width) = next_segment(rest);
            if segment_width > 0
                && self.line_width > 0
                && self.line_width + segment_width > self.width
            {
                self.inner.write_all(b"\n")?;
                self.line_width = 0;
            }
            self.inner.write_all(&rest[..consumed])?;
            self.line_width += segment_width;
            rest = &rest[consumed..];
        }
        Ok(())
    }
}

impl<W> Write for WordWrapWriter<'_, W>
where
    W: Write + ?Sized,
{
    fn write(&mut self, buffer: &[u8]) -> io::Result<usize> {
        // Space, tab, and newline are ASCII, so they never appear inside a
        // multibyte sequence; scanning for them byte-wise is safe even when a
        // character straddles two writes (its bytes accumulate in `word`).
        let mut rest = buffer;
        while let Some(index) = rest.iter().position(|byte| matches!(byte, b' ' | b'\t' | b'\n')) {
            self.word.extend_from_slice(&rest[..index]);
            match rest[index] {
                b'\n' => {
                    self.place_word()?;
                    self.inner.write_all(b"\n")?;
                    self.line_width = 0;
                    self.gap = 0;
                }
                // A tab counts as a single separator column; emitting it as a
                // space keeps wrap accounting and the visible output in step.
                _ => {
                    self.place_word()?;
                    self.gap += 1;
                }
            }
            rest = &rest[index + 1..];
        }
        self.word.extend_from_slice(rest);
        Ok(buffer.len())
    }

    fn flush(&mut self) -> io::Result<()> {
        self.inner.flush()
    }
}

/// Returns the unicode display width of a string, measured over whole grapheme
/// clusters so a ZWJ emoji counts once rather than per scalar. A tab counts as a
/// single column, matching how [`WordWrapWriter`] emits one.
pub(crate) fn display_width(text: &str) -> usize {
    text.width()
}

const ESCAPE: u8 = 0x1b;

/// Returns the display width of `bytes`, ignoring any trailing incomplete UTF-8
/// sequence. ANSI escape sequences count as zero width.
pub(crate) fn visible_width(bytes: &[u8]) -> usize {
    let mut rest = bytes;
    let mut width = 0;
    while !rest.is_empty() {
        if rest[0] == ESCAPE {
            rest = &rest[escape_sequence_len(rest)..];
        } else {
            let run_len = rest.iter().position(|byte| *byte == ESCAPE).unwrap_or(rest.len());
            width += display_width(utf8_complete_prefix(&rest[..run_len]));
            rest = &rest[run_len..];
        }
    }
    width
}

/// Returns the byte length and display width of the next segment of `bytes`: an
/// ANSI escape sequence (zero width), a single grapheme cluster, or the whole
/// remainder when no cluster can be decoded (counted as zero width).
fn next_segment(bytes: &[u8]) -> (usize, usize) {
    if bytes.first() == Some(&ESCAPE) {
        return (escape_sequence_len(bytes), 0);
    }
    match utf8_complete_prefix(bytes).graphemes(true).next() {
        Some(cluster) => (cluster.len(), display_width(cluster)),
        None => (bytes.len().max(1), 0),
    }
}

/// Returns the byte length of the ANSI CSI escape sequence at the start of
/// `bytes` (`ESC [`, parameter and intermediate bytes, then one final byte in
/// `@`..=`~`). An incomplete sequence consumes the whole remainder.
fn escape_sequence_len(bytes: &[u8]) -> usize {
    if bytes.get(1) != Some(&b'[') {
        return bytes.len().min(2);
    }
    bytes
        .iter()
        .enumerate()
        .skip(2)
        .find(|(_, byte)| (0x40..=0x7e).contains(*byte))
        .map_or(bytes.len(), |(index, _)| index + 1)
}

#[cfg(test)]
mod tests {
    use std::io::{self, Write};

    use super::{WordWrapWriter, display_width, visible_width};

    #[test]
    fn passes_text_within_the_width_limit_through_unchanged() -> io::Result<()> {
        let mut output = Vec::new();
        let mut wrapper = WordWrapWriter::new(&mut output, 80);

        wrapper.write_all(b"hello world")?;
        wrapper.finish()?;

        assert_eq!(String::from_utf8_lossy(&output), "hello world");
        Ok(())
    }

    #[test]
    fn wraps_at_the_last_space_before_the_limit() -> io::Result<()> {
        let mut output = Vec::new();
        let mut wrapper = WordWrapWriter::new(&mut output, 11);

        wrapper.write_all(b"hello world again")?;
        wrapper.finish()?;

        assert_eq!(String::from_utf8_lossy(&output), "hello world\nagain");
        Ok(())
    }

    #[test]
    fn preserves_interior_space_runs_that_fit_on_the_line() -> io::Result<()> {
        let mut output = Vec::new();
        let mut wrapper = WordWrapWriter::new(&mut output, 80);

        // Inline code spans rely on interior spaces surviving verbatim.
        wrapper.write_all(b"a  b   c")?;
        wrapper.finish()?;

        assert_eq!(String::from_utf8_lossy(&output), "a  b   c");
        Ok(())
    }

    #[test]
    fn drops_the_separator_only_at_the_wrap_point() -> io::Result<()> {
        let mut output = Vec::new();
        let mut wrapper = WordWrapWriter::new(&mut output, 6);

        // The interior run of spaces is kept while it fits, but the separator
        // before the word that overflows the line is dropped at the wrap.
        wrapper.write_all(b"aa  bb cc")?;
        wrapper.finish()?;

        assert_eq!(String::from_utf8_lossy(&output), "aa  bb\ncc");
        Ok(())
    }

    #[test]
    fn hard_breaks_a_word_wider_than_the_limit_at_a_grapheme_boundary() -> io::Result<()> {
        let mut output = Vec::new();
        let mut wrapper = WordWrapWriter::new(&mut output, 5);

        wrapper.write_all(b"abcdefgh end")?;
        wrapper.finish()?;

        assert_eq!(String::from_utf8_lossy(&output), "abcde\nfgh\nend");
        Ok(())
    }

    #[test]
    fn counts_fullwidth_characters_as_two_columns() -> io::Result<()> {
        let mut output = Vec::new();
        let mut wrapper = WordWrapWriter::new(&mut output, 5);

        wrapper.write_all("日本 語学".as_bytes())?;
        wrapper.finish()?;

        assert_eq!(String::from_utf8_lossy(&output), "日本\n語学");
        Ok(())
    }

    #[test]
    fn keeps_a_zwj_emoji_cluster_whole_when_it_fits() -> io::Result<()> {
        let mut output = Vec::new();
        let mut wrapper = WordWrapWriter::new(&mut output, 3);

        wrapper.write_all("👩\u{200d}💻".as_bytes())?;
        wrapper.finish()?;

        assert_eq!(String::from_utf8_lossy(&output), "👩\u{200d}💻");
        Ok(())
    }

    #[test]
    fn counts_a_tab_as_a_single_column_and_emits_it_as_a_space() -> io::Result<()> {
        let mut output = Vec::new();
        let mut wrapper = WordWrapWriter::new(&mut output, 5);

        wrapper.write_all(b"ab\tcd\tef")?;
        wrapper.finish()?;

        // "ab cd" fills 5 columns, so the next word wraps; tabs render as spaces.
        assert_eq!(String::from_utf8_lossy(&output), "ab cd\nef");
        Ok(())
    }

    #[test]
    fn passes_ansi_escape_sequences_through_as_zero_width() -> io::Result<()> {
        let mut output = Vec::new();
        let mut wrapper = WordWrapWriter::new(&mut output, 5);

        wrapper.write_all(b"\x1b[31mhello\x1b[0m world")?;
        wrapper.finish()?;

        assert_eq!(String::from_utf8_lossy(&output), "\x1b[31mhello\x1b[0m\nworld");
        Ok(())
    }

    #[test]
    fn resets_the_line_budget_after_an_explicit_newline() -> io::Result<()> {
        let mut output = Vec::new();
        let mut wrapper = WordWrapWriter::new(&mut output, 6);

        wrapper.write_all(b"abcdef\ngh ij")?;
        wrapper.finish()?;

        assert_eq!(String::from_utf8_lossy(&output), "abcdef\ngh ij");
        Ok(())
    }

    #[test]
    fn keeps_width_accounting_when_a_character_is_split_across_writes() -> io::Result<()> {
        let mut output = Vec::new();
        let mut wrapper = WordWrapWriter::new(&mut output, 2);
        let bytes = "日本".as_bytes();

        // Split mid-scalar: the first write ends inside the first character.
        wrapper.write_all(&bytes[..2])?;
        wrapper.write_all(&bytes[2..])?;
        wrapper.finish()?;

        assert_eq!(String::from_utf8_lossy(&output), "日\n本");
        Ok(())
    }

    #[test]
    fn display_width_is_grapheme_and_tab_aware() {
        assert_eq!(display_width("abc"), 3);
        assert_eq!(display_width("日本語"), 6);
        assert_eq!(display_width("e\u{301}"), 1);
        assert_eq!(display_width("👩\u{200d}💻"), 2);
        assert_eq!(display_width("a\tb"), 3);
        assert_eq!(display_width("\t"), 1);
    }

    #[test]
    fn visible_width_ignores_escape_sequences_and_matches_display_width() {
        assert_eq!(visible_width(b"\x1b[31mhello\x1b[0m"), 5);
        assert_eq!(visible_width("👩\u{200d}💻".as_bytes()), 2);
    }
}
