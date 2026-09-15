use std::fmt;

use crate::types::IntType;

/// A byte range within an expression's text.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub struct Span {
    pub start: usize,
    pub end: usize,
}

impl Span {
    #[must_use]
    pub const fn new(start: usize, end: usize) -> Self {
        Self { start, end }
    }

    /// The smallest span covering both.
    #[must_use]
    pub fn to(self, other: Self) -> Self {
        Self::new(self.start.min(other.start), self.end.max(other.end))
    }
}

/// An expression that doesn't parse or doesn't typecheck.
#[derive(Debug, Clone, PartialEq, Eq, thiserror::Error)]
#[error("{message}")]
pub struct ExprError {
    pub message: String,
    /// Where in the expression text.
    pub span: Span,
    pub help: Option<String>,
}

impl ExprError {
    pub(crate) fn new(message: impl Into<String>, span: Span) -> Self {
        Self {
            message: message.into(),
            span,
            help: None,
        }
    }

    #[must_use]
    pub(crate) fn help(mut self, help: impl Into<String>) -> Self {
        self.help = Some(help.into());
        self
    }
}

/// An expression that failed while running.
///
/// Evaluation is deterministic: the same inputs fail the same way, so this is never
/// retryable. The engine halts the project at the record's version with this error
/// (ADR 0005, ADR 0007).
#[derive(Debug, Clone, PartialEq, Eq, thiserror::Error)]
#[error("{kind}")]
pub struct EvalError {
    pub kind: EvalErrorKind,
    /// The part of the expression that failed.
    pub span: Span,
}

impl EvalError {
    /// Always `false`: evaluating the same inputs again fails the same way.
    #[must_use]
    pub fn is_retryable(&self) -> bool {
        false
    }
}

#[derive(Debug, Clone, PartialEq, Eq)]
#[non_exhaustive]
pub enum EvalErrorKind {
    /// An arithmetic result doesn't fit its type, e.g. `balance - amount` below zero.
    Overflow {
        op: &'static str,
        ty: IntType,
    },
    DivideByZero,
    /// A cast like `u64(x)` got a value outside the target's range.
    CastOutOfRange {
        to: IntType,
    },
    /// The caller didn't supply an input the compiled expression reads. This is a bug
    /// in the caller, never in the user's expression.
    MissingInput(&'static str),
}

impl fmt::Display for EvalErrorKind {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::Overflow { op, ty } => {
                write!(f, "`{op}` overflowed: the result doesn't fit in {ty}")
            }
            Self::DivideByZero => f.write_str("division by zero"),
            Self::CastOutOfRange { to } => write!(f, "the value doesn't fit in {to}"),
            Self::MissingInput(what) => write!(f, "internal error: missing {what} input"),
        }
    }
}
