//! Inline flattening, display widths, and word wrapping.

use unicode_segmentation::UnicodeSegmentation;
use unicode_width::UnicodeWidthStr;

use crate::model::Inline;
use crate::style::{Line, Span, Style};
use crate::theme::solarized::DARK_PALETTE;

const URL_OPEN: &str = "(";
const URL_CLOSE: &str = ")";
const TITLE_SEPARATOR: &str = " — ";
const IMAGE_OPEN: &str = "[img: ";
const IMAGE_CLOSE: &str = "]";

/// Lays out inlines in `width` columns, or unwrapped when `width` is `None`; without a
/// width a soft break starts a new line, with one it becomes a space. No inlines lay
/// out to no lines, so an empty heading or paragraph contributes nothing.
pub(crate) fn lay_out_inlines(inlines: &[Inline], style: Style, width: Option<usize>) -> Vec<Line> {
    if inlines.is_empty() {
        return Vec::new();
    }
    let segments = flatten(inlines, style, width.is_none());
    match width {
        Some(width) => wrap(&segments, width),
        None => segments,
    }
}

/// Wraps each flattened line to `width` columns.
pub(super) fn wrap(segments: &[Line], width: usize) -> Vec<Line> {
    let mut wrapper = Wrapper::new(width);
    for segment in segments {
        for span in segment {
            wrapper.push_text(&span.text, span.style);
        }
        wrapper.end_line();
    }
    wrapper.lines
}

/// Returns the lines separated by hard breaks (and by soft breaks when
/// `soft_break_is_newline`); otherwise a soft break becomes a space.
pub(super) fn flatten(inlines: &[Inline], style: Style, soft_break_is_newline: bool) -> Vec<Line> {
    let mut flattener = Flattener { lines: Vec::new(), line: Line::new(), soft_break_is_newline };
    flattener.push_inlines(inlines, style);
    flattener.lines.push(flattener.line);
    flattener.lines
}

struct Flattener {
    lines: Vec<Line>,
    line: Line,
    soft_break_is_newline: bool,
}

impl Flattener {
    fn push_inlines(&mut self, inlines: &[Inline], style: Style) {
        for inline in inlines {
            match inline {
                Inline::Text(text) => push_span(&mut self.line, text, style),
                Inline::Code(code) => {
                    let code_style = Style { fg: Some(DARK_PALETTE.inline_code), ..style };
                    push_span(&mut self.line, "`", code_style);
                    push_span(&mut self.line, code, code_style);
                    push_span(&mut self.line, "`", code_style);
                }
                Inline::Emphasis(children) => {
                    self.push_inlines(children, Style { italic: true, ..style });
                }
                Inline::Strong(children) => {
                    self.push_inlines(children, Style { bold: true, ..style });
                }
                Inline::Strikethrough(children) => {
                    self.push_inlines(children, Style { strikethrough: true, ..style });
                }
                Inline::Link { url, title, children, show_url } => {
                    let link = Style { fg: Some(DARK_PALETTE.link), underline: true, ..style };
                    self.push_inlines(children, link);
                    if *show_url {
                        self.push_url(url);
                    }
                    self.push_title(title.as_deref());
                }
                Inline::Image { url, title, alt } => {
                    let alt_style = Style { fg: Some(DARK_PALETTE.muted), italic: true, ..style };
                    push_span(&mut self.line, IMAGE_OPEN, alt_style);
                    self.push_inlines(alt, alt_style);
                    push_span(&mut self.line, IMAGE_CLOSE, alt_style);
                    self.push_url(url);
                    self.push_title(title.as_deref());
                }
                Inline::SoftBreak if !self.soft_break_is_newline => {
                    push_span(&mut self.line, " ", style);
                }
                Inline::SoftBreak | Inline::HardBreak => {
                    self.lines.push(std::mem::take(&mut self.line));
                }
            }
        }
    }

    fn push_url(&mut self, url: &str) {
        let url_style = destination_style();
        push_span(&mut self.line, URL_OPEN, url_style);
        push_span(&mut self.line, url, url_style);
        push_span(&mut self.line, URL_CLOSE, url_style);
    }

