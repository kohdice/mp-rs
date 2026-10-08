//! Styled text as data: layout produces lines of spans; only `ansi` encodes them, in
//! the [`ColorMode`] the caller chooses.

use crate::theme::Rgb;

/// Whether output carries ANSI escape sequences.
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq)]
pub enum ColorMode {
    /// 24-bit color and SGR attributes; code blocks are syntax highlighted.
    Ansi,
    /// Text only, with no escape sequences.
    #[default]
    Plain,
}

/// `Default` is unstyled text.
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq)]
pub(crate) struct Style {
    pub fg: Option<Rgb>,
    pub bg: Option<Rgb>,
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
