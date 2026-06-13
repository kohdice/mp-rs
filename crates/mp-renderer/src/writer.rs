use std::io::{self, Write};

use unicode_width::UnicodeWidthStr;

use crate::utf8::utf8_complete_prefix;

/// A [`Write`] sink that discards its bytes and accumulates the unicode display width of
/// the UTF-8 text written through it. Measuring a cell by rendering it through this
/// adapter keeps width measurement and emission on one code path, so they cannot drift.
#[derive(Debug, Default)]
pub(crate) struct WidthMeasuringWriter {
    width: usize,
    carry: Vec<u8>,
}

impl WidthMeasuringWriter {
    pub(crate) fn width(&self) -> usize {
        self.width
    }
}

impl Write for WidthMeasuringWriter {
    fn write(&mut self, buffer: &[u8]) -> io::Result<usize> {
        if self.carry.is_empty() {
            // Common case: measure straight from the caller's buffer and stash only a
            // trailing incomplete sequence (at most 3 bytes), avoiding a full copy.
            let (valid_up_to, width) = utf8_prefix_width(buffer);
            self.width += width;
            self.carry.extend_from_slice(&buffer[valid_up_to..]);
        } else {
            self.carry.extend_from_slice(buffer);
            let (valid_up_to, width) = utf8_prefix_width(&self.carry);
            self.width += width;
            self.carry.drain(..valid_up_to);
        }
        Ok(buffer.len())
    }

    fn flush(&mut self) -> io::Result<()> {
        Ok(())
    }
}

/// Returns the length of the longest UTF-8-complete prefix of `bytes` and its unicode
/// display width; any trailing incomplete scalar sequence is excluded from both.
fn utf8_prefix_width(bytes: &[u8]) -> (usize, usize) {
    let text = utf8_complete_prefix(bytes);
    (text.len(), text.width())
}

pub(crate) struct LinePrefixWriter<'a, W, P>
where
    W: Write + ?Sized,
    P: FnMut(&mut W, bool) -> io::Result<()>,
{
    inner: &'a mut W,
    prefix: P,
    at_line_start: bool,
    wrote_anything: bool,
    skip_next_prefix: bool,
}

impl<'a, W, P> LinePrefixWriter<'a, W, P>
where
    W: Write + ?Sized,
    P: FnMut(&mut W, bool) -> io::Result<()>,
{
    /// Creates a writer that emits `prefix` at the start of every line. The prefix
    /// closure receives `true` when the line's only content is its terminating newline,
    /// so callers can drop trailing whitespace (e.g. a bare `│` for blank quote lines).
    /// With `hanging`, the first line is not prefixed, so the caller can place that
    /// line's leading content itself (a hanging indent).
    pub(crate) fn new(inner: &'a mut W, prefix: P, hanging: bool) -> Self {
        Self {
            inner,
            prefix,
            at_line_start: true,
            wrote_anything: false,
            skip_next_prefix: hanging,
        }
    }

    pub(crate) fn wrote_anything(&self) -> bool {
        self.wrote_anything
    }

    fn write_prefix(&mut self, blank_line: bool) -> io::Result<()> {
        if self.skip_next_prefix {
            self.skip_next_prefix = false;
        } else {
            (self.prefix)(self.inner, blank_line)?;
            self.wrote_anything = true;
        }
        self.at_line_start = false;
        Ok(())
    }
}

impl<W, P> Write for LinePrefixWriter<'_, W, P>
where
    W: Write + ?Sized,
    P: FnMut(&mut W, bool) -> io::Result<()>,
{
    fn write(&mut self, buffer: &[u8]) -> io::Result<usize> {
        if buffer.is_empty() {
            return Ok(0);
        }

        if self.at_line_start {
            self.write_prefix(buffer[0] == b'\n')?;
        }

        let end =
            buffer.iter().position(|byte| *byte == b'\n').map_or(buffer.len(), |index| index + 1);
        let written = self.inner.write(&buffer[..end])?;
        if written > 0 {
            self.at_line_start = buffer[written - 1] == b'\n';
        }

        Ok(written)
    }

    fn flush(&mut self) -> io::Result<()> {
        self.inner.flush()
    }
}

pub(crate) fn write_spaces<W>(writer: &mut W, count: usize) -> io::Result<()>
where
    W: Write + ?Sized,
{
    const SPACES: &[u8] = b"                                                                ";

    write_chunked_repeats(writer, SPACES, 1, count)
}

pub(crate) fn write_repeated_str<W>(writer: &mut W, text: &str, count: usize) -> io::Result<()>
where
    W: Write + ?Sized,
{
    const BUFFER_SIZE: usize = 256;

    if text.is_empty() || count == 0 {
        return Ok(());
    }

    let text = text.as_bytes();
    if text.len() > BUFFER_SIZE {
        for _ in 0..count {
            writer.write_all(text)?;
        }
        return Ok(());
    }

    let chunk_len = (BUFFER_SIZE / text.len()) * text.len();
    let mut buffer = [0; BUFFER_SIZE];
    for slot in buffer[..chunk_len].chunks_exact_mut(text.len()) {
        slot.copy_from_slice(text);
    }

    write_chunked_repeats(writer, &buffer[..chunk_len], text.len(), count)
}

