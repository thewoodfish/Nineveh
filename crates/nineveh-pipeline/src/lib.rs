//! Nineveh's pipeline: a project's transactions from the stream to its committed
//! state (ADR 0005).
//!
//! ```text
//! stream ─▶ decode (parallel, in order) ─▶ fold (one task) ─▶ commit (one PG txn)
//! ```
//!
//! [`Pipeline::run`] reads a [`Source`], decodes each response on a blocking task,
//! folds decoded batches over the store's rows and commits each result with the
//! cursor. The channel between the stages is bounded, so memory stays flat whatever
//! the stream's pace.
//!
//! A deep backfill streams several disjoint version ranges at once ([`Parallel`]).
//! Each range decodes as it reads and keeps only transactions with records, within a
//! budget, and the fold takes the ranges in version order.
//!
//! Failures split two ways (`is_retryable` on every error):
//!
//! - **Retryable** (a dropped connection, the database restarting): the run ends, and
//!   the next one reopens the store and streams again from the version after the
//!   committed cursor, after a growing, jittered delay. Commits are atomic and the fold
//!   is deterministic, so the result is exactly as if nothing had failed.
//! - **Deterministic** (a record that doesn't decode, a rule that fails, a store built
//!   from another config): everything before the failing version is committed and the
//!   project halts there with a located error.
//!
//! [`Pipeline::status`] publishes the cursor, the last block time (for lag), counters
//! and the last error, for the CLI and the control plane.

mod config;
mod error;
mod filter;
mod pipeline;
mod reader;
mod source;
mod status;

pub use config::{Parallel, PipelineConfig};
pub use error::PipelineError;
pub use filter::stream_filter;
pub use pipeline::{Outcome, Pipeline};
pub use source::{BatchStream, Source, StreamSource};
pub use status::{Phase, Status};
