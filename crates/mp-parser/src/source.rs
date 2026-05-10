use std::ops::Range;

pub(crate) fn gap_has_blank_line(gap: &str) -> bool {
    if gap == "\n" || gap == "\r\n" {
        return true;
    }

    let mut newline_count = 0;
    for byte in gap.bytes() {
        if byte == b'\n' {
            newline_count += 1;
            if newline_count >= 2 {
                return true;
            }
        }
    }
    false
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
}
