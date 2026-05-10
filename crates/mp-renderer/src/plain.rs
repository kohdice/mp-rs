use std::io::{self, Write};

use mp_ast::{Inline, LinkKind};

use crate::list::str_width;

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
                    writer.write_all(b"(")?;
                    writer.write_all(destination.as_bytes())?;
                    writer.write_all(b")")?;
                    if !title.is_empty() {
                        writer.write_all(" — ".as_bytes())?;
                        writer.write_all(title.as_bytes())?;
                    }
                }
            }
            Inline::Image { destination, title, alt } => {
                writer.write_all(b"[img: ")?;
                write_plain_inlines(writer, alt)?;
                writer.write_all(b"](")?;
                writer.write_all(destination.as_bytes())?;
                writer.write_all(b")")?;
                if !title.is_empty() {
                    writer.write_all(" — ".as_bytes())?;
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
                width += str_width("(") + str_width(destination) + str_width(")");
                if !title.is_empty() {
                    width += str_width(" — ") + str_width(title);
                }
            }
            width
        }
        Inline::Image { destination, title, alt } => {
            let mut width = str_width("[img: ")
                + plain_inlines_width(alt)
                + str_width("](")
                + str_width(destination)
                + str_width(")");
            if !title.is_empty() {
                width += str_width(" — ") + str_width(title);
            }
            width
        }
        Inline::HardBreak | Inline::SoftBreak => str_width("\n"),
    }
}
