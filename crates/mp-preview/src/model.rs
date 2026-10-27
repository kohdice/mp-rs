//! Owned document model produced by `markdown::parse` and consumed by layout.

#[derive(Debug, PartialEq, Eq)]
#[expect(
    clippy::enum_variant_names,
    reason = "BlockQuote and CodeBlock are the CommonMark names for these blocks"
)]
pub(crate) enum Block {
    Paragraph(Vec<Inline>),
    Heading {
        level: u8,
        inlines: Vec<Inline>,
    },
    BlockQuote(Vec<Block>),
    /// A GitHub alert; `title` is the custom title, or the type's label when none is
    /// given.
    Alert {
        kind: AlertKind,
        title: String,
        blocks: Vec<Block>,
    },
    /// `start` is `Some` for ordered lists and `None` for unordered ones.
    List {
        start: Option<u64>,
        tight: bool,
        items: Vec<ListItem>,
    },
    CodeBlock {
        info: String,
        code: String,
    },
    Html(String),
    Table {
        align: Vec<Align>,
        header: Vec<Vec<Inline>>,
        rows: Vec<Vec<Vec<Inline>>>,
    },
    ThematicBreak,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) enum AlertKind {
    Note,
    Tip,
    Important,
    Warning,
    Caution,
}

impl AlertKind {
    /// The uppercase label GitHub shows when an alert has no custom title.
    pub(crate) const fn label(self) -> &'static str {
        match self {
            Self::Note => "NOTE",
            Self::Tip => "TIP",
            Self::Important => "IMPORTANT",
            Self::Warning => "WARNING",
            Self::Caution => "CAUTION",
        }
    }
}

#[derive(Debug, PartialEq, Eq)]
pub(crate) struct ListItem {
    /// `Some(checked)` for task list items, `None` for ordinary items.
    pub task: Option<bool>,
    pub blocks: Vec<Block>,
}

#[derive(Debug, PartialEq, Eq)]
pub(crate) enum Inline {
    Text(String),
    Code(String),
    Emphasis(Vec<Inline>),
    Strong(Vec<Inline>),
    Strikethrough(Vec<Inline>),
    /// `show_url` is false when the visible text already equals the URL (ignoring a
    /// leading `mailto:`), so layout does not repeat it.
    Link {
        url: String,
        title: Option<String>,
        children: Vec<Inline>,
        show_url: bool,
    },
    Image {
        url: String,
        title: Option<String>,
        alt: Vec<Inline>,
    },
    SoftBreak,
    HardBreak,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) enum Align {
    None,
    Left,
    Center,
    Right,
}
