//! The frontmatter block a diagram source may start with, and the title it gives.

use std::borrow::Cow;

use crate::diagram::Failure;
use crate::diagram::flowchart::label::shown_text;

use super::shape_data::scalar;
use super::syntax_error;

/// A diagram's source split at its frontmatter.
pub(in crate::diagram::flowchart) struct FrontMatter<'a> {
    /// The `title` the frontmatter gives, which upstream draws above the diagram.
    pub title: Option<String>,
    /// The source after the frontmatter.
    pub body: &'a str,
    /// The 1-based line of `source` that `body` starts on.
    pub first_line: usize,
}

/// Splits off the frontmatter `source` starts with and reads its top-level `title`,
/// mirroring `extractFrontMatter` in `packages/mermaid/src/diagram-api/frontmatter.ts`.
/// Other keys are ignored: nothing in upstream's `config` has a counterpart in
/// box-drawing text. Errors with "unclosed front matter" when the block never closes.
/// `source` uses `\n` line endings: the Markdown layer (`sanitize_block_body`)
/// normalises CR before any diagram source reaches the parser.
pub(in crate::diagram::flowchart) fn front_matter(
    source: &str,
) -> Result<FrontMatter<'_>, Failure> {
    let none = FrontMatter { title: None, body: source, first_line: 1 };
    let mut lines = source.split_inclusive('\n').zip(1..);
    let Some((first, opening)) = lines.next() else { return Ok(none) };
    let fence = first.trim_start_matches(|c: char| c.is_whitespace() && c != '\n');
    if !is_fence(fence) {
        return Ok(none);
    }
    let indent = first.get(..first.len() - fence.len()).unwrap_or_default();
    let mut offset = first.len();
    let mut title = None;
    for (line, number) in lines {
        offset += line.len();
        // The opening fence may be indented. Upstream's `frontMatterRegex`
        // (`^([^\S\n\r]*)-{3}` … `\1-{3}`) needs the closing fence to repeat that indent,
        // and the indent is removed from each line between, a line without it being read
        // as written.
        let stripped = line.strip_prefix(indent);
        if stripped.is_some_and(is_fence) {
            let body = source.get(offset..).unwrap_or_default();
            return Ok(FrontMatter { title, body, first_line: number + 1 });
        }
        if let Some(value) = title_value(stripped.unwrap_or(line)) {
            title = value;
        }
    }
    Err(syntax_error(opening, "unclosed front matter"))
}

/// Whether `line` is a frontmatter fence: `---` and nothing after it but blanks.
fn is_fence(line: &str) -> bool {
    line.strip_prefix("---").is_some_and(|after| after.trim().is_empty())
}

/// The title a frontmatter `line`, its block's indent removed, gives when it holds the
/// top-level `title` key: `Some(None)` when the value is falsy, `None` when the line
/// holds another key. Only a value on the key's line is read, not a block scalar (`|`,
/// `>`) or a plain scalar continued on the lines after it.
fn title_value(line: &str) -> Option<Option<String>> {
    let after_key = line.strip_prefix("title:")?.trim_end_matches('\n');
    // In YAML, a `:` separates a key from its value only before a blank or the line's
    // end, so `title:x` is one plain scalar and no key.
    if !(after_key.is_empty() || after_key.starts_with([' ', '\t'])) {
        return None;
    }
    let value = after_key.trim_start_matches([' ', '\t']);
    let text = match value.starts_with(['"', '\'']).then(|| scalar(value)).flatten() {
        // A quoted value is a string, falsy only when empty.
        Some((quoted, _)) => Some(quoted.text()).filter(|text| !text.is_empty()),
        // A bare value, which YAML's block context lets hold commas and braces, runs to
        // the end of the line or to a comment, a `#` after a blank (YAML 1.2 §6.6); the
        // blank after the colon counts, so `title: # note` is a comment alone. A `#`
        // right after other text, as in `Hello#tag`, is part of the value.
        None => {
            let end = after_key
                .match_indices('#')
                .find(|&(at, _)| {
                    after_key.get(..at).is_some_and(|before| before.ends_with([' ', '\t']))
                })
                .map_or(after_key.len(), |(at, _)| at);
            let plain = after_key.get(..end).unwrap_or_default().trim();
            (!is_falsy_plain_scalar(plain)).then_some(Cow::Borrowed(plain))
        }
    };
    Some(text.map(|text| shown_text(&text)))
}

/// Whether the bare YAML scalar `value` reads as a falsy JavaScript value under js-yaml's
/// `JSON_SCHEMA`, which upstream's `extractFrontMatter` loads the frontmatter with before
/// keeping the title only `if (parsed.title)`: empty, `~` or `null` (null), `false`
/// (boolean), or a number equal to 0.
fn is_falsy_plain_scalar(value: &str) -> bool {
    matches!(value, "" | "~" | "null" | "Null" | "NULL" | "false" | "False" | "FALSE")
        || value.parse::<f64>().is_ok_and(|number| number == 0.0)
}
