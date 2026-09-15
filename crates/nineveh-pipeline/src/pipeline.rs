use std::future::Future;
use std::hash::{BuildHasher, RandomState};
use std::pin::Pin;
use std::sync::Arc;
use std::time::Duration;

use nineveh_config::Project;
use nineveh_core::Version;
use nineveh_decode::{DecodedTransaction, Lockfile, TransactionDecoder};
use nineveh_engine::{ChangeSet, Engine, FoldError};
use nineveh_ingest::Batch;
use nineveh_proto::transaction::Transaction;
use nineveh_store::{Loaded, Store};
use sqlx::PgPool;
use tokio::sync::{mpsc, watch};
use tokio::task::JoinHandle;
use tracing::{debug, error, info, warn};

use crate::config::PipelineConfig;
use crate::error::PipelineError;
use crate::source::{BatchStream, Source};
use crate::status::{Phase, Status};

/// Rounds of load-and-refold before a batch is declared stuck. Each round loads every
/// key the last one missed, so real batches settle in two or three.
const MAX_LOAD_ROUNDS: usize = 32;

/// How a run ended, other than by an error.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
#[non_exhaustive]
pub enum Outcome {
    /// Shut down. The next run resumes after `cursor`.
    Stopped { cursor: Option<Version> },
    /// Everything through `until` is committed.
    Finished { cursor: Option<Version> },
}

/// One project's pipeline: stream → decode → fold → commit (ADR 0005).
///
/// Decoding runs in parallel, one blocking task per stream response, and comes back
/// in order through a bounded channel, so a slow fold or database slows the stream
/// rather than filling memory. One task folds and commits. Every commit writes rows,
/// outbox and cursor together, so whenever a run fails, the next one reopens the store
/// and resumes from the committed cursor: nothing is skipped or applied twice.
#[derive(Debug)]
pub struct Pipeline<S> {
    source: S,
    pool: PgPool,
    schema: String,
    project: Arc<Project>,
    lock: Arc<Lockfile>,
    config: PipelineConfig,
    status: watch::Sender<Status>,
}

impl<S: Source> Pipeline<S> {
    /// A pipeline building `project` into `schema`, reading from `source`.
    #[must_use]
    pub fn new(
        source: S,
        pool: PgPool,
        schema: impl Into<String>,
        project: Arc<Project>,
        lock: Arc<Lockfile>,
        config: PipelineConfig,
    ) -> Self {
        Self {
            source,
            pool,
            schema: schema.into(),
            project,
            lock,
            config,
            status: watch::Sender::new(Status::default()),
        }
    }

    /// The pipeline's status, updated after every commit and phase change.
    #[must_use]
    pub fn status(&self) -> watch::Receiver<Status> {
        self.status.subscribe()
    }

    /// Run until `shutdown` completes, `until` is committed, or a failure that
    /// retrying can't fix. Retryable failures restart the run from the committed cursor
    /// after a jittered, growing delay.
    ///
    /// Shutdown is checked between commits, never inside one.
    ///
    /// # Errors
    ///
    /// A deterministic failure halts the project at its version: a record that
    /// doesn't decode, a rule that fails, or a store built from another config. Also
    /// a retryable failure that outlasts [`PipelineConfig::max_retries`].
    pub async fn run(&self, shutdown: impl Future<Output = ()>) -> Result<Outcome, PipelineError> {
        let mut shutdown = std::pin::pin!(shutdown);
        let mut retries = 0u32;
        loop {
            let before = self.status.borrow().cursor;
            self.set(|s| s.phase = Phase::Starting);
            let error = match self.attempt(shutdown.as_mut()).await {
                Ok(outcome) => {
                    self.set(|s| s.phase = Phase::Stopped);
                    return Ok(outcome);
                }
                Err(error) => error,
            };
            let give_up = |retries| self.config.max_retries.is_some_and(|max| retries > max);
            if self.status.borrow().cursor != before {
                retries = 0;
            }
            retries = retries.saturating_add(1);
            if !error.is_retryable() || give_up(retries) {
                error!(schema = %self.schema, error = %error, "pipeline halted");
                self.set(|s| {
                    s.phase = Phase::Halted;
                    s.last_error = Some(error.to_string());
                });
                return Err(error);
            }
            let delay = self.backoff(retries);
            warn!(schema = %self.schema, error = %error, retries, ?delay, "retrying");
            self.set(|s| {
                s.phase = Phase::Retrying;
                s.retries = retries;
                s.last_error = Some(error.to_string());
            });
            tokio::select! {
                biased;
                () = shutdown.as_mut() => {
                    self.set(|s| s.phase = Phase::Stopped);
                    return Ok(Outcome::Stopped { cursor: self.status.borrow().cursor });
                }
                () = tokio::time::sleep(delay) => {}
            }
        }
    }