/// Writes `count` repetitions of a `unit`-byte pattern, where `chunk` holds whole
/// repetitions of that pattern (`chunk.len()` is a non-zero multiple of `unit`).
fn write_chunked_repeats<W>(
    writer: &mut W,
    chunk: &[u8],
    unit: usize,
    mut count: usize,
) -> io::Result<()>
where
    W: Write + ?Sized,
{
    let repeats_per_chunk = chunk.len() / unit;
    while count >= repeats_per_chunk {
        writer.write_all(chunk)?;
        count -= repeats_per_chunk;
    }
    if count > 0 {
        writer.write_all(&chunk[..count * unit])?;
    }

    Ok(())
}

#[cfg(test)]
mod tests {
    use std::io::{self, Write};

    use super::{LinePrefixWriter, WidthMeasuringWriter, write_repeated_str};

    #[test]
    fn width_measuring_writer_measures_ascii_and_fullwidth_text() -> io::Result<()> {
        let mut counter = WidthMeasuringWriter::default();

        counter.write_all("a日b".as_bytes())?;

        assert_eq!(counter.width(), 4);
        Ok(())
    }

    #[test]
    fn width_measuring_writer_handles_utf8_split_across_writes() -> io::Result<()> {
        let mut counter = WidthMeasuringWriter::default();
        let bytes = "日本語".as_bytes();

        // Split mid-scalar: the first write ends inside the second character.
        counter.write_all(&bytes[..4])?;
        counter.write_all(&bytes[4..])?;

        assert_eq!(counter.width(), 6);
        Ok(())
    }

    #[test]
    fn line_prefix_writer_reports_partial_input_writes() -> io::Result<()> {
        let mut output = ShortWriter::new(2);
        let mut prefixed =
            LinePrefixWriter::new(&mut output, |writer, _blank| writer.write_all(b"> "), false);

        let written = prefixed.write(b"abcdef")?;

        assert_eq!(written, 2);
        assert_eq!(output.bytes, b"> ab");
        Ok(())
    }

    #[test]
    fn line_prefix_writer_prefixes_all_lines_with_write_all() -> io::Result<()> {
        let mut output = Vec::new();
        let mut prefixed =
            LinePrefixWriter::new(&mut output, |writer, _blank| writer.write_all(b"> "), false);

        prefixed.write_all(b"a\nb")?;

        assert_eq!(String::from_utf8_lossy(&output), "> a\n> b");
        Ok(())
    }

    #[test]
    fn appends_no_prefix_after_a_trailing_newline() -> io::Result<()> {
        let mut output = Vec::new();
        let mut prefixed =
            LinePrefixWriter::new(&mut output, |writer, _blank| writer.write_all(b"> "), false);

        prefixed.write_all(b"a\n")?;

        assert_eq!(String::from_utf8_lossy(&output), "> a\n");
        Ok(())
    }

    #[test]
    fn hanging_writer_skips_the_prefix_on_the_first_line_only() -> io::Result<()> {
        let mut output = Vec::new();
        let mut prefixed =
            LinePrefixWriter::new(&mut output, |writer, _blank| writer.write_all(b"  "), true);

        prefixed.write_all(b"a\nb\nc")?;

        assert_eq!(String::from_utf8_lossy(&output), "a\n  b\n  c");
        Ok(())
    }

    #[test]
    fn writes_nothing_when_unused() {
        let mut output = Vec::new();
        let _prefixed =
            LinePrefixWriter::new(&mut output, |writer, _blank| writer.write_all(b"> "), false);

        assert!(output.is_empty(), "an unused writer must not emit an orphaned prefix");
    }

    #[test]
    fn writes_repeated_text_in_chunks_without_changing_output() -> io::Result<()> {
        let mut output = Vec::new();

        write_repeated_str(&mut output, "─", 100)?;

        assert_eq!(String::from_utf8_lossy(&output), "─".repeat(100));
        Ok(())
    }

    #[test]
    fn writes_large_repeated_text_that_does_not_fit_the_stack_buffer() -> io::Result<()> {
        let mut output = Vec::new();
        let text = "x".repeat(300);

        write_repeated_str(&mut output, &text, 2)?;

        assert_eq!(String::from_utf8_lossy(&output), text.repeat(2));
        Ok(())
    }

    struct ShortWriter {
        bytes: Vec<u8>,
        max_write_len: usize,
    }

    impl ShortWriter {
        fn new(max_write_len: usize) -> Self {
            Self { bytes: Vec::new(), max_write_len }
        }
    }

    impl Write for ShortWriter {
        fn write(&mut self, buffer: &[u8]) -> io::Result<usize> {
            let written = buffer.len().min(self.max_write_len);
            self.bytes.extend_from_slice(&buffer[..written]);
            Ok(written)
        }

        fn flush(&mut self) -> io::Result<()> {
            Ok(())
        }
    }
}
