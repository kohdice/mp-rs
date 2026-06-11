use crate::Text;

/// An inline (span-level) Markdown node.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Inline<'a> {
    /// Plain text.
    Text(Text<'a>),
    /// Emphasized (italic) children.
    Emphasis(Vec<Inline<'a>>),
    /// Strongly emphasized (bold) children.
    Strong(Vec<Inline<'a>>),
    /// Struck-through children (GFM).
    Strikethrough(Vec<Inline<'a>>),
    /// An inline code span.
    Code(Text<'a>),
    /// A hyperlink.
    Link {
        /// Link destination URL.
        destination: Text<'a>,
        /// Link title; empty when the source declares none.
        title: Text<'a>,
        /// How the link was written in the source.
        kind: LinkKind,
        /// Visible link text.
        children: Vec<Inline<'a>>,
    },
    /// An image reference.
    Image {
        /// Image destination URL.
        destination: Text<'a>,
        /// Image title; empty when the source declares none.
        title: Text<'a>,
        /// Alt text shown as the visible placeholder.
        alt: Vec<Inline<'a>>,
    },
    /// A hard line break (trailing backslash or two trailing spaces).
    HardBreak,
    /// A soft line break (a single newline in the source).
    SoftBreak,
}

/// How a link was written in the Markdown source.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum LinkKind {
    /// An inline or reference link with explicit link text.
    Regular,
    /// An autolink like `<https://example.com>`, whose text is the URL itself.
    Autolink,
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn represents_inline_nodes() {
        let inlines = vec![
            Inline::Text(Text::borrowed("text")),
            Inline::Emphasis(vec![Inline::Text(Text::borrowed("em"))]),
            Inline::Strong(vec![Inline::Text(Text::borrowed("strong"))]),
            Inline::Strikethrough(vec![Inline::Text(Text::borrowed("strike"))]),
            Inline::Code(Text::borrowed("code")),
            Inline::Link {
                destination: Text::borrowed("https://example.com"),
                title: Text::borrowed("Example"),
                kind: LinkKind::Regular,
                children: vec![Inline::Text(Text::borrowed("link"))],
            },
            Inline::Image {
                destination: Text::borrowed("image.png"),
                title: Text::borrowed("Image"),
                alt: vec![Inline::Text(Text::borrowed("alt"))],
            },
            Inline::HardBreak,
            Inline::SoftBreak,
        ];

        assert_eq!(inlines.len(), 9);
    }

    #[test]
    fn textual_fields_can_borrow_from_the_input_markdown() {
        let input = String::from("borrowed text");
        let inline = Inline::Text(Text::borrowed(input.as_str()));

        assert_eq!(inline, Inline::Text(Text::borrowed("borrowed text")));
    }
}
