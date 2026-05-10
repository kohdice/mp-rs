pub(crate) const HEADING_LEVEL_COUNT: usize = 6;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) struct Rgb {
    pub(crate) r: u8,
    pub(crate) g: u8,
    pub(crate) b: u8,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) struct Palette {
    pub(crate) body: Rgb,
    pub(crate) muted: Rgb,
    pub(crate) list_marker: Rgb,
    pub(crate) inline_code: Rgb,
    pub(crate) code_fence: Rgb,
    pub(crate) link: Rgb,
    pub(crate) heading_colors: [Rgb; HEADING_LEVEL_COUNT],
}

pub(crate) mod solarized {
    use super::{Palette, Rgb};

    pub(crate) const BASE01: Rgb = Rgb { r: 0x58, g: 0x6e, b: 0x75 };
    pub(crate) const BASE0: Rgb = Rgb { r: 0x83, g: 0x94, b: 0x96 };
    pub(crate) const YELLOW: Rgb = Rgb { r: 0xb5, g: 0x89, b: 0x00 };
    pub(crate) const ORANGE: Rgb = Rgb { r: 0xcb, g: 0x4b, b: 0x16 };
    pub(crate) const VIOLET: Rgb = Rgb { r: 0x6c, g: 0x71, b: 0xc4 };
    pub(crate) const BLUE: Rgb = Rgb { r: 0x26, g: 0x8b, b: 0xd2 };
    pub(crate) const CYAN: Rgb = Rgb { r: 0x2a, g: 0xa1, b: 0x98 };

    pub(crate) const DARK_PALETTE: Palette = Palette {
        body: BASE0,
        muted: BASE01,
        list_marker: CYAN,
        inline_code: CYAN,
        code_fence: BASE01,
        link: VIOLET,
        heading_colors: [YELLOW, ORANGE, BLUE, CYAN, VIOLET, VIOLET],
    };
}

#[cfg(test)]
mod tests {
    use super::{Rgb, solarized};

    #[test]
    fn default_palette_uses_solarized_dark_values() {
        assert_eq!(solarized::BASE01, Rgb { r: 88, g: 110, b: 117 });
        assert_eq!(solarized::BASE0, Rgb { r: 131, g: 148, b: 150 });
        assert_eq!(solarized::YELLOW, Rgb { r: 181, g: 137, b: 0 });
        assert_eq!(solarized::CYAN, Rgb { r: 42, g: 161, b: 152 });
        assert_eq!(solarized::VIOLET, Rgb { r: 108, g: 113, b: 196 });
        assert_eq!(solarized::DARK_PALETTE.body, solarized::BASE0);
        assert_eq!(solarized::DARK_PALETTE.muted, solarized::BASE01);
        assert_eq!(solarized::DARK_PALETTE.heading_colors[0], solarized::YELLOW);
    }
}
