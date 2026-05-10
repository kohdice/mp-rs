use crate::Block;

#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct Document<'a> {
    pub blocks: Vec<Block<'a>>,
    pub has_trailing_newline: bool,
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn represents_an_empty_document() {
        let document = Document::default();

        assert_eq!(document.blocks, Vec::<Block<'_>>::new());
    }
}
