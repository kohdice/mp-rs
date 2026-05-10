use crate::Text;

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct CodeBlock<'a> {
    pub info: Option<Text<'a>>,
    pub text: Text<'a>,
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn preserves_fenced_code_block_info_and_literal_content() {
        let block = CodeBlock {
            info: Some(Text::borrowed("mermaid")),
            text: Text::borrowed("graph TD;\nA-->B;\n"),
        };

        assert_eq!(block.info, Some(Text::borrowed("mermaid")));
        assert_eq!(block.text, Text::borrowed("graph TD;\nA-->B;\n"));
    }
}
