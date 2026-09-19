//! Reading and decoding the stream, over one range of versions or several in parallel,
//! handed to the fold in version order.
//!
//! Each range has a reader task: it streams its versions, decodes each response on a
//! blocking task (a few at once, forwarded in order), and keeps only transactions
//! that have records. For a typical contract most transactions have none, so a range
//! read ahead of the fold holds little. What it holds is bounded by a budget of
//! decoded transactions per range, released as the fold consumes them. Budgets are
//! per range, so a range ahead can never starve the one the fold is waiting on.
//!
//! The [`Merger`] runs up to `streams` ranges at once and yields the front range's
//! responses until the range is complete, then moves on. A range whose stream ends
//! before covering its versions ends the merge, and the pipeline resumes from its
//! cursor: nothing is skipped.

use std::collections::VecDeque;
use std::sync::Arc;

use nineveh_config::Project;
use nineveh_core::Version;
use nineveh_decode::{DecodedTransaction, Lockfile, TransactionDecoder};
use nineveh_ingest::Batch;
use nineveh_proto::transaction::Transaction;
use tokio::sync::{OwnedSemaphorePermit, Semaphore, mpsc};
use tokio::task::JoinHandle;

use crate::config::PipelineConfig;
use crate::error::PipelineError;
use crate::source::{BatchStream, Source};

/// Responses a range may have decoded and waiting. Their size is bounded by the
/// transaction budget; this only bounds the count of empty ones.
const RANGE_CHANNEL: usize = 4096;

/// One stream response, decoded.
#[derive(Debug)]
pub(crate) struct Decoded {
    /// The transactions that have records. The rest change nothing.
    pub(crate) transactions: Vec<DecodedTransaction>,
    /// The last version the response accounts for, if any.
    pub(crate) covered: Option<Version>,
    pub(crate) timestamp_micros: Option<u64>,
    /// A transaction that didn't decode. `transactions` and `covered` stop before it.
    pub(crate) failure: Option<PipelineError>,
    /// This response's share of its range's budget, returned when it's dropped.
    _permit: Option<OwnedSemaphorePermit>,
}

/// What the merger hands the fold, in version order.
#[derive(Debug)]
pub(crate) enum Item {
    Decoded(Decoded),
    /// The stream ended; `next` is the first version it didn't cover.
    End {
        next: Version,
    },
    Failed(PipelineError),
}

/// A range of versions to stream: `last` is `None` for an open-ended tail.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) struct Range {
    pub(crate) first: Version,
    pub(crate) last: Option<Version>,
}

/// Split `from..=until` into the ranges to stream. With parallel backfill, versions up
/// to `through` go in chunks of `chunk` versions; everything after is one range.
pub(crate) fn plan(from: Version, config: &PipelineConfig) -> Vec<Range> {
    let until = config.until;
    let mut ranges = Vec::new();
    let mut first = from;
    if let Some(parallel) = &config.parallel {
        let through = until.map_or(parallel.through, |u| u.min(parallel.through));
        let chunk = parallel.chunk_versions.max(1);
        while first <= through {
            let last = Version::new(first.get().saturating_add(chunk - 1).min(through.get()));
            ranges.push(Range {
                first,
                last: Some(last),
            });
            match last.next() {
                Some(next) => first = next,
                None => return ranges,
            }
        }
    }
    if until.is_none_or(|until| first <= until) {
        ranges.push(Range { first, last: until });
    }
    ranges
}

/// Runs range readers and yields their responses in version order.
pub(crate) struct Merger<S> {
    source: Arc<S>,
    project: Arc<Project>,
    lock: Arc<Lockfile>,
    pending: VecDeque<Range>,
    active: VecDeque<Active>,
    streams: usize,
    decode_tasks: usize,
    budget: usize,
    /// The first version not yet yielded.
    next: Version,
}

struct Active {
    range: Range,
    receiver: mpsc::Receiver<Item>,
    /// The last version yielded from this range.
    covered: Option<Version>,
    _task: AbortOnDrop,
}

