//! Word-wrapping writer adapter.

use std::io::{self, Write};

use unicode_width::UnicodeWidthChar;

/// A [`Write`] adapter that wraps UTF-8 text written through it at a maximum
/// display width, breaking lines at word boundaries.
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
    word: Vec<u8>,
}

impl<'a, W> WordWrapWriter<'a, W>
where
    W: Write + ?Sized,
{
    pub(crate) fn new(inner: &'a mut W, width: usize) -> Self {
        Self { inner, width, line_width: 0, word: Vec::new() }
    }

    /// Writes any buffered content to the inner writer and ends the wrapping.
    pub(crate) fn finish(mut self) -> io::Result<()> {
        self.place_word()
    }

    /// Emits the buffered word, preceded by a space when it fits on the
    /// current line or by a newline when it does not. A word wider than the
    /// whole limit is split at character boundaries across as many lines as
    /// it needs.
    fn place_word(&mut self) -> io::Result<()> {
        if self.word.is_empty() {
            return Ok(());
        }
        let word_width = visible_width(&self.word);
        if self.line_width > 0 {
            if self.line_width + 1 + word_width > self.width {
                self.inner.write_all(b"\n")?;
                self.line_width = 0;
            } else {
                self.inner.write_all(b" ")?;
                self.line_width += 1;
            }
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

    /// Writes the buffered word split at character boundaries, breaking to a
    /// new line whenever the next character would not fit. Every line takes
    /// at least one character so the writer always makes progress.
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

/// Returns the display width of `bytes`, ignoring any trailing incomplete
/// UTF-8 sequence. ANSI escape sequences count as zero width.
pub(crate) fn visible_width(bytes: &[u8]) -> usize {
    let mut rest = bytes;
    let mut width = 0;
    while !rest.is_empty() {
        let (consumed, segment_width) = next_segment(rest);
        width += segment_width;
        rest = &rest[consumed..];
    }
    width
}

const ESCAPE: u8 = 0x1b;

/// Returns the byte length and display width of the next segment of `bytes`:
/// an ANSI escape sequence (zero width), a single character, or the whole
/// remainder when it is not valid UTF-8 (counted as zero width).
fn next_segment(bytes: &[u8]) -> (usize, usize) {
    if bytes.first() == Some(&ESCAPE) {
        return (escape_sequence_len(bytes), 0);
    }
    match std::str::from_utf8(bytes) {
        Ok(text) => match text.chars().next() {
            Some(c) => (c.len_utf8(), c.width().unwrap_or(0)),
            None => (bytes.len().max(1), 0),
        },
        Err(error) if error.valid_up_to() > 0 => {
            let c = std::str::from_utf8(&bytes[..error.valid_up_to()])
                .ok()
                .and_then(|text| text.chars().next());
            match c {
                Some(c) => (c.len_utf8(), c.width().unwrap_or(0)),
                None => (bytes.len(), 0),
            }
        }
        Err(_) => (bytes.len(), 0),
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

impl<W> Write for WordWrapWriter<'_, W>
where
    W: Write + ?Sized,
{
    fn write(&mut self, buffer: &[u8]) -> io::Result<usize> {
        for &byte in buffer {
            match byte {
                b' ' => self.place_word()?,
                b'\n' => {
                    self.place_word()?;
                    self.inner.write_all(b"\n")?;
                    self.line_width = 0;
                }
                _ => self.word.push(byte),
            }
        }
        Ok(buffer.len())
    }

    fn flush(&mut self) -> io::Result<()> {
        self.inner.flush()
    }
}

#[cfg(test)]
mod tests {
    use std::io::{self, Write};

    use super::WordWrapWriter;

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
    fn skips_consecutive_spaces_and_emits_no_trailing_spaces_at_the_wrap_point() -> io::Result<()> {
        let mut output = Vec::new();
        let mut wrapper = WordWrapWriter::new(&mut output, 11);

        wrapper.write_all(b"hello   world   again")?;
        wrapper.finish()?;

        assert_eq!(String::from_utf8_lossy(&output), "hello world\nagain");
        Ok(())
    }

    #[test]
    fn hard_breaks_a_word_wider_than_the_limit_at_a_character_boundary() -> io::Result<()> {
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
}
