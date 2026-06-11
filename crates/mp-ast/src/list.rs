use crate::Block;

/// An ordered or unordered list.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct List<'a> {
    /// Whether the list is ordered, and where its numbering starts.
    pub kind: ListKind,
    /// List items in source order.
    pub items: Vec<ListItem<'a>>,
}

/// The flavor of a list.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ListKind {
    /// A bullet list.
    Unordered,
    /// A numbered list.
    Ordered {
        /// Number of the first item.
        start: u64,
    },
}

/// A single list item.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ListItem<'a> {
    /// Checkbox state when the item is a GFM task-list item.
    pub task: Option<TaskState>,
    /// Block content of the item.
    pub blocks: Vec<Block<'a>>,
}

/// Checkbox state of a GFM task-list item.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum TaskState {
    /// The checkbox is checked (`[x]`).
    Checked,
    /// The checkbox is unchecked (`[ ]`).
    Unchecked,
}
