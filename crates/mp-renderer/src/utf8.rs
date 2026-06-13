//! UTF-8 prefix recovery shared by the width-measuring and wrapping writers.

/// Returns the longest UTF-8-complete prefix of `bytes`: the whole input when it is
/// valid UTF-8, everything before a trailing incomplete sequence, or an empty string
/// when the input starts with an invalid byte.
pub(crate) fn utf8_complete_prefix(bytes: &[u8]) -> &str {
    match std::str::from_utf8(bytes) {
        Ok(text) => text,
        Err(error) => std::str::from_utf8(&bytes[..error.valid_up_to()]).unwrap_or_default(),
    }
}

#[cfg(test)]
mod tests {
    use super::utf8_complete_prefix;

    #[test]
    fn returns_the_whole_input_for_valid_utf8() {
        assert_eq!(utf8_complete_prefix(b"hello"), "hello");
        assert_eq!(utf8_complete_prefix("日本語".as_bytes()), "日本語");
        assert_eq!(utf8_complete_prefix(b""), "");
    }

    #[test]
    fn drops_a_trailing_incomplete_multibyte_sequence() {
        let bytes = "日本".as_bytes();
        // "日" is 3 bytes; cutting after 4 bytes leaves one stray continuation byte.
        assert_eq!(utf8_complete_prefix(&bytes[..4]), "日");
        assert_eq!(utf8_complete_prefix(&bytes[..3]), "日");
    }

    #[test]
    fn returns_an_empty_prefix_when_the_input_starts_with_an_invalid_byte() {
        assert_eq!(utf8_complete_prefix(&[0xFF, b'a']), "");
        assert_eq!(utf8_complete_prefix(&[0x80]), "");
    }
}