    /// One run, from opening the store to the first failure.
    async fn attempt<F: Future<Output = ()>>(
        &self,
        mut shutdown: Pin<&mut F>,
    ) -> Result<Outcome, PipelineError> {
        let mut store =
            Store::open(self.pool.clone(), &self.schema, &self.project, &self.lock).await?;
        self.set(|s| s.cursor = store.cursor());
        let from = match store.cursor() {
            Some(cursor) => cursor.next().ok_or(PipelineError::VersionOverflow)?,
            None => self.config.start,
        };
        let until = self.config.until;
        let finished = |store: &Store| until.is_some_and(|until| store.cursor() >= Some(until));
        if finished(&store) {
            return Ok(Outcome::Finished {
                cursor: store.cursor(),
            });
        }

        let stream = tokio::select! {
            biased;
            () = shutdown.as_mut() => return Ok(Outcome::Stopped { cursor: store.cursor() }),
            stream = self.source.open(from, until) => stream?,
        };
        info!(schema = %self.schema, %from, "streaming");
        self.set(|s| s.phase = Phase::Running);

        let (sender, mut receiver) = mpsc::channel(self.config.decode_tasks.max(1));
        let reader = Reader {
            project: Arc::clone(&self.project),
            lock: Arc::clone(&self.lock),
            until,
            out: sender,
        };
        let _reader = AbortOnDrop(tokio::spawn(reader.run(stream, from)));

        let engine = Engine::new(&self.project);
        let mut cache = Loaded::default();
        let mut pending = None;
        loop {
            let item = if let Some(item) = pending.take() {
                item
            } else {
                tokio::select! {
                    biased;
                    () = shutdown.as_mut() => {
                        return Ok(Outcome::Stopped { cursor: store.cursor() });
                    }
                    item = receiver.recv() => match item {
                        Some(item) => item,
                        None if finished(&store) => {
                            return Ok(Outcome::Finished { cursor: store.cursor() });
                        }
                        None => return Err(PipelineError::Task("the stream reader stopped".into())),
                    },
                }
            };
            let first = match item {
                Item::Decoded(handle) => join(handle).await?,
                Item::End { next } if !finished(&store) => {
                    return Err(PipelineError::StreamEnded { next });
                }
                Item::End { .. } => {
                    return Ok(Outcome::Finished {
                        cursor: store.cursor(),
                    });
                }
                Item::Failed(error) => return Err(error),
            };

            // While behind, group whatever is already decoded into one commit.
            let mut count = first.transactions.len();
            let mut group = vec![first];
            while count < self.config.max_batch_transactions
                && group.last().is_some_and(|d| d.failure.is_none())
            {
                match receiver.try_recv() {
                    Ok(Item::Decoded(handle)) => {
                        let decoded = join(handle).await?;
                        count = count.saturating_add(decoded.transactions.len());
                        group.push(decoded);
                    }
                    Ok(other) => {
                        pending = Some(other);
                        break;
                    }
                    Err(_) => break,
                }
            }
            self.commit_group(&engine, &mut store, &mut cache, group)
                .await?;
            if finished(&store) {
                return Ok(Outcome::Finished {
                    cursor: store.cursor(),
                });
            }
        }
    }

