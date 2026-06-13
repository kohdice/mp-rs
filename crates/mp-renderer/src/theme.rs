use mp_ast::HeadingLevel;

/// Number of heading levels a [`Palette`] assigns colors to, tied to
/// [`HeadingLevel`] so the palette and the enum cannot drift apart.
pub const HEADING_LEVEL_COUNT: usize = HeadingLevel::COUNT;

/// A 24-bit RGB color.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct Rgb {
    /// Red channel.
    pub r: u8,
    /// Green channel.
    pub g: u8,
    /// Blue channel.
    pub b: u8,
}

/// Colors a [`crate::Renderer`] uses for each kind of content.
///
/// The default palette is solarized dark.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct Palette {
    /// Body text.
    pub body: Rgb,
    /// De-emphasized decorations such as quote bars and URL displays.
    pub muted: Rgb,
    /// List and task markers.
    pub list_marker: Rgb,
    /// Inline code spans.
    pub inline_code: Rgb,
    /// Code block fences.
    pub code_fence: Rgb,
    /// Link text.
    pub link: Rgb,
    /// Heading colors, indexed by heading depth (`H1` first).
    pub heading_colors: [Rgb; HEADING_LEVEL_COUNT],
}

impl Default for Palette {
    fn default() -> Self {
        solarized::DARK_PALETTE
    }
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
