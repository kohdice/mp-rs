use crate::theme::{HEADING_LEVEL_COUNT, Palette, Rgb};

#[derive(Debug, Clone, Copy, Default, PartialEq, Eq)]
pub(crate) struct TextStyle {
    pub(crate) fg: Option<Rgb>,
    flags: u8,
}

impl TextStyle {
    const BOLD: u8 = 1 << 0;
    const DIM: u8 = 1 << 1;
    const ITALIC: u8 = 1 << 2;
    const UNDERLINE: u8 = 1 << 3;
    const STRIKETHROUGH: u8 = 1 << 4;

    pub(crate) fn fg(self, fg: Rgb) -> Self {
        Self { fg: Some(fg), ..self }
    }

    pub(crate) fn bold(self) -> Self {
        self.with_flag(Self::BOLD)
    }

    pub(crate) fn dim(self) -> Self {
        self.with_flag(Self::DIM)
    }

    pub(crate) fn italic(self) -> Self {
        self.with_flag(Self::ITALIC)
    }

    pub(crate) fn underline(self) -> Self {
        self.with_flag(Self::UNDERLINE)
    }

    pub(crate) fn strikethrough(self) -> Self {
        self.with_flag(Self::STRIKETHROUGH)
    }

    pub(crate) const fn is_bold(self) -> bool {
        self.has_flag(Self::BOLD)
    }

    pub(crate) const fn is_dim(self) -> bool {
        self.has_flag(Self::DIM)
    }

    pub(crate) const fn is_italic(self) -> bool {
        self.has_flag(Self::ITALIC)
    }

    pub(crate) const fn is_underline(self) -> bool {
        self.has_flag(Self::UNDERLINE)
    }

    pub(crate) const fn is_strikethrough(self) -> bool {
        self.has_flag(Self::STRIKETHROUGH)
    }

    const fn with_flag(self, flag: u8) -> Self {
        Self { flags: self.flags | flag, ..self }
    }

    const fn has_flag(self, flag: u8) -> bool {
        self.flags & flag != 0
    }
}

pub(crate) fn heading_style(level: u8, palette: Palette) -> TextStyle {
    let color = palette
        .heading_colors
        .get(usize::from(level.saturating_sub(1)))
        .copied()
        .unwrap_or(palette.heading_colors[HEADING_LEVEL_COUNT - 1]);
    match level {
        1 | 2 => TextStyle::default().fg(color).bold().underline(),
        3 | 4 => TextStyle::default().fg(color).bold(),
        6 => TextStyle::default().fg(color).dim(),
        _ => TextStyle::default().fg(color),
    }
}
