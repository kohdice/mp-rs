use crate::Block;

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct List<'a> {
    pub kind: ListKind,
    pub items: Vec<ListItem<'a>>,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ListKind {
    Unordered,
    Ordered { start: u64 },
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ListItem<'a> {
    pub task: Option<TaskState>,
    pub blocks: Vec<Block<'a>>,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum TaskState {
    Checked,
    Unchecked,
}
