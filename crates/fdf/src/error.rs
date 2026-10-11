//! Source positions and parse errors.

use std::fmt;

/// 1-based line and column of a token or declaration in an FDF source.
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Default)]
pub struct Span {
    pub line: u32,
    pub col: u32,
}

/// A syntax error with its position.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ParseError {
    pub line: u32,
    pub col: u32,
    pub message: String,
}

impl fmt::Display for ParseError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(
            f,
            "line {}, column {}: {}",
            self.line, self.col, self.message
        )
    }
}

impl std::error::Error for ParseError {}

impl ParseError {
    pub(crate) fn at(span: Span, message: impl Into<String>) -> Self {
        ParseError {
            line: span.line,
            col: span.col,
            message: message.into(),
        }
    }
}
