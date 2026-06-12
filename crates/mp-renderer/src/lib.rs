//! Block-to-terminal rendering.
//!
//! Renders [`mp_ast`] blocks as styled terminal output, one block at a time. Non-empty
//! output is guaranteed to end with exactly one trailing newline (see
//! [`Renderer::finish`]).

mod list;
mod renderer;
mod style;
mod syntax;
mod table;
mod theme;
mod tokens;
mod writer;

pub use renderer::{CodeTheme, ColorMode, RenderOptions, RenderState, Renderer};
pub use theme::{HEADING_LEVEL_COUNT, Palette, Rgb};
