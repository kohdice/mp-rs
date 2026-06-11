use std::io::{self, Write};

use mp_ast::{Inline, LinkKind};

use crate::list::str_width;
use crate::tokens::{IMAGE_OPEN, LINK_TEXT_CLOSE, TITLE_SEPARATOR, URL_CLOSE, URL_OPEN};

pub(crate) fn write_plain_inlines<W>(writer: &mut W, inlines: &[Inline<'_>]) -> io::Result<()>
where
    W: Write + ?Sized,
{
    for inline in inlines {
        match inline {
            Inline::Text(text) => writer.write_all(text.as_bytes())?,
            Inline::Code(text) => {
                writer.write_all(b"`")?;
                writer.write_all(text.as_bytes())?;
                writer.write_all(b"`")?;
            }
            Inline::Emphasis(children)
            | Inline::Strong(children)
            | Inline::Strikethrough(children) => write_plain_inlines(writer, children)?,
            Inline::Link { destination, title, kind, children } => {
                write_plain_inlines(writer, children)?;
                if *kind == LinkKind::Regular {
                    writer.write_all(URL_OPEN.as_bytes())?;
                    writer.write_all(destination.as_bytes())?;
                    writer.write_all(URL_CLOSE.as_bytes())?;
                    if !title.is_empty() {
                        writer.write_all(TITLE_SEPARATOR.as_bytes())?;
                        writer.write_all(title.as_bytes())?;
                    }
                }
            }
            Inline::Image { destination, title, alt } => {
                writer.write_all(IMAGE_OPEN.as_bytes())?;
                write_plain_inlines(writer, alt)?;
                writer.write_all(LINK_TEXT_CLOSE.as_bytes())?;
                writer.write_all(URL_OPEN.as_bytes())?;
                writer.write_all(destination.as_bytes())?;
                writer.write_all(URL_CLOSE.as_bytes())?;
                if !title.is_empty() {
                    writer.write_all(TITLE_SEPARATOR.as_bytes())?;
                    writer.write_all(title.as_bytes())?;
                }
            }
            Inline::HardBreak | Inline::SoftBreak => writer.write_all(b"\n")?,
        }
    }

    Ok(())
}

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

#[cfg(test)]
mod tests {
    use mp_ast::{LinkKind, Text};

    use super::*;

    #[test]
    fn plain_width_matches_written_width_for_a_link_cell_with_title() -> io::Result<()> {
        let cell = vec![Inline::Link {
            destination: Text::borrowed("https://example.com"),
            title: Text::borrowed("Example"),
            kind: LinkKind::Regular,
            children: vec![Inline::Text(Text::borrowed("link"))],
        }];

        let mut written = Vec::new();
        write_plain_inlines(&mut written, &cell)?;
        let rendered = String::from_utf8(written)
            .map_err(|error| io::Error::new(io::ErrorKind::InvalidData, error))?;

        assert_eq!(rendered, "link(https://example.com) — Example");
        assert_eq!(plain_inlines_width(&cell), str_width(&rendered));
        Ok(())
    }

    #[test]
    fn plain_width_matches_written_width_for_an_image_cell_with_title() -> io::Result<()> {
        let cell = vec![Inline::Image {
            destination: Text::borrowed("image.png"),
            title: Text::borrowed("Logo"),
            alt: vec![Inline::Text(Text::borrowed("alt"))],
        }];

        let mut written = Vec::new();
        write_plain_inlines(&mut written, &cell)?;
        let rendered = String::from_utf8(written)
            .map_err(|error| io::Error::new(io::ErrorKind::InvalidData, error))?;

        assert_eq!(rendered, "[img: alt](image.png) — Logo");
        assert_eq!(plain_inlines_width(&cell), str_width(&rendered));
        Ok(())
    }
}