impl<S: Source + 'static> Merger<S> {
    pub(crate) fn new(
        source: Arc<S>,
        project: Arc<Project>,
        lock: Arc<Lockfile>,
        ranges: Vec<Range>,
        config: &PipelineConfig,
    ) -> Self {
        let streams = config.parallel.as_ref().map_or(1, |p| p.streams.max(1));
        let next = ranges.first().map_or(Version::GENESIS, |r| r.first);
        Self {
            source,
            project,
            lock,
            pending: ranges.into(),
            active: VecDeque::new(),
            streams,
            decode_tasks: config.decode_tasks.max(1),
            budget: config.buffered_transactions.max(1),
            next,
        }
    }

    /// The next response in version order. Cancel-safe: nothing is lost if the
    /// future is dropped before it completes.
    pub(crate) async fn next(&mut self) -> Item {
        loop {
            self.fill();
            let Some(front) = self.active.front_mut() else {
                return Item::End { next: self.next };
            };
            match front.receiver.recv().await {
                Some(item) => return self.take(item),
                None => {
                    if let Some(end) = self.finish_front() {
                        return end;
                    }
                }
            }
        }
    }

    /// The next response if one is ready now, for grouping into a commit. Stops at
    /// the end of the front range; [`Merger::next`] moves past it.
    pub(crate) fn try_next(&mut self) -> Option<Item> {
        let item = self.active.front_mut()?.receiver.try_recv().ok()?;
        Some(self.take(item))
    }

    fn take(&mut self, item: Item) -> Item {
        if let (Item::Decoded(decoded), Some(front)) = (&item, self.active.front_mut())
            && let Some(covered) = decoded.covered
        {
            front.covered = front.covered.max(Some(covered));
            if let Some(next) = covered.next() {
                self.next = self.next.max(next);
            }
        }
        item
    }

    /// The front range's reader is done. If it covered its range, drop it and go on;
    /// otherwise its stream ended early, and so does the merge.
    fn finish_front(&mut self) -> Option<Item> {
        let front = self.active.front()?;
        let complete = front
            .range
            .last
            .is_some_and(|last| front.covered >= Some(last));
        if complete {
            self.active.pop_front();
            None
        } else {
            Some(Item::End { next: self.next })
        }
    }

    /// Start readers until `streams` ranges are active.
    fn fill(&mut self) {
        while self.active.len() < self.streams {
            let Some(range) = self.pending.pop_front() else {
                return;
            };
            let (sender, receiver) = mpsc::channel(RANGE_CHANNEL);
            let reader = RangeReader {
                source: Arc::clone(&self.source),
                project: Arc::clone(&self.project),
                lock: Arc::clone(&self.lock),
                range,
                decode_tasks: self.decode_tasks,
                budget: Arc::new(Semaphore::new(self.budget)),
                budget_size: self.budget,
                out: sender,
            };
            self.active.push_back(Active {
                range,
                receiver,
                covered: None,
                _task: AbortOnDrop(tokio::spawn(reader.run())),
            });
        }
    }
}

/// Streams one range and decodes it.
struct RangeReader<S> {
    source: Arc<S>,
    project: Arc<Project>,
    lock: Arc<Lockfile>,
    range: Range,
    decode_tasks: usize,
    budget: Arc<Semaphore>,
    budget_size: usize,
    out: mpsc::Sender<Item>,
}