    /// Fold and commit a group of decoded responses. If one of them failed to decode,
    /// or a rule fails, everything before the failing version is committed first, so
    /// the project halts exactly there.
    async fn commit_group(
        &self,
        engine: &Engine<'_>,
        store: &mut Store,
        cache: &mut Loaded,
        group: Vec<Decoded>,
    ) -> Result<(), PipelineError> {
        let mut transactions = Vec::new();
        let mut covered = None;
        let mut timestamp = None;
        let mut failure = None;
        for decoded in group {
            transactions.extend(decoded.transactions);
            covered = covered.max(decoded.covered);
            timestamp = decoded.timestamp_micros.or(timestamp);
            failure = decoded.failure;
        }
        let Some(covered) = covered else {
            return failure.map_or(Ok(()), Err);
        };

        match fold(engine, store, cache, &transactions).await {
            Ok(changes) => {
                self.commit(store, cache, changes, covered, timestamp)
                    .await?;
            }
            Err(PipelineError::Halt(halt)) => {
                let prefix = transactions.partition_point(|tx| tx.version < halt.version);
                if let Some(before) = halt.version.get().checked_sub(1).map(Version::new) {
                    let transactions = &transactions[..prefix];
                    let changes = fold(engine, store, cache, transactions).await?;
                    let timestamp = transactions.last().map(|tx| tx.timestamp_micros);
                    self.commit(store, cache, changes, before, timestamp)
                        .await?;
                }
                return Err(PipelineError::Halt(halt));
            }
            Err(error) => return Err(error),
        }
        failure.map_or(Ok(()), Err)
    }

    /// Commit a folded batch covering every version through `covered`.
    async fn commit(
        &self,
        store: &mut Store,
        cache: &mut Loaded,
        mut changes: ChangeSet,
        covered: Version,
        timestamp: Option<u64>,
    ) -> Result<(), PipelineError> {
        let first = match store.cursor() {
            Some(cursor) => cursor.next().ok_or(PipelineError::VersionOverflow)?,
            None => self.config.start,
        };
        if covered < first {
            // Nothing new: a failure at the first version of the batch.
            return Ok(());
        }
        // The batch covers versions past its last transaction with records: ones with
        // none, and under a filter, ones the server scanned without a match.
        changes.last_version = Some(covered);
        store.commit(&changes).await?;
        cache.apply(&changes);
        if cache.len() > self.config.cache_rows {
            cache.clear();
        }
        let versions = (covered.get() - first.get()).saturating_add(1);
        debug!(
            schema = %self.schema,
            cursor = %covered,
            versions,
            changes = changes.changes.len(),
            "committed"
        );
        self.set(|s| {
            s.cursor = Some(covered);
            s.timestamp_micros = timestamp.or(s.timestamp_micros);
            s.versions = s.versions.saturating_add(versions);
            s.commits = s.commits.saturating_add(1);
            s.retries = 0;
        });
        Ok(())
    }

    /// The delay before retry number `retries`: doubling from the initial delay up to
    /// the maximum, then jittered into its upper half so restarting pipelines don't
    /// reconnect in lockstep.
    fn backoff(&self, retries: u32) -> Duration {
        let factor = 1u32
            .checked_shl(retries.saturating_sub(1).min(20))
            .unwrap_or(u32::MAX);
        let delay = self
            .config
            .backoff_initial
            .saturating_mul(factor)
            .min(self.config.backoff_max);
        let nanos = u64::try_from(delay.as_nanos()).unwrap_or(u64::MAX);
        let half = nanos / 2;
        let jitter = RandomState::new().hash_one(retries) % half.saturating_add(1);
        Duration::from_nanos(half.saturating_add(jitter))
    }

    fn set(&self, update: impl FnOnce(&mut Status)) {
        self.status.send_modify(update);
    }
}

