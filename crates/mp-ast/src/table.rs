use crate::Inline;

/// A GFM table.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Table<'a> {
    /// Header cells, one inline sequence per column.
    pub header: Vec<Vec<Inline<'a>>>,
    /// Per-column alignment declared in the delimiter row.
    pub alignments: Vec<Alignment>,
    /// Body rows, each holding one cell per column.
    pub rows: Vec<Vec<Vec<Inline<'a>>>>,
}

/// Column alignment declared in a table's delimiter row.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Alignment {
    /// No explicit alignment.
    None,
    /// Left-aligned (`:---`).
    Left,
    /// Center-aligned (`:---:`).
    Center,
    /// Right-aligned (`---:`).
    Right,
}
