use std::fmt;

use nineveh_config::Span;
use nineveh_core::Version;
use nineveh_decode::Origin;

use crate::state::{Key, TableId};

/// Why a batch didn't fold.
#[derive(Debug, Clone, PartialEq, Eq, thiserror::Error)]
#[non_exhaustive]
pub enum FoldError {
    /// The view didn't hold these keys. Load them and fold the same batch again:
    /// folding is pure, so a retry is safe.
    #[error("the state view is missing {} key(s) the batch reads", .0.len())]
    NotLoaded(Vec<(TableId, Key)>),

    /// A deterministic failure. The project halts at this version until the config
    /// is fixed; retrying would fail the same way (ADR 0005).
    #[error(transparent)]
    Halt(Box<Halt>),
}

impl FoldError {
    /// Whether folding the same batch again can succeed: only after loading the keys
    /// a [`FoldError::NotLoaded`] names.
    #[must_use]
    pub fn is_retryable(&self) -> bool {
        matches!(self, Self::NotLoaded(_))
    }
}

/// A located, deterministic failure.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Halt {
    pub version: Version,
    pub origin: Option<Origin>,
    /// The state table whose rule failed, if one did.
    pub table: Option<String>,
    /// The rule's index in its table's `reduce` list.
    pub rule: Option<usize>,
    pub message: String,
    /// Where in `nineveh.yaml` the failing expression is.
    pub span: Option<Span>,
}

impl fmt::Display for Halt {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(f, "version {}", self.version)?;
        if let Some(origin) = self.origin {
            write!(f, ", {origin}")?;
        }
        if let Some(table) = &self.table {
            write!(f, ", table `{table}`")?;
        }
        if let Some(rule) = self.rule {
            write!(f, ", rule {}", rule + 1)?;
        }
        write!(f, ": {}", self.message)
    }
}

impl std::error::Error for Halt {}
