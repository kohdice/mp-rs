use std::borrow::Cow;
use std::fmt;
use std::ops::Deref;

/// Textual content that borrows from the source Markdown whenever possible.
///
/// Parsing keeps string slices into the input document and only allocates when text
/// must be assembled from multiple fragments (see [`Text::push_str`]).
#[derive(Debug, Clone)]
pub struct Text<'a>(Cow<'a, str>);

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
        Self(Cow::Borrowed(text))
    }

    /// Wraps an owned string.
    #[must_use]
    pub fn owned(text: impl Into<String>) -> Self {
        Self(Cow::Owned(text.into()))
    }

    /// Returns the text as a string slice.
    #[must_use]
    pub fn as_str(&self) -> &str {
        self.0.as_ref()
    }

    /// Returns `true` if the text contains no bytes.
    #[must_use]
    pub fn is_empty(&self) -> bool {
        self.as_str().is_empty()
    }

    /// Appends `addition`, promoting borrowed text to owned only when needed.
    pub fn push_str(&mut self, addition: &str) {
        if addition.is_empty() {
            return;
        }
        self.0.to_mut().push_str(addition);
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

impl fmt::Display for Text<'_> {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        formatter.write_str(self.as_str())
    }
}

impl<'a> From<&'a str> for Text<'a> {
    fn from(text: &'a str) -> Self {
        Self::borrowed(text)
    }
}

impl From<String> for Text<'_> {
    fn from(text: String) -> Self {
        Self::owned(text)
    }
}

#[cfg(test)]
mod tests {
    use std::borrow::Cow;

    use super::Text;

    #[test]
    fn borrowed_text_uses_a_cow_borrowed_variant() {
        let text = Text::borrowed("short");

        assert!(matches!(text.0, Cow::Borrowed("short")));
    }

    #[test]
    fn appends_to_borrowed_text_by_promoting_it_to_owned_text() {
        let mut text = Text::borrowed("hello");

        text.push_str(" world");

        assert_eq!(text.as_str(), "hello world");
    }
}
