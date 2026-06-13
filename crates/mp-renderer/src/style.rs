use mp_ast::HeadingLevel;

use crate::theme::{Palette, Rgb};

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

    pub(crate) const fn fg(self, fg: Rgb) -> Self {
        Self { fg: Some(fg), ..self }
    }

    pub(crate) const fn bold(self) -> Self {
        self.with_flag(Self::BOLD)
    }

    pub(crate) const fn dim(self) -> Self {
        self.with_flag(Self::DIM)
    }

    pub(crate) const fn italic(self) -> Self {
        self.with_flag(Self::ITALIC)
    }

    pub(crate) const fn underline(self) -> Self {
        self.with_flag(Self::UNDERLINE)
    }

    pub(crate) const fn strikethrough(self) -> Self {
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

pub(crate) fn heading_style(level: HeadingLevel, palette: Palette) -> TextStyle {
    let color = palette.heading_colors[usize::from(level.depth() - 1)];
    match level {
        HeadingLevel::H1 | HeadingLevel::H2 => TextStyle::default().fg(color).bold().underline(),
        HeadingLevel::H3 | HeadingLevel::H4 => TextStyle::default().fg(color).bold(),
        HeadingLevel::H5 | HeadingLevel::H6 => TextStyle::default().fg(color),
    }
}

#[cfg(test)]
mod tests {
    use mp_ast::HeadingLevel;

    use super::heading_style;
    use crate::theme::solarized;

    #[test]
    fn heading_style_applies_decreasing_emphasis_per_level() {
        let palette = solarized::DARK_PALETTE;

        let h1 = heading_style(HeadingLevel::H1, palette);
        assert!(h1.is_bold() && h1.is_underline(), "H1 should be bold and underlined");

        let h2 = heading_style(HeadingLevel::H2, palette);
        assert!(h2.is_bold() && h2.is_underline(), "H2 should be bold and underlined");

        let h3 = heading_style(HeadingLevel::H3, palette);
        assert!(h3.is_bold() && !h3.is_underline(), "H3 should be bold without underline");

        let h4 = heading_style(HeadingLevel::H4, palette);
        assert!(h4.is_bold() && !h4.is_underline(), "H4 should be bold without underline");

        let h5 = heading_style(HeadingLevel::H5, palette);
        assert!(
            !h5.is_bold() && !h5.is_underline() && !h5.is_dim(),
            "H5 should be plain color only"
        );

        let h6 = heading_style(HeadingLevel::H6, palette);
        assert!(
            !h6.is_bold() && !h6.is_underline() && !h6.is_dim(),
            "H6 should be plain color only"
        );
        let shallower_levels = [
            HeadingLevel::H1,
            HeadingLevel::H2,
            HeadingLevel::H3,
            HeadingLevel::H4,
            HeadingLevel::H5,
        ];
        for level in shallower_levels {
            assert_ne!(
                h6.fg,
                heading_style(level, palette).fg,
                "H6 color must differ from the {level:?} color"
            );
        }
        assert_ne!(h6.fg, Some(palette.body), "H6 color must differ from the body color");
    }
}
