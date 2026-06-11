//! Shared AST data types for the mp workspace.
//!
//! `mp-parser` produces these types and `mp-renderer` consumes them; keeping the
//! vocabulary in a dedicated crate lets both sides agree on the document structure
//! without depending on each other.

mod block;
mod code;
mod inline;
mod list;
mod table;
mod text;

pub use block::{Block, BlockQuote, BlockQuoteKind, Heading};
pub use code::CodeBlock;
pub use inline::{Inline, LinkKind};
pub use list::{List, ListItem, ListKind, TaskState};
pub use table::{Alignment, Table};
pub use text::Text;
