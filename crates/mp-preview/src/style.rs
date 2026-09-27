//! Styled text as data: layout produces lines of spans; only `ansi` encodes them.

use crate::theme::Rgb;

/// `Default` is unstyled text.
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq)]
pub(crate) struct Style {
    pub fg: Option<Rgb>,
    pub bold: bool,
    pub dim: bool,
    pub italic: bool,
    pub underline: bool,
    pub strikethrough: bool,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) struct Span {
    pub text: String,
    pub style: Style,
}

pub(crate) type Line = Vec<Span>;
