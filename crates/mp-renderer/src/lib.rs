//! Block-to-terminal rendering.
//!
//! Renders [`mp_ast`] blocks as styled terminal output, one block at a time. Non-empty
//! output is guaranteed to end with exactly one trailing newline (see
//! [`Renderer::finish`]).

mod list;
mod plain;
mod renderer;
mod style;
mod syntax;
mod table;
mod theme;
mod tokens;
mod writer;

pub use renderer::{RenderOptions, RenderState, Renderer};
