//! The properties of `style`, `classDef` and `linkStyle` statements that a text drawing
//! shows, read from their `property:value` lists as upstream Mermaid reads them
//! (<https://mermaid.js.org/syntax/flowchart.html>, "Styling and classes").

use crate::theme::Rgb;

/// What a style list sets. A property the list does not set is `None` or
/// [`ColorSetting::Unset`], so that an earlier list's value stays in place when the two
/// are combined.
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq)]
pub(super) struct Styling {
    /// `stroke`: the color of a box's border or a link's line.
    pub stroke: ColorSetting,
    /// `color`: the color of a label.
    pub color: ColorSetting,
    /// `fill`: the background of a box's interior and label.
    pub fill: ColorSetting,
    /// Whether `stroke-width` makes borders and lines heavy.
    pub heavy: Option<bool>,
    /// Whether `stroke-dasharray` makes borders and lines dotted.
    pub dotted: Option<bool>,
    /// Whether `font-weight` makes a label bold.
    pub bold: Option<bool>,
    /// Whether `font-style` makes a label italic.
    pub italic: Option<bool>,
}

impl Styling {
    /// Reads the comma-separated `property:value` list `list`.
    pub(super) fn parse(list: &str) -> Self {
        let mut styling = Self::default();
        for declaration in list.split(',') {
            let Some((property, value)) = declaration.split_once(':') else { continue };
            // CSS reads property names and keywords in any ASCII case, and hex digits and
            // color names are case-insensitive too, so the declaration is read lowercased.
            let property = property.trim().to_ascii_lowercase();
            let value = without_important(value.trim()).to_ascii_lowercase();
            let value = value.as_str();
            match property.as_str() {
                // Upstream draws no border for `none` or `transparent`; a box without a
                // border would lose the drawing's structure, so they reset it to the
                // default color instead.
                "stroke" => styling.stroke = styling.stroke.then(paint(value, &RESETS)),
                // `transparent` hides upstream's text; text that cannot be seen would lose
                // the label, so it resets the default color instead. `none` is no `color`
                // value in CSS, which drops the declaration.
                "color" => styling.color = styling.color.then(paint(value, &["transparent"])),
                "fill" => styling.fill = styling.fill.then(paint(value, &RESETS)),
                "stroke-width" => styling.heavy = is_heavy(value).or(styling.heavy),
                "stroke-dasharray" => styling.dotted = Some(is_dashed(value)),
                "font-weight" => styling.bold = is_bold(value).or(styling.bold),
                "font-style" => styling.italic = is_italic(value).or(styling.italic),
                // Upstream hands every property to the SVG; a terminal cell has no size,
                // font, opacity, corner radius or filter, so the rest are read and dropped.
                _ => {}
            }
        }
        styling
    }

    /// `self` with what `later` sets in its place, as CSS's last declaration wins.
    pub(super) fn then(self, later: Self) -> Self {
        Self {
            stroke: self.stroke.then(later.stroke),
            color: self.color.then(later.color),
            fill: self.fill.then(later.fill),
            heavy: later.heavy.or(self.heavy),
            dotted: later.dotted.or(self.dotted),
            bold: later.bold.or(self.bold),
            italic: later.italic.or(self.italic),
        }
    }
}

/// What a style list does to one color property.
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq)]
pub(super) enum ColorSetting {
    /// The list does not set it, so an earlier list's value stays in place.
    #[default]
    Unset,
    /// The list sets it to no color, overriding an earlier one: the drawing's default.
    Reset,
    /// The list sets it to this color.
    Color(Rgb),
}

impl ColorSetting {
    /// `self` with `later` in its place unless `later` leaves the property unset.
    fn then(self, later: Self) -> Self {
        if later == Self::Unset { self } else { later }
    }

    /// The color set, or `None` when unset or reset.
    pub(super) fn rgb(self) -> Option<Rgb> {
        match self {
            Self::Color(rgb) => Some(rgb),
            Self::Unset | Self::Reset => None,
        }
    }
}

/// `value` without a trailing `!important`. Upstream's browser lets an important
/// declaration win over later ones; lists here combine in order alone, so the flag is
/// dropped and the declaration read as any other.
fn without_important(value: &str) -> &str {
    let flag = "!important";
    value
        .len()
        .checked_sub(flag.len())
        .filter(|&at| value.get(at..).is_some_and(|end| end.eq_ignore_ascii_case(flag)))
        .and_then(|at| value.get(..at))
        .map_or(value, str::trim_end)
}

/// The values that set `stroke` and `fill` to no color.
const RESETS: [&str; 2] = ["none", "transparent"];

