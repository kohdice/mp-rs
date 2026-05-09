use std::io::{self, Write};

pub fn write_preview<W>(mut writer: W) -> io::Result<()>
where
    W: Write,
{
    writeln!(writer, "Hello, world!")
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn writes_preview_text() -> io::Result<()> {
        let mut output = Vec::new();

        write_preview(&mut output)?;

        assert_eq!(output, b"Hello, world!\n");
        Ok(())
    }
}
