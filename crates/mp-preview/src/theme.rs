//! Terminal colors: the solarized dark palette for every styled span except
//! syntax-highlighted code bodies, whose colors come from the syntect theme in
//! `highlight.rs`.

/// A 24-bit RGB color.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) struct Rgb {
    pub r: u8,
    pub g: u8,
    pub b: u8,
}

/// Colors layout uses for each kind of content.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) struct Palette {
    pub body: Rgb,
    /// De-emphasized decorations such as quote bars and URL displays.
    pub muted: Rgb,
    pub list_marker: Rgb,
    pub inline_code: Rgb,
    pub code_fence: Rgb,
    pub link: Rgb,
    /// Indexed by heading level minus one (`H1` first).
    pub heading_colors: [Rgb; 6],
}

pub(crate) mod solarized {
    use super::{Palette, Rgb};

    pub(crate) const BASE01: Rgb = Rgb { r: 0x58, g: 0x6e, b: 0x75 };
    pub(crate) const BASE0: Rgb = Rgb { r: 0x83, g: 0x94, b: 0x96 };
    pub(crate) const YELLOW: Rgb = Rgb { r: 0xb5, g: 0x89, b: 0x00 };
    pub(crate) const ORANGE: Rgb = Rgb { r: 0xcb, g: 0x4b, b: 0x16 };
    pub(crate) const VIOLET: Rgb = Rgb { r: 0x6c, g: 0x71, b: 0xc4 };
    pub(crate) const MAGENTA: Rgb = Rgb { r: 0xd3, g: 0x36, b: 0x82 };
    pub(crate) const BLUE: Rgb = Rgb { r: 0x26, g: 0x8b, b: 0xd2 };
    pub(crate) const CYAN: Rgb = Rgb { r: 0x2a, g: 0xa1, b: 0x98 };

    pub(crate) const DARK_PALETTE: Palette = Palette {
        body: BASE0,
        muted: BASE01,
        list_marker: CYAN,
        inline_code: CYAN,
        code_fence: BASE01,
        link: VIOLET,
        // Heading colors step from warm/bright to cool/muted so perceived salience
        // decreases with depth; VIOLET is the lowest-saturation accent and sits last.
        heading_colors: [YELLOW, ORANGE, MAGENTA, CYAN, BLUE, VIOLET],
    };
}

#[cfg(test)]
mod tests {
    use super::solarized;

    #[test]
    fn heading_colors_are_pairwise_distinct_and_differ_from_the_body_color() {
        let palette = solarized::DARK_PALETTE;
        for (index, color) in palette.heading_colors.iter().enumerate() {
            assert_ne!(
                *color, palette.body,
                "heading color {index} must differ from the body color"
            );
            for (other_index, other) in palette.heading_colors.iter().enumerate().skip(index + 1) {
                assert_ne!(
                    color, other,
                    "heading colors {index} and {other_index} must be distinct"
                );
            }
        }
    }
}