/// What the color property value `value` sets: [`ColorSetting::Reset`] for one of
/// `resets`, the color it names, or nothing for a value that is no color, which CSS
/// drops.
fn paint(value: &str, resets: &[&str]) -> ColorSetting {
    if resets.iter().any(|reset| reset.eq_ignore_ascii_case(value)) {
        return ColorSetting::Reset;
    }
    color(value).map_or(ColorSetting::Unset, ColorSetting::Color)
}

/// Whether the `stroke-width` `value`, in pixels with or without the `px` unit, draws
/// heavy lines. Upstream draws a line as wide as the value; a cell holds only a light
/// line or a heavy one, so widths from 3px, wider than upstream's 1px and 2px defaults,
/// draw heavy and narrower ones light. `None` for a value in another unit or no number.
fn is_heavy(value: &str) -> Option<bool> {
    let pixels: f64 = value.strip_suffix("px").unwrap_or(value).trim().parse().ok()?;
    Some(pixels >= 3.0)
}

/// Whether the `stroke-dasharray` `value` dashes lines: anything but empty, `none` or
/// dashes of no length. Upstream draws the dash and gap lengths the value lists; the
/// box-drawing glyphs have one dashed form for each line, so every pattern draws as it.
fn is_dashed(value: &str) -> bool {
    let none = value.is_empty() || value.eq_ignore_ascii_case("none");
    let zero = |length: &str| length.trim_end_matches("px").parse::<f64>() == Ok(0.0);
    !(none || value.split([' ', ',']).filter(|length| !length.is_empty()).all(zero))
}

/// Whether the `font-weight` `value` is bold: `bold`, `bolder` or a weight from 700,
/// the weight `bold` stands for; `None` for a value that is no weight.
fn is_bold(value: &str) -> Option<bool> {
    match value {
        "bold" | "bolder" => Some(true),
        "normal" | "lighter" => Some(false),
        _ => value.parse::<f64>().ok().map(|weight| weight >= 700.0),
    }
}

/// Whether the `font-style` `value` slants the text; `None` for a value that is no
/// style.
fn is_italic(value: &str) -> Option<bool> {
    match value {
        "italic" | "oblique" => Some(true),
        "normal" => Some(false),
        _ => None,
    }
}

/// The color `value` names: `#rgb`, `#rgba`, `#rrggbb` or `#rrggbbaa`, whose alpha a
/// terminal cannot show and which is dropped, or a CSS named color in any case. `None`
/// for anything else.
fn color(value: &str) -> Option<Rgb> {
    let Some(hex) = value.strip_prefix('#') else {
        return NAMED_COLORS
            .iter()
            .find(|(name, _)| name.eq_ignore_ascii_case(value))
            .map(|&(_, rgb)| rgb_of(rgb));
    };
    if !hex.bytes().all(|byte| byte.is_ascii_hexdigit()) {
        return None;
    }
    let digit = |at: usize| u8::from_str_radix(hex.get(at..=at)?, 16).ok();
    let pair = |at: usize| u8::from_str_radix(hex.get(at..at + 2)?, 16).ok();
    match hex.len() {
        3 | 4 => Some(Rgb { r: digit(0)? * 17, g: digit(1)? * 17, b: digit(2)? * 17 }),
        6 | 8 => Some(Rgb { r: pair(0)?, g: pair(2)?, b: pair(4)? }),
        _ => None,
    }
}

/// The color `0xRRGGBB`.
const fn rgb_of(hex: u32) -> Rgb {
    let [_, r, g, b] = hex.to_be_bytes();
    Rgb { r, g, b }
}

