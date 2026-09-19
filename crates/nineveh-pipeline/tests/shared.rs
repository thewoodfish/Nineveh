//! The shared reader: one stream per network, many projects (ADR 0021).
//!
//! Every test here is really the same assertion in a different situation — that the
//! versions a project sees are contiguous, in order and delivered once. The shared
//! reader is the only place in Nineveh where one project's transactions come from a
//! connection it doesn't own, so it is the only place a gap could be introduced
//! silently, and a gap would corrupt state with no error anywhere.

#![allow(
    clippy::unwrap_used,
    clippy::panic,
    reason = "test-only crate: helpers panic on unexpected results"
)]

use std::future::{Future, ready};
use std::sync::Arc;
use std::sync::atomic::{AtomicUsize, Ordering};
use std::time::Duration;

use nineveh_core::{ChainId, Version};
use nineveh_ingest::{Batch, IngestError};
use nineveh_pipeline::{BatchStream, SharedTip, Source};
use nineveh_proto::transaction::Transaction;
use tokio::sync::watch;

/// Versions a scripted batch carries.
const BATCH_SIZE: u64 = 4;

/// A chain that grows on demand, counting how many streams were opened over it.
///
/// The count is the point of the whole design, so it is what most of these tests
/// assert on.
struct Chain {
    height: watch::Sender<u64>,
    opens: AtomicUsize,
}

impl Chain {
    fn new(height: u64) -> Arc<Self> {
        Arc::new(Self {
            height: watch::channel(height).0,
            opens: AtomicUsize::new(0),
        })
    }

    /// Advance the chain's tip, as new transactions being committed would.
    fn grow_to(&self, height: u64) {
        self.height.send_replace(height);
    }

    fn opens(&self) -> usize {
        self.opens.load(Ordering::Relaxed)
    }
}

impl Source for Chain {
    type Stream = ChainStream;

    fn open(
        &self,
        from: Version,
        until: Option<Version>,
    ) -> impl Future<Output = Result<ChainStream, IngestError>> + Send {
        self.opens.fetch_add(1, Ordering::Relaxed);
        ready(Ok(ChainStream {
            height: self.height.subscribe(),
            next: from.get(),
            until: until.map(Version::get),
        }))
    }
}

struct ChainStream {
    height: watch::Receiver<u64>,
    next: u64,
    until: Option<u64>,
}

impl BatchStream for ChainStream {
    async fn next(&mut self) -> Result<Option<Arc<Batch>>, IngestError> {
        loop {
            if self.until.is_some_and(|until| self.next > until) {
                return Ok(None);
            }
            let tip = *self.height.borrow();
            if self.next > tip {
                // At the chain's tip, as a live stream is: wait, don't end.
                if self.height.changed().await.is_err() {
                    return Ok(None);
                }
                continue;
            }
            let last = (self.next + BATCH_SIZE - 1)
                .min(tip)
                .min(self.until.unwrap_or(u64::MAX));
            let transactions: Vec<Transaction> = (self.next..=last)
                .map(|version| Transaction {
                    version,
                    ..Transaction::default()
                })
                .collect();
            self.next = last + 1;
            return Ok(Some(Arc::new(Batch {
                chain_id: ChainId::try_from(2u64).unwrap(),
                transactions,
                processed_range: None,
            })));
        }
    }
}

/// Read versions from a stream until it has delivered through `through`, checking as
/// it goes that they are contiguous from `from` — no gap, no repeat, no reordering.
async fn read_through(
    stream: &mut impl BatchStream,
    from: u64,
    through: u64,
    what: &str,
) -> Vec<u64> {
    let mut seen = Vec::new();
    let mut expected = from;
    while seen.last().copied() != Some(through) {
        let batch = tokio::time::timeout(Duration::from_secs(10), stream.next())
            .await
            .unwrap_or_else(|_| panic!("{what}: timed out at version {expected}"))
            .unwrap()
            .unwrap_or_else(|| panic!("{what}: the stream ended at version {expected}"));
        for tx in &batch.transactions {
            assert_eq!(
                tx.version, expected,
                "{what}: version {} arrived where {expected} was due — the shared \
                     reader lost or repeated a version",
                tx.version
            );
            seen.push(tx.version);
            expected += 1;
        }
    }
    seen
}

