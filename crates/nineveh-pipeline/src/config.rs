use std::time::Duration;

use nineveh_core::Version;

/// How a pipeline runs. [`PipelineConfig::new`] gives defaults sized for a live tail
/// and a single-stream backfill.
#[derive(Debug, Clone)]
#[non_exhaustive]
pub struct PipelineConfig {
    /// The first version of a new build. A build that has committed resumes from its
    /// cursor instead. Resolve `start_version: auto` before this.
    pub start: Version,
    /// Stop once this version is committed. `None` runs until shut down.
    pub until: Option<Version>,
    /// Stream responses decoding at once, per stream. Raw responses in memory are at
    /// most this many per stream.
    pub decode_tasks: usize,
    /// Backfill over several streams at once. `None` streams one range.
    pub parallel: Option<Parallel>,
    /// Decoded transactions (with records) a stream may hold ahead of the fold. Bounds
    /// memory while parallel streams read ahead; responses without records cost
    /// nothing against it.
    pub buffered_transactions: usize,
    /// Transactions with records folded into one commit, at most. While the fold is
    /// behind, decoded responses are grouped up to this; at the tip each commits as it
    /// arrives.
    pub max_batch_transactions: usize,
    /// Rows the fold's cache may hold between commits before it's dropped. Rows are
    /// loaded back from the store as needed, so this bounds memory, not correctness.
    pub cache_rows: usize,
    /// First delay before retrying after a retryable failure.
    pub backoff_initial: Duration,
    /// Longest delay between retries.
    pub backoff_max: Duration,
    /// Give up after this many consecutive failed attempts. `None` retries forever,
    /// for a service; progress resets the count.
    pub max_retries: Option<u32>,
}

impl PipelineConfig {
    #[must_use]
    pub fn new(start: Version) -> Self {
        Self {
            start,
            until: None,
            decode_tasks: 4,
            parallel: None,
            buffered_transactions: 100_000,
            max_batch_transactions: 10_000,
            cache_rows: 200_000,
            backoff_initial: Duration::from_millis(250),
            backoff_max: Duration::from_secs(30),
            max_retries: None,
        }
    }
}

/// Parallel backfill: versions from the cursor through `through` are split into
/// ranges of `chunk_versions`, streamed `streams` at a time and folded in order.
/// Versions after `through` are one stream, the live tail.
///
/// One stream covers about 3.5–11k versions a second, whatever the filter
/// (`docs/research/spike-a-stream.md`), so deep backfills need several. Set `through`
/// near the chain's current version: ranges past the tip would only wait for it.
#[derive(Debug, Clone)]
#[non_exhaustive]
pub struct Parallel {
    pub streams: usize,
    pub chunk_versions: u64,
    pub through: Version,
}

impl Parallel {
    #[must_use]
    pub fn new(streams: usize, through: Version) -> Self {
        Self {
            streams,
            chunk_versions: 1_000_000,
            through,
        }
    }
}