/// The named colors of CSS Color Module Level 4
/// (<https://www.w3.org/TR/css-color-4/#named-colors>), which browsers draw upstream's
/// style values with; `transparent` is not among them.
const NAMED_COLORS: [(&str, u32); 148] = [
    ("aliceblue", 0xf0f8ff),
    ("antiquewhite", 0xfaebd7),
    ("aqua", 0x00ffff),
    ("aquamarine", 0x7fffd4),
    ("azure", 0xf0ffff),
    ("beige", 0xf5f5dc),
    ("bisque", 0xffe4c4),
    ("black", 0x000000),
    ("blanchedalmond", 0xffebcd),
    ("blue", 0x0000ff),
    ("blueviolet", 0x8a2be2),
    ("brown", 0xa52a2a),
    ("burlywood", 0xdeb887),
    ("cadetblue", 0x5f9ea0),
    ("chartreuse", 0x7fff00),
    ("chocolate", 0xd2691e),
    ("coral", 0xff7f50),
    ("cornflowerblue", 0x6495ed),
    ("cornsilk", 0xfff8dc),
    ("crimson", 0xdc143c),
    ("cyan", 0x00ffff),
    ("darkblue", 0x00008b),
    ("darkcyan", 0x008b8b),
    ("darkgoldenrod", 0xb8860b),
    ("darkgray", 0xa9a9a9),
    ("darkgreen", 0x006400),
    ("darkgrey", 0xa9a9a9),
    ("darkkhaki", 0xbdb76b),
    ("darkmagenta", 0x8b008b),
    ("darkolivegreen", 0x556b2f),
    ("darkorange", 0xff8c00),
    ("darkorchid", 0x9932cc),
    ("darkred", 0x8b0000),
    ("darksalmon", 0xe9967a),
    ("darkseagreen", 0x8fbc8f),
    ("darkslateblue", 0x483d8b),
    ("darkslategray", 0x2f4f4f),
    ("darkslategrey", 0x2f4f4f),
    ("darkturquoise", 0x00ced1),
    ("darkviolet", 0x9400d3),
    ("deeppink", 0xff1493),
    ("deepskyblue", 0x00bfff),
    ("dimgray", 0x696969),
    ("dimgrey", 0x696969),
    ("dodgerblue", 0x1e90ff),
    ("firebrick", 0xb22222),
    ("floralwhite", 0xfffaf0),
    ("forestgreen", 0x228b22),
    ("fuchsia", 0xff00ff),
    ("gainsboro", 0xdcdcdc),
    ("ghostwhite", 0xf8f8ff),
    ("gold", 0xffd700),
    ("goldenrod", 0xdaa520),
    ("gray", 0x808080),
    ("green", 0x008000),
    ("greenyellow", 0xadff2f),
    ("grey", 0x808080),
    ("honeydew", 0xf0fff0),
    ("hotpink", 0xff69b4),
    ("indianred", 0xcd5c5c),
    ("indigo", 0x4b0082),
    ("ivory", 0xfffff0),
    ("khaki", 0xf0e68c),
    ("lavender", 0xe6e6fa),
    ("lavenderblush", 0xfff0f5),
    ("lawngreen", 0x7cfc00),
    ("lemonchiffon", 0xfffacd),
    ("lightblue", 0xadd8e6),
    ("lightcoral", 0xf08080),
    ("lightcyan", 0xe0ffff),
    ("lightgoldenrodyellow", 0xfafad2),
    ("lightgray", 0xd3d3d3),
    ("lightgreen", 0x90ee90),
    ("lightgrey", 0xd3d3d3),
    ("lightpink", 0xffb6c1),
    ("lightsalmon", 0xffa07a),
    ("lightseagreen", 0x20b2aa),
    ("lightskyblue", 0x87cefa),
    ("lightslategray", 0x778899),
    ("lightslategrey", 0x778899),
    ("lightsteelblue", 0xb0c4de),
    ("lightyellow", 0xffffe0),
    ("lime", 0x00ff00),
    ("limegreen", 0x32cd32),
    ("linen", 0xfaf0e6),
    ("magenta", 0xff00ff),
    ("maroon", 0x800000),
    ("mediumaquamarine", 0x66cdaa),
    ("mediumblue", 0x0000cd),
    ("mediumorchid", 0xba55d3),
    ("mediumpurple", 0x9370db),
    ("mediumseagreen", 0x3cb371),
    ("mediumslateblue", 0x7b68ee),
    ("mediumspringgreen", 0x00fa9a),
    ("mediumturquoise", 0x48d1cc),
    ("mediumvioletred", 0xc71585),
    ("midnightblue", 0x191970),
    ("mintcream", 0xf5fffa),
    ("mistyrose", 0xffe4e1),
    ("moccasin", 0xffe4b5),
    ("navajowhite", 0xffdead),
    ("navy", 0x000080),
    ("oldlace", 0xfdf5e6),
    ("olive", 0x808000),
    ("olivedrab", 0x6b8e23),
    ("orange", 0xffa500),
    ("orangered", 0xff4500),
    ("orchid", 0xda70d6),
    ("palegoldenrod", 0xeee8aa),
    ("palegreen", 0x98fb98),
    ("paleturquoise", 0xafeeee),
    ("palevioletred", 0xdb7093),
    ("papayawhip", 0xffefd5),
    ("peachpuff", 0xffdab9),
    ("peru", 0xcd853f),
    ("pink", 0xffc0cb),
    ("plum", 0xdda0dd),
    ("powderblue", 0xb0e0e6),
    ("purple", 0x800080),
    ("rebeccapurple", 0x663399),
    ("red", 0xff0000),
    ("rosybrown", 0xbc8f8f),
    ("royalblue", 0x4169e1),
    ("saddlebrown", 0x8b4513),
    ("salmon", 0xfa8072),
    ("sandybrown", 0xf4a460),
    ("seagreen", 0x2e8b57),
    ("seashell", 0xfff5ee),
    ("sienna", 0xa0522d),
    ("silver", 0xc0c0c0),
    ("skyblue", 0x87ceeb),
    ("slateblue", 0x6a5acd),
    ("slategray", 0x708090),
    ("slategrey", 0x708090),
    ("snow", 0xfffafa),
    ("springgreen", 0x00ff7f),
    ("steelblue", 0x4682b4),
    ("tan", 0xd2b48c),
    ("teal", 0x008080),
    ("thistle", 0xd8bfd8),
    ("tomato", 0xff6347),
    ("turquoise", 0x40e0d0),
    ("violet", 0xee82ee),
    ("wheat", 0xf5deb3),
    ("white", 0xffffff),
    ("whitesmoke", 0xf5f5f5),
    ("yellow", 0xffff00),
    ("yellowgreen", 0x9acd32),
];

