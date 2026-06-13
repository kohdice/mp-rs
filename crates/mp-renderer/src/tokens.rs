//! Shared link and image display delimiters.
//!
//! These are the single vocabulary that keeps width measurement and emission in
//! agreement: a table cell is measured by rendering it in plain mode through
//! [`crate::writer::WidthMeasuringWriter`], while the styled output is produced by
//! [`crate::renderer`]. Both paths emit the same visible characters, so a column's
//! measured width matches what the styled output actually writes.

/// Opening delimiter before a link or image destination, e.g. `link(url)`.
pub(crate) const URL_OPEN: &str = "(";
/// Closing delimiter after a link or image destination.
pub(crate) const URL_CLOSE: &str = ")";
/// Separator placed between a destination and its title.
pub(crate) const TITLE_SEPARATOR: &str = " — ";
/// Prefix introducing an image's alt text, e.g. `[img: alt](url)`.
pub(crate) const IMAGE_OPEN: &str = "[img: ";
/// Closing delimiter after a link's or image's child text.
pub(crate) const LINK_TEXT_CLOSE: &str = "]";