    fn push_title(&mut self, title: Option<&str>) {
        if let Some(title) = title {
            let title_style = Style { italic: true, ..destination_style() };
            push_span(&mut self.line, TITLE_SEPARATOR, title_style);
            push_span(&mut self.line, title, title_style);
        }
    }
}

/// The base style of a link or image URL and its title, which is the URL style in italics.
fn destination_style() -> Style {
    Style { fg: Some(DARK_PALETTE.muted), dim: true, ..Style::default() }
}

/// Greedy word wrapper. Each word is measured once, as a whole string, when it is
/// complete, so wrapping stays linear in the input size.
struct Wrapper {
    width: usize,
    lines: Vec<Line>,
    line: Line,
    line_width: usize,
    /// Separator columns seen since the last placed word; emitted only between two
    /// words on the same line.
    gap: usize,
    gap_style: Style,
    /// Text of the pending word, which may span several styles.
    word: String,
    /// Style runs of `word` as (end byte offset, style).
    word_runs: Vec<(usize, Style)>,
}

impl Wrapper {
    fn new(width: usize) -> Self {
        Self {
            width,
            lines: Vec::new(),
            line: Line::new(),
            line_width: 0,
            gap: 0,
            gap_style: Style::default(),
            word: String::new(),
            word_runs: Vec::new(),
        }
    }

    fn push_text(&mut self, text: &str, style: Style) {
        let mut rest = text;
        while let Some(index) = rest.find(' ') {
            self.push_word_part(&rest[..index], style);
            self.place_word();
            if self.gap == 0 {
                self.gap_style = style;
            }
            self.gap += 1;
            rest = &rest[index + 1..];
        }
        self.push_word_part(rest, style);
    }

    fn push_word_part(&mut self, text: &str, style: Style) {
        if text.is_empty() {
            return;
        }
        self.word.push_str(text);
        match self.word_runs.last_mut() {
            Some((end, run_style)) if *run_style == style => *end = self.word.len(),
            _ => self.word_runs.push((self.word.len(), style)),
        }
    }

    fn place_word(&mut self) {
        if self.word.is_empty() {
            return;
        }
        let word_width = self.word.width();
        if self.line_width > 0 {
            if self.line_width + self.gap + word_width > self.width {
                self.break_line();
            } else {
                for _ in 0..self.gap {
                    push_span(&mut self.line, " ", self.gap_style);
                }
                self.line_width += self.gap;
            }
        }
        if word_width > self.width {
            self.place_oversized_word();
        } else {
            let mut start = 0;
            for &(end, style) in &self.word_runs {
                push_span(&mut self.line, &self.word[start..end], style);
                start = end;
            }
            self.line_width += word_width;
        }
        self.gap = 0;
        self.word.clear();
        self.word_runs.clear();
    }

    /// Splits the pending word at grapheme cluster boundaries, starting on an empty
    /// line. Cluster widths are summed, which is conservative: it may break earlier
    /// than whole-string measurement would. Every line takes at least one cluster.
    fn place_oversized_word(&mut self) {
        let mut start = 0;
        for &(end, style) in &self.word_runs {
            for cluster in self.word[start..end].graphemes(true) {
                let cluster_width = cluster.width();
                if self.line_width > 0 && self.line_width + cluster_width > self.width {
                    self.lines.push(std::mem::take(&mut self.line));
                    self.line_width = 0;
                }
                push_span(&mut self.line, cluster, style);
                self.line_width += cluster_width;
            }
            start = end;
        }
    }

    fn end_line(&mut self) {
        self.place_word();
        self.break_line();
        self.gap = 0;
    }

    fn break_line(&mut self) {
        self.lines.push(std::mem::take(&mut self.line));
        self.line_width = 0;
    }
}

/// Appends `text`, merging it into the last span when the style is unchanged.
pub(super) fn push_span(line: &mut Line, text: &str, style: Style) {
    match line.last_mut() {
        Some(last) if last.style == style => last.text.push_str(text),
        _ => line.push(Span { text: text.to_owned(), style }),
    }
}
