//! Keeping decoded text from driving the terminal.

/// Replaces a C0 control or DEL with its Unicode Control Picture and a C1 control with
/// U+FFFD, which stands in for the C1 pictures Unicode lacks, so decoded input cannot
/// drive the terminal. Any other character is returned unchanged.
pub(crate) fn visualize_control(character: char) -> char {
    match character {
        '\0'..='\x1f' => char::from_u32(0x2400 + u32::from(character)).unwrap_or(character),
        '\x7f' => '\u{2421}',
        '\u{80}'..='\u{9f}' => char::REPLACEMENT_CHARACTER,
        _ => character,
    }
}
