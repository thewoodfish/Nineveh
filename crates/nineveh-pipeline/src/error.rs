use nineveh_core::Version;
use nineveh_decode::DecodeError;
use nineveh_engine::Halt;
use nineveh_ingest::IngestError;
use nineveh_store::StoreError;

/// Why the pipeline stopped, or why one attempt at running it failed.
#[derive(Debug, thiserror::Error)]
#[non_exhaustive]
pub enum PipelineError {
    #[error(transparent)]
    Ingest(#[from] IngestError),

    /// A selected record didn't decode. The project halts at its version.
    #[error(transparent)]
    Decode(#[from] DecodeError),

    /// A rule failed. The project halts at its version.
    #[error("halted at {0}")]
    Halt(Box<Halt>),

    #[error(transparent)]
    Store(#[from] StoreError),

    /// The stream ended before the pipeline was done with it. Reconnecting resumes.
    #[error("the Transaction Stream ended before version {next}")]
    StreamEnded { next: Version },

    #[error("the version counter overflowed u64")]
    VersionOverflow,

    /// Folding kept asking for keys that loading should have provided.
    #[error("internal error: folding the batch ending at {version} didn't converge")]
    NotConverging { version: Version },

    #[error("internal error: a decode task failed: {0}")]
    Task(String),

    /// The decoder produced a record for a source the config doesn't have. Dropping it
    /// would leave a hole in the record log and so corrupt a later replay (ADR 0022).
    #[error("internal error: version {version} has a record from unknown source #{id}")]
    RecordSourceUnknown { version: Version, id: u32 },
}

impl PipelineError {
    /// Whether restarting from the committed cursor can succeed. Deterministic failures
    /// (bad data, a failing rule, a store built from another config) fail the same way
    /// again, so the project halts on them instead (ADR 0005).
    #[must_use]
    pub fn is_retryable(&self) -> bool {
        match self {
            Self::Ingest(e) => e.is_retryable(),
            Self::Decode(e) => e.is_retryable(),
            Self::Store(e) => e.is_retryable(),
            Self::StreamEnded { .. } => true,
            Self::Halt(_)
            | Self::VersionOverflow
            | Self::NotConverging { .. }
            | Self::RecordSourceUnknown { .. }
            | Self::Task(_) => false,
        }
    }

    /// The version the project halted at, for a deterministic failure in the data.
    #[must_use]
    pub fn halted_at(&self) -> Option<Version> {
        match self {
            Self::Decode(e) => Some(e.version),
            Self::Halt(h) => Some(h.version),
            _ => None,
        }
    }
}
