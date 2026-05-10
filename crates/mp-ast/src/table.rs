use crate::Inline;

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Table<'a> {
    pub header: Vec<Vec<Inline<'a>>>,
    pub alignments: Vec<Alignment>,
    pub rows: Vec<Vec<Vec<Inline<'a>>>>,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Alignment {
    None,
    Left,
    Center,
    Right,
}
