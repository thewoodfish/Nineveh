//! Where transactions come from: the hosted Transaction Stream, or anything else that
//! delivers the same ordered, gap-checked batches (tests use a scripted one).

use std::future::Future;
use std::sync::Arc;

use nineveh_core::Version;
use nineveh_ingest::{Batch, IngestError, StreamConfig, TransactionStream};

/// Opens streams of transactions from a given version.
///
/// The pipeline opens a new stream on every (re)start, from the version after the
/// committed cursor, so a source never has to remember where it was.
pub trait Source: Send + Sync {
    type Stream: BatchStream + 'static;

    /// Open a stream that delivers every version from `from` on, in order. With
    /// `until`, the stream may stop after it; the pipeline ignores anything later.
    fn open(
        &self,
        from: Version,
        until: Option<Version>,
    ) -> impl Future<Output = Result<Self::Stream, IngestError>> + Send;
}

/// An open stream of batches.
pub trait BatchStream: Send {
    /// The next batch, or `None` when the stream ends.
    ///
    /// Must be cancel-safe: the pipeline may drop an unfinished `next` to forward
    /// what it has already decoded, and call `next` again later, losing nothing.
    ///
    /// A batch is shared rather than owned because one read can serve several
    /// projects (ADR 0021): the shared reader hands the same `Arc` to every project
    /// that wants those versions, and each decodes it against its own lock. Nothing
    /// mutates a batch, so there is nothing to clone.
    fn next(&mut self) -> impl Future<Output = Result<Option<Arc<Batch>>, IngestError>> + Send;
}

/// A Transaction Stream endpoint, such as Aptos Labs' hosted one.
#[derive(Debug, Clone)]
pub struct StreamSource {
    config: StreamConfig,
}

impl StreamSource {
    /// Streams with `config`. Its `starting_version` and `transactions_count` are set
    /// on each open.
    #[must_use]
    pub fn new(config: StreamConfig) -> Self {
        Self { config }
    }
}

impl Source for StreamSource {
    type Stream = TransactionStream;

    async fn open(
        &self,
        from: Version,
        until: Option<Version>,
    ) -> Result<TransactionStream, IngestError> {
        let mut config = self.config.clone();
        config.starting_version = from;
        // The server counts versions covered, filtered or not.
        config.transactions_count = until
            .and_then(|until| until.get().checked_sub(from.get()))
            .and_then(|span| span.checked_add(1));
        TransactionStream::connect(config).await
    }
}

impl BatchStream for TransactionStream {
    async fn next(&mut self) -> Result<Option<Arc<Batch>>, IngestError> {
        Ok(self.next_batch().await?.map(Arc::new))
    }
}