impl<S: Source> RangeReader<S> {
    async fn run(self) {
        let mut stream = match self.source.open(self.range.first, self.range.last).await {
            Ok(stream) => stream,
            Err(error) => {
                let _ = self.out.send(Item::Failed(error.into())).await;
                return;
            }
        };
        let mut decoding: VecDeque<JoinHandle<Decoded>> = VecDeque::new();
        let mut ended = false;
        let mut error = None;
        loop {
            let mut joined = None;
            while !ended && decoding.len() < self.decode_tasks {
                let next = match decoding.front_mut() {
                    None => stream.next().await,
                    // Forward the oldest response once it's decoded rather than wait
                    // for the stream to fill every decode task: at the chain's tip the
                    // next response can be a long time coming. Reading comes first, so
                    // a backfill keeps every task busy. `next` is cancel-safe.
                    Some(front) => tokio::select! {
                        biased;
                        next = stream.next() => next,
                        done = front => {
                            joined = Some(done);
                            break;
                        }
                    },
                };
                match next {
                    Ok(Some(batch)) => {
                        let (take, covered) = trim(&batch, self.range.last);
                        ended = self.range.last.is_some_and(|last| covered >= Some(last));
                        let project = Arc::clone(&self.project);
                        let lock = Arc::clone(&self.lock);
                        decoding.push_back(tokio::task::spawn_blocking(move || {
                            decode(&project, &lock, &batch.transactions[..take], covered)
                        }));
                    }
                    Ok(None) => ended = true,
                    Err(e) => {
                        // Forward what's already decoded first: it's valid.
                        error = Some(e.into());
                        ended = true;
                    }
                }
            }
            let Some(handle) = decoding.pop_front() else {
                if let Some(error) = error {
                    let _ = self.out.send(Item::Failed(error)).await;
                }
                return;
            };
            let joined = match joined {
                Some(done) => done,
                None => handle.await,
            };
            let decoded = match joined {
                Ok(decoded) => decoded,
                Err(e) => {
                    let _ = self
                        .out
                        .send(Item::Failed(PipelineError::Task(e.to_string())))
                        .await;
                    return;
                }
            };
            // Wait for budget: the fold consuming this range's earlier responses
            // returns it.
            let wanted = decoded.transactions.len().clamp(1, self.budget_size);
            let Ok(permits) = u32::try_from(wanted) else {
                return;
            };
            let Ok(permit) = Arc::clone(&self.budget).acquire_many_owned(permits).await else {
                return;
            };
            let decoded = Decoded {
                _permit: Some(permit),
                ..decoded
            };
            let failed = decoded.failure.is_some();
            if self.out.send(Item::Decoded(decoded)).await.is_err() || failed {
                return;
            }
        }
    }
}

/// A response's transactions and the last version it covers, cut at `last`.
/// How many of a batch's transactions this range wants, and the last version the
/// batch accounts for.
///
/// It reports a count rather than trimming, because the batch is shared: another
/// project reading the same `Arc` may want a different part of it (ADR 0021).
fn trim(batch: &Batch, last: Option<Version>) -> (usize, Option<Version>) {
    let transactions = &batch.transactions;
    let delivered = transactions.last().map(|tx| Version::new(tx.version));
    let mut covered = delivered.max(batch.processed_range.as_ref().map(|range| *range.end()));
    let mut take = transactions.len();
    if let Some(last) = last {
        take = transactions.partition_point(|tx| tx.version <= last.get());
        covered = covered.map(|c| c.min(last));
    }
    (take, covered)
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
        _permit: None,
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

/// Stops a reader, and with it its stream, when the merge ends.
struct AbortOnDrop(JoinHandle<()>);

impl Drop for AbortOnDrop {
    fn drop(&mut self) {
        self.0.abort();
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn ranges(
        from: u64,
        until: Option<u64>,
        parallel: Option<(usize, u64, u64)>,
    ) -> Vec<(u64, Option<u64>)> {
        let mut config = PipelineConfig::new(Version::new(0));
        config.until = until.map(Version::new);
        config.parallel =
            parallel.map(
                |(streams, chunk_versions, through)| crate::config::Parallel {
                    streams,
                    chunk_versions,
                    through: Version::new(through),
                },
            );
        plan(Version::new(from), &config)
            .into_iter()
            .map(|r| (r.first.get(), r.last.map(Version::get)))
            .collect()
    }

    #[test]
    fn one_range_without_parallel_backfill() {
        assert_eq!(ranges(10, None, None), [(10, None)]);
        assert_eq!(ranges(10, Some(20), None), [(10, Some(20))]);
    }

    #[test]
    fn chunks_up_to_the_boundary_then_a_tail() {
        assert_eq!(
            ranges(10, None, Some((4, 5, 21))),
            [(10, Some(14)), (15, Some(19)), (20, Some(21)), (22, None)]
        );
        // `until` inside the parallel part: no tail.
        assert_eq!(
            ranges(10, Some(17), Some((4, 5, 21))),
            [(10, Some(14)), (15, Some(17))]
        );
        // `until` past it: the tail stops at `until`.
        assert_eq!(
            ranges(10, Some(30), Some((4, 100, 21))),
            [(10, Some(21)), (22, Some(30))]
        );
        // Already past the boundary: just the tail.
        assert_eq!(ranges(25, None, Some((4, 5, 21))), [(25, None)]);
        // Exactly at `until`.
        assert_eq!(ranges(22, Some(21), Some((4, 5, 21))), Vec::new());
    }
}
