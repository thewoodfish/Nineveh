use nineveh_core::Version;

/// What a pipeline is doing, published after every commit and every state change.
#[derive(Debug, Clone, Default, PartialEq, Eq)]
#[non_exhaustive]
pub struct Status {
    pub phase: Phase,
    /// The last committed version.
    pub cursor: Option<Version>,
    /// Block time of the last committed transaction, in microseconds since the Unix
    /// epoch. Lag is the wall clock minus this.
    pub timestamp_micros: Option<u64>,
    /// Versions committed since the pipeline started.
    pub versions: u64,
    /// Commits since the pipeline started.
    pub commits: u64,
    /// Consecutive failed attempts; zero once one makes progress.
    pub retries: u32,
    /// The most recent failure, retryable or not.
    pub last_error: Option<String>,
}

#[derive(Debug, Clone, Copy, Default, PartialEq, Eq)]
#[non_exhaustive]
pub enum Phase {
    /// Opening the store and the stream.
    #[default]
    Starting,
    Running,
    /// Waiting to retry after a retryable failure.
    Retrying,
    /// Stopped on a deterministic failure, at the version after the cursor.
    Halted,
    /// Stopped: shut down, or done with `until`.
    Stopped,
}
