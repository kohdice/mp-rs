//! Helpers shared by the integration tests of `mp-preview`.

use mp_preview::{ColorMode, Options, preview};

/// The plain-text output of [`preview`] for `markdown` at `width`, final newline included.
pub fn plain(markdown: &str, width: Option<usize>) -> String {
    let mut out = Vec::new();
    preview(markdown, &Options { width, color: ColorMode::Plain }, &mut out)
        .unwrap_or_else(|error| panic!("preview failed: {error}"));
    String::from_utf8(out).unwrap_or_else(|error| panic!("{error}"))
}
