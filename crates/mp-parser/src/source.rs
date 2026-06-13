use std::ops::Range;

pub(crate) fn gap_has_blank_line(gap: &str) -> bool {
    let newline_count = gap.bytes().filter(|&byte| byte == b'\n').count();
    if newline_count >= 2 {
        return true;
    }

    // pulldown-cmark starts the next block after its leading indentation, so a
    // blank-line gap can collapse to a single newline followed by only spaces or
    // tabs once the previous block absorbs the first newline.
    newline_count == 1 && gap.bytes().all(|byte| matches!(byte, b'\n' | b'\r' | b' ' | b'\t'))
}

pub(crate) fn trim_trailing_blank_gap_end(source: &str, range: &Range<usize>) -> usize {
    let text = &source[range.clone()];
    if text.ends_with("\r\n\r\n") {
        range.end.saturating_sub(2)
    } else if text.ends_with("\n\n") {
        range.end.saturating_sub(1)
    } else {
        range.end
    }
}

pub(crate) fn gap_has_blockquote_blank_line(gap: &str) -> bool {
    if gap_has_blank_line(gap) {
        return true;
    }

    gap.lines().any(|line| {
        let trimmed = line.trim_start_matches([' ', '\t']);
        trimmed == ">"
            || trimmed == ">\r"
            || trimmed.strip_prefix('>').is_some_and(|rest| rest.trim().is_empty())
    })
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn detects_blank_lines_in_plain_and_blockquote_gaps() {
        assert!(gap_has_blank_line("\n\n"));
        assert!(gap_has_blockquote_blank_line(">\n"));
    }

    #[test]
    fn treats_single_newline_followed_by_only_whitespace_as_blank_line() {
        assert!(gap_has_blank_line("\n  "));
        assert!(gap_has_blank_line("\r\n\t"));
    }

    #[test]
    fn keeps_single_newline_followed_by_non_whitespace_as_no_blank_line() {
        assert!(!gap_has_blank_line("\nx"));
        assert!(!gap_has_blank_line("\n> b"));
    }
}
