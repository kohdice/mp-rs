use std::ops::Deref;

/// Longest string stored inline by [`Text::inline`] before falling back to a
/// heap allocation; sized to hold the short fragments a parser rewrites (escaped
/// characters, HTML entities) without allocating.
const INLINE_CAPACITY: usize = 22;

#[derive(Debug, Clone)]
enum Repr<'a> {
    Borrowed(&'a str),
    Owned(String),
    Inline { len: u8, bytes: [u8; INLINE_CAPACITY] },
}

/// Textual content that borrows from the source Markdown whenever possible.
///
/// Parsing keeps string slices into the input document and only allocates when text
/// must be assembled from multiple fragments (see [`Text::push_str`]); short rewritten
/// fragments are stored inline (see [`Text::inline`]).
#[derive(Debug, Clone)]
pub struct Text<'a>(Repr<'a>);

impl<'a, 'b> PartialEq<Text<'b>> for Text<'a> {
    fn eq(&self, other: &Text<'b>) -> bool {
        self.as_str() == other.as_str()
    }
}

impl Eq for Text<'_> {}

impl<'a> Text<'a> {
    /// Wraps a string slice borrowed from the source document.
    #[must_use]
    pub const fn borrowed(text: &'a str) -> Self {
        Self(Repr::Borrowed(text))
    }

    /// Wraps an owned string.
    #[must_use]
    pub fn owned(text: impl Into<String>) -> Self {
        Self(Repr::Owned(text.into()))
    }

    /// Wraps a short string, copying it inline without a heap allocation when it
    /// fits in [`INLINE_CAPACITY`]; longer input falls back to [`Text::owned`].
    #[must_use]
    pub fn inline(text: &str) -> Self {
        let bytes = text.as_bytes();
        match u8::try_from(bytes.len()) {
            Ok(len) if bytes.len() <= INLINE_CAPACITY => {
                let mut buffer = [0; INLINE_CAPACITY];
                buffer[..bytes.len()].copy_from_slice(bytes);
                Self(Repr::Inline { len, bytes: buffer })
            }
            _ => Self::owned(text),
        }
    }

    /// Returns the text as a string slice.
    #[must_use]
    pub fn as_str(&self) -> &str {
        match &self.0 {
            Repr::Borrowed(text) => text,
            Repr::Owned(text) => text.as_str(),
            // The inline bytes always come from a `&str`, so they are valid UTF-8.
            Repr::Inline { len, bytes } => {
                std::str::from_utf8(&bytes[..usize::from(*len)]).unwrap_or("")
            }
        }
    }

    /// Returns `true` if the text contains no bytes.
    #[must_use]
    pub fn is_empty(&self) -> bool {
        self.as_str().is_empty()
    }

    /// Appends `addition`, promoting borrowed or inline text to owned only when needed.
    pub fn push_str(&mut self, addition: &str) {
        if addition.is_empty() {
            return;
        }
        if let Repr::Owned(text) = &mut self.0 {
            text.push_str(addition);
            return;
        }
        let mut owned = String::with_capacity(self.as_str().len() + addition.len());
        owned.push_str(self.as_str());
        owned.push_str(addition);
        self.0 = Repr::Owned(owned);
    }
}

impl AsRef<str> for Text<'_> {
    fn as_ref(&self) -> &str {
        self.as_str()
    }
}

impl Deref for Text<'_> {
    type Target = str;

    fn deref(&self) -> &Self::Target {
        self.as_str()
    }
}

#[cfg(test)]
mod tests {
    use super::{INLINE_CAPACITY, Repr, Text};

    #[test]
    fn borrowed_text_uses_the_borrowed_variant() {
        let text = Text::borrowed("short");

        assert!(matches!(text.0, Repr::Borrowed("short")));
    }

    #[test]
    fn short_inline_text_avoids_a_heap_allocation() {
        let text = Text::inline("a&b");

        assert!(matches!(text.0, Repr::Inline { .. }));
        assert_eq!(text.as_str(), "a&b");
    }

    #[test]
    fn inline_text_longer_than_the_capacity_falls_back_to_owned() {
        let long = "x".repeat(INLINE_CAPACITY + 1);
        let text = Text::inline(&long);

        assert!(matches!(text.0, Repr::Owned(_)));
        assert_eq!(text.as_str(), long);
    }

    #[test]
    fn appends_to_inline_text_by_promoting_it_to_owned_text() {
        let mut text = Text::inline("hello");

        text.push_str(" world");

        assert!(matches!(text.0, Repr::Owned(_)));
        assert_eq!(text.as_str(), "hello world");
    }
}
