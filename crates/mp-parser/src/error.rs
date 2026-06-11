use std::borrow::Cow;

/// An inconsistency detected in the underlying Markdown event stream.
///
/// Every constructor site is defensive: well-formed input never produces this error, so
/// it surfaces only if pulldown-cmark yields a structurally invalid event sequence.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ParseError {
    message: Cow<'static, str>,
}

impl ParseError {
    pub(crate) fn new(message: impl Into<Cow<'static, str>>) -> Self {
        Self { message: message.into() }
    }
}

impl std::fmt::Display for ParseError {
    fn fmt(&self, formatter: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        formatter.write_str(self.message.as_ref())
    }
}

impl std::error::Error for ParseError {}
