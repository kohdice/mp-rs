use crate::Text;

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Inline<'a> {
    Text(Text<'a>),
    Emphasis(Vec<Inline<'a>>),
    Strong(Vec<Inline<'a>>),
    Strikethrough(Vec<Inline<'a>>),
    Code(Text<'a>),
    Link { destination: Text<'a>, title: Text<'a>, kind: LinkKind, children: Vec<Inline<'a>> },
    Image { destination: Text<'a>, title: Text<'a>, alt: Vec<Inline<'a>> },
    HardBreak,
    SoftBreak,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum LinkKind {
    Regular,
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