/// Fold a batch, loading the keys it reads until it has them all (ADR 0013).
async fn fold(
    engine: &Engine<'_>,
    store: &Store,
    cache: &mut Loaded,
    transactions: &[DecodedTransaction],
) -> Result<ChangeSet, PipelineError> {
    for _ in 0..MAX_LOAD_ROUNDS {
        match engine.fold(cache, transactions) {
            Ok(changes) => return Ok(changes),
            Err(FoldError::NotLoaded(keys)) => cache.merge(store.load(keys).await?),
            Err(FoldError::Halt(halt)) => return Err(PipelineError::Halt(halt)),
            Err(other) => return Err(PipelineError::Task(other.to_string())),
        }
    }
    Err(PipelineError::NotConverging {
        version: transactions
            .last()
            .map_or(Version::GENESIS, |tx| tx.version),
    })
}

/// What the reader hands the fold, in stream order.
enum Item {
    Decoded(JoinHandle<Decoded>),
    /// The stream ended; `next` is the first version it didn't cover.
    End {
        next: Version,
    },
    Failed(PipelineError),
}

/// One stream response, decoded.
struct Decoded {
    /// The transactions that have records. The rest change nothing.
    transactions: Vec<DecodedTransaction>,
    /// The last version the response accounts for, if any.
    covered: Option<Version>,
    timestamp_micros: Option<u64>,
    /// A transaction that didn't decode. `transactions` and `covered` stop before it.
    failure: Option<PipelineError>,
}

/// Reads the stream and starts decoding each response as it arrives.
struct Reader {
    project: Arc<Project>,
    lock: Arc<Lockfile>,
    until: Option<Version>,
    out: mpsc::Sender<Item>,
}

impl Reader {
    async fn run<B: BatchStream>(self, mut stream: B, mut next: Version) {
        loop {
            let item = match stream.next().await {
                Ok(Some(batch)) => {
                    let (transactions, covered) = trim(batch, self.until);
                    if let Some(after) = covered.and_then(Version::next) {
                        next = next.max(after);
                    }
                    let project = Arc::clone(&self.project);
                    let lock = Arc::clone(&self.lock);
                    Item::Decoded(tokio::task::spawn_blocking(move || {
                        decode(&project, &lock, &transactions, covered)
                    }))
                }
                Ok(None) => Item::End { next },
                Err(error) => Item::Failed(error.into()),
            };
            let done = self.until.is_some_and(|until| next > until);
            let more = matches!(item, Item::Decoded(_)) && !done;
            // The channel is bounded: this waits while the fold is behind.
            if self.out.send(item).await.is_err() || !more {
                return;
            }
        }
    }
}

/// A response's transactions and the last version it covers, cut at `until`.
fn trim(batch: Batch, until: Option<Version>) -> (Vec<Transaction>, Option<Version>) {
    let mut transactions = batch.transactions;
    let last = transactions.last().map(|tx| Version::new(tx.version));
    let mut covered = last.max(batch.processed_range.map(|range| *range.end()));
    if let Some(until) = until {
        let keep = transactions.partition_point(|tx| tx.version <= until.get());
        transactions.truncate(keep);
        covered = covered.map(|c| c.min(until));
    }
    (transactions, covered)
}

fn decode(
    project: &Project,
    lock: &Lockfile,
    transactions: &[Transaction],
    covered: Option<Version>,
) -> Decoded {
    let transaction_decoder = TransactionDecoder::new(lock, project.selection());
    let mut out = Decoded {
        transactions: Vec::new(),
        covered,
        timestamp_micros: None,
        failure: None,
    };
    for tx in transactions {
        match transaction_decoder.decode(tx) {
            Ok(decoded) => {
                out.timestamp_micros = Some(decoded.timestamp_micros);
                if !decoded.records.is_empty() {
                    out.transactions.push(decoded);
                }
            }
            Err(error) => {
                out.covered = error.version.get().checked_sub(1).map(Version::new);
                out.failure = Some(error.into());
                break;
            }
        }
    }
    out
}

async fn join(handle: JoinHandle<Decoded>) -> Result<Decoded, PipelineError> {
    handle
        .await
        .map_err(|error| PipelineError::Task(error.to_string()))
}

/// Stops the reader, and with it the stream, when a run ends.
struct AbortOnDrop(JoinHandle<()>);

impl Drop for AbortOnDrop {
    fn drop(&mut self) {
        self.0.abort();
    }
}