/// The headline: three projects, one connection.
#[tokio::test(flavor = "multi_thread")]
async fn one_stream_serves_every_project_on_the_network() {
    let chain = Chain::new(0);
    let tip = SharedTip::start(Arc::clone(&chain), Version::new(1), 4);
    let source = tip.source();

    let mut streams = Vec::new();
    for _ in 0..3 {
        streams.push(source.open(Version::new(1), None).await.unwrap());
    }
    chain.grow_to(40);

    for (n, stream) in streams.iter_mut().enumerate() {
        let seen = read_through(stream, 1, 40, &format!("project {n}")).await;
        assert_eq!(seen.len(), 40);
    }
    assert_eq!(
        chain.opens(),
        1,
        "three projects cost one stream — the whole point of ADR 0021"
    );
    assert_eq!(tip.subscribers(), 3);
}

/// A project starting behind the reader fills its own history on a slot, then joins.
/// The two have to meet exactly: one version delivered twice is as wrong as one lost.
#[tokio::test(flavor = "multi_thread")]
async fn a_project_that_starts_behind_catches_up_and_joins_without_a_seam() {
    let chain = Chain::new(100);
    let tip = SharedTip::start(Arc::clone(&chain), Version::new(101), 4);
    let source = tip.source();

    // Let the reader get established past the newcomer's start.
    chain.grow_to(120);
    wait_until(|| tip.position() >= Some(Version::new(120))).await;

    let mut behind = source.open(Version::new(1), None).await.unwrap();
    chain.grow_to(160);

    let seen = read_through(&mut behind, 1, 160, "the latecomer").await;
    assert_eq!(seen.len(), 160, "it saw all of history and then the tip");
    assert_eq!(
        chain.opens(),
        2,
        "the shared reader, plus one slot for this project's history"
    );
}

/// A project that stops reading fills its queue and is detached, and rejoins by the
/// same path a new project takes. What it must never do is come back with a hole.
#[tokio::test(flavor = "multi_thread")]
async fn a_project_that_falls_behind_is_detached_and_rejoins_without_a_gap() {
    let chain = Chain::new(0);
    let tip = SharedTip::start(Arc::clone(&chain), Version::new(1), 4);
    let source = tip.source();

    let mut slow = source.open(Version::new(1), None).await.unwrap();
    chain.grow_to(8);
    // Read a little, so it has a position to resume from.
    read_through(&mut slow, 1, 8, "the slow project").await;

    // Now go quiet while far more than one queue's worth of chain goes by.
    chain.grow_to(4_000);
    wait_until(|| tip.subscribers() == 0).await;
    assert_eq!(
        tip.subscribers(),
        0,
        "a project that stopped reading is dropped from the shared stream"
    );

    // Reading again repairs it, transparently: the caller sees version 9 next.
    chain.grow_to(4_100);
    let seen = read_through(&mut slow, 9, 4_100, "the slow project, rejoining").await;
    assert_eq!(seen.first().copied(), Some(9));
    assert_eq!(seen.last().copied(), Some(4_100));
}

/// The reason detaching is allowed at all: one project's fold must never become
/// everyone else's pace.
#[tokio::test(flavor = "multi_thread")]
async fn a_stalled_project_never_holds_up_the_others() {
    let chain = Chain::new(0);
    let tip = SharedTip::start(Arc::clone(&chain), Version::new(1), 4);
    let source = tip.source();

    let _stalled = source.open(Version::new(1), None).await.unwrap();
    let mut healthy = source.open(Version::new(1), None).await.unwrap();

    chain.grow_to(4_000);
    let seen = read_through(&mut healthy, 1, 4_000, "the healthy project").await;
    assert_eq!(
        seen.len(),
        4_000,
        "the healthy project read the whole chain while its neighbour never called next"
    );
}

/// Backfill ranges are history and can never come from a reader at the tip, so each
/// takes a slot — and the pool is what keeps the total under Geomi's cap.
#[tokio::test(flavor = "multi_thread")]
async fn a_bounded_range_takes_a_slot_of_its_own() {
    let chain = Chain::new(1_000);
    let tip = SharedTip::start(Arc::clone(&chain), Version::new(1_001), 2);
    let source = tip.source();

    let mut range = source
        .open(Version::new(1), Some(Version::new(20)))
        .await
        .unwrap();
    let seen = read_through(&mut range, 1, 20, "a backfill range").await;
    assert_eq!(seen.len(), 20);
    assert!(
        matches!(range.next().await, Ok(None)),
        "a bounded range ends at its last version rather than joining the tip"
    );
}

/// Poll a condition until it holds, so a test never depends on a sleep being long
/// enough on a loaded machine.
async fn wait_until(mut done: impl FnMut() -> bool) {
    let deadline = std::time::Instant::now() + Duration::from_secs(10);
    while !done() {
        assert!(
            std::time::Instant::now() < deadline,
            "timed out waiting for the shared reader"
        );
        tokio::time::sleep(Duration::from_millis(5)).await;
    }
}
