use mp_ast::{Inline, LinkKind};

use crate::list::str_width;
use crate::tokens::{IMAGE_OPEN, LINK_TEXT_CLOSE, TITLE_SEPARATOR, URL_CLOSE, URL_OPEN};

pub(crate) fn plain_inlines_width(inlines: &[Inline<'_>]) -> usize {
    inlines.iter().map(plain_inline_width).sum()
}

fn plain_inline_width(inline: &Inline<'_>) -> usize {
    match inline {
        Inline::Text(text) => str_width(text),
        Inline::Code(text) => str_width("`") + str_width(text) + str_width("`"),
        Inline::Emphasis(children) | Inline::Strong(children) | Inline::Strikethrough(children) => {
            plain_inlines_width(children)
        }
        Inline::Link { destination, title, kind, children } => {
            let mut width = plain_inlines_width(children);
            if *kind == LinkKind::Regular {
                width += str_width(URL_OPEN) + str_width(destination) + str_width(URL_CLOSE);
                if !title.is_empty() {
                    width += str_width(TITLE_SEPARATOR) + str_width(title);
                }
            }
            width
        }
        Inline::Image { destination, title, alt } => {
            let mut width = str_width(IMAGE_OPEN)
                + plain_inlines_width(alt)
                + str_width(LINK_TEXT_CLOSE)
                + str_width(URL_OPEN)
                + str_width(destination)
                + str_width(URL_CLOSE);
            if !title.is_empty() {
                width += str_width(TITLE_SEPARATOR) + str_width(title);
            }
            width
        }
        Inline::HardBreak | Inline::SoftBreak => str_width("\n"),
    }
}