#[cfg(test)]
mod tests {
    use super::*;

    const RED: Rgb = Rgb { r: 255, g: 0, b: 0 };
    const GREEN: Rgb = Rgb { r: 0, g: 255, b: 0 };
    const PINK: Rgb = Rgb { r: 255, g: 153, b: 255 };

    #[test]
    fn styling_parse_reads_hex_and_named_colors_and_skips_other_values() {
        let stroke = |value: &str| Styling::parse(&format!("stroke:{value}")).stroke;

        assert_eq!(stroke("#f0f"), ColorSetting::Color(Rgb { r: 255, g: 0, b: 255 }));
        assert_eq!(stroke("#ff0000"), ColorSetting::Color(RED));
        assert_eq!(stroke("#ff000080"), ColorSetting::Color(RED));
        assert_eq!(stroke("red"), ColorSetting::Color(RED));
        assert_eq!(stroke("SteelBlue"), ColorSetting::Color(Rgb { r: 70, g: 130, b: 180 }));
        assert_eq!(stroke("notacolor"), ColorSetting::Unset);
    }

    #[test]
    fn styling_parse_none_and_transparent_reset_stroke_and_fill() {
        for value in ["none", "transparent"] {
            let reset = Styling::parse(&format!("stroke:{value},fill:{value}"));
            assert_eq!((reset.stroke, reset.fill), (ColorSetting::Reset, ColorSetting::Reset));

            let earlier = Styling::parse("stroke:#f00,fill:#f9f");
            let combined = earlier.then(reset);
            assert_eq!((combined.stroke.rgb(), combined.fill.rgb()), (None, None), "{value}");
        }
    }

    #[test]
    fn styling_parse_color_transparent_resets_and_color_none_is_ignored() {
        let earlier = Styling::parse("color:#00ff00");

        assert_eq!(Styling::parse("color:transparent").color, ColorSetting::Reset);
        assert_eq!(earlier.then(Styling::parse("color:transparent")).color.rgb(), None);
        assert_eq!(Styling::parse("color:none").color, ColorSetting::Unset);
        assert_eq!(earlier.then(Styling::parse("color:none")).color.rgb(), Some(GREEN));
    }

    #[test]
    fn styling_parse_reads_properties_and_keywords_in_any_case() {
        for (bold, italic) in
            [("font-weight:bold", "font-style:italic"), ("FONT-WEIGHT:Bold", "Font-Style:ITALIC")]
        {
            let styling = Styling::parse(&format!("{bold},{italic}"));

            assert_eq!((styling.bold, styling.italic), (Some(true), Some(true)), "{bold},{italic}");
        }
        assert_eq!(Styling::parse("Fill:Red").fill, ColorSetting::Color(RED));
        assert_eq!(Styling::parse("stroke-dasharray:None").dotted, Some(false));
    }

    #[test]
    fn styling_parse_ignores_unknown_properties() {
        assert_eq!(
            Styling::parse("font-size:12pt,stroke:#ff0000"),
            Styling { stroke: ColorSetting::Color(RED), ..Styling::default() }
        );
    }

    #[test]
    fn styling_parse_drops_an_important_flag() {
        assert_eq!(Styling::parse("fill:#f9f !important").fill, ColorSetting::Color(PINK));
    }

    #[test]
    fn styling_parse_non_zero_stroke_dasharray_is_dotted() {
        let dotted = |value: &str| Styling::parse(&format!("stroke-dasharray:{value}")).dotted;

        assert_eq!(dotted("5 5"), Some(true));
        assert_eq!(dotted("5"), Some(true));
        assert_eq!(dotted("none"), Some(false));
        assert_eq!(dotted("0 0"), Some(false));
        assert_eq!(
            Styling::parse("stroke-dasharray:5")
                .then(Styling::parse("stroke-dasharray:none"))
                .dotted,
            Some(false)
        );
    }
}
