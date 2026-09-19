//! One Transaction Stream per network, read once and handed to every project on it
//! (ADR 0021).
//!
//! A stream per project puts a ceiling on hosted projects that no amount of revenue
//! moves: Geomi caps concurrent streams per organization, and the cap is **7 on
//! testnet** — which is where the free tier lives (`docs/research/spike-a-stream.md`).
//! Reading the chain once per network makes the steady-state count `1 + the backfill
//! pool`, a constant of the deployment rather than a function of how many customers
//! there are.
//!
//! The shared reader always runs at the tip and never rewinds. History is a separate
//! job: a project that starts behind the reader takes a slot from a small fixed pool,
//! streams its own range up to where the reader is, and then joins. Everything below
//! is in service of making that handover seamless and gapless.
//!
//! **The handover has no gap because the reader's position is always a batch
//! boundary.** A subscriber registers and reads that position under one lock, so the
//! queue it was handed begins exactly after the version it was told to catch up to.
//! Every queued batch then lies wholly after the catch-up range — never straddling it —
//! so nothing has to be trimmed and nothing can be delivered twice or skipped.
//!
//! **A slow project never slows anyone else.** Each subscriber has a bounded queue and
//! the reader offers batches without waiting. A project whose fold falls far enough
//! behind to fill its queue is detached, and its stream quietly takes a backfill slot,
//! catches up and rejoins — the same path a new project takes. Dropping a batch is
//! never an option: ordered, exactly-once delivery per project is what makes the fold
//! replayable (ADR 0005), and a gap would corrupt state with no error anywhere.

use std::sync::{Arc, Mutex, PoisonError};
use std::time::Duration;

use nineveh_core::Version;
use nineveh_ingest::{Batch, IngestError};
use tokio::sync::{OwnedSemaphorePermit, Semaphore, mpsc};
use tracing::{info, warn};

use crate::source::{BatchStream, Source};

/// Batches a subscriber may have waiting before the reader gives up on it.
///
/// Sized for a fold that stalls briefly — a slow commit, a lock wait — rather than one
/// that is genuinely behind. Being detached is not a failure: it costs a backfill slot
/// and a catch-up, which is the right price for a project that cannot keep up with the
/// chain, and the alternative is holding the whole network's reader at its pace.
const QUEUE_DEPTH: usize = 64;

/// How long the reader waits before reopening a stream that failed.
const REOPEN_DELAY: Duration = Duration::from_secs(2);

/// The last version a batch accounts for: its newest transaction, or how far the
/// server scanned when a filter left the batch empty.
fn covers(batch: &Batch) -> Option<Version> {
    batch
        .transactions
        .last()
        .map(|tx| Version::new(tx.version))
        .max(batch.processed_range.as_ref().map(|range| *range.end()))
}

/// The shared reader for one network.
pub struct SharedTip<S> {
    source: Arc<S>,
    /// Streams available for catching up. Sized with the shared reader's own stream so
    /// that `1 + slots` stays under the organization's cap with headroom.
    slots: Arc<Semaphore>,
    state: Mutex<State>,
}

struct State {
    subscribers: Vec<Subscriber>,
    /// The last version the reader has delivered to everyone. Always a batch
    /// boundary, which is what makes the handover exact.
    position: Option<Version>,
    /// Set when the reader task has stopped for good.
    stopped: bool,
    next_id: u64,
}

struct Subscriber {
    id: u64,
    queue: mpsc::Sender<Arc<Batch>>,
}

impl<S: Source + 'static> SharedTip<S> {
    /// Start reading `source` at `from` and keep reading forever.
    ///
    /// `slots` is the backfill pool, shared with every project on this network.
    #[must_use]
    pub fn start(source: Arc<S>, from: Version, slots: usize) -> Arc<Self> {
        let tip = Arc::new(Self {
            source,
            slots: Arc::new(Semaphore::new(slots.max(1))),
            state: Mutex::new(State {
                subscribers: Vec::new(),
                position: None,
                stopped: false,
                next_id: 0,
            }),
        });
        let reader = Arc::downgrade(&tip);
        tokio::spawn(async move {
            let mut next = from;
            loop {
                let Some(tip) = reader.upgrade() else { return };
                match tip.read_from(next).await {
                    Ok(reached) => next = reached,
                    Err(error) => {
                        warn!(%error, "the shared reader's stream failed; reopening");
                    }
                }
                // Resuming from the position already delivered leaves no gap: the
                // reader's cursor only ever moves on a whole batch.
                let resumed = {
                    let state = tip.state();
                    state.position.and_then(Version::next)
                };
                if let Some(resumed) = resumed {
                    next = resumed;
                }
                drop(tip);
                tokio::time::sleep(REOPEN_DELAY).await;
            }
        });
        tip
    }

    /// A source that hands this reader's batches to a pipeline.
    #[must_use]
    pub fn source(self: &Arc<Self>) -> SharedSource<S> {
        SharedSource {
            tip: Arc::clone(self),
        }
    }

    /// Read one stream until it ends or fails, fanning every batch out.
    async fn read_from(&self, from: Version) -> Result<Version, IngestError> {
        let mut stream = self.source.open(from, None).await?;
        let mut reached = from;
        while let Some(batch) = stream.next().await? {
            let covered = covers(&batch);
            self.fan_out(&batch, covered);
            if let Some(covered) = covered
                && let Some(next) = covered.next()
            {
                reached = next;
            }
        }
        Ok(reached)
    }

    /// Offer a batch to every subscriber and move the position, under one lock.
    ///
    /// The lock is what makes attaching exact: a subscriber that registers and reads
    /// the position in the same critical section cannot have a batch delivered in
    /// between, so the queue it holds begins precisely after the position it was told.
    ///
    /// Nothing here waits. A subscriber whose queue is full is dropped from the list,
    /// which its own stream notices and repairs by catching up on a slot.
    fn fan_out(&self, batch: &Arc<Batch>, covered: Option<Version>) {
        let mut state = self.state();
        let mut detached = Vec::new();
        state.subscribers.retain(|subscriber| {
            match subscriber.queue.try_send(Arc::clone(batch)) {
                Ok(()) => true,
                Err(mpsc::error::TrySendError::Full(_)) => {
                    detached.push(subscriber.id);
                    false
                }
                // Gone: the pipeline dropped its stream.
                Err(mpsc::error::TrySendError::Closed(_)) => false,
            }
        });
        if covered.is_some() {
            state.position = covered;
        }
        drop(state);
        for id in detached {
            info!(
                subscriber = id,
                "detached from the shared stream: too far behind to keep up, catching up on a slot"
            );
        }
    }

    /// Register for everything the reader delivers from now on.
    ///
    /// Returns the queue and the position it begins after, or `None` if the reader has
    /// stopped for good.
    fn attach(&self) -> Option<(mpsc::Receiver<Arc<Batch>>, Option<Version>)> {
        let mut state = self.state();
        if state.stopped {
            return None;
        }
        let (sender, receiver) = mpsc::channel(QUEUE_DEPTH);
        let id = state.next_id;
        state.next_id += 1;
        state.subscribers.push(Subscriber { id, queue: sender });
        Some((receiver, state.position))
    }
}

impl<S> SharedTip<S> {
    /// How far the reader has got, for reporting.
    #[must_use]
    pub fn position(&self) -> Option<Version> {
        self.state().position
    }

    /// How many projects are on the shared stream right now.
    #[must_use]
    pub fn subscribers(&self) -> usize {
        self.state().subscribers.len()
    }

    /// Catch-up streams still available. Zero means the next project to fall behind
    /// waits for one, which is the intended behaviour: a wait, never a stream the
    /// organization's cap would refuse.
    #[must_use]
    pub fn slots_free(&self) -> usize {
        self.slots.available_permits()
    }

    fn state(&self) -> std::sync::MutexGuard<'_, State> {
        // A panic under this lock would leave the list and the position consistent —
        // both are only ever replaced wholesale — so the contents stay usable.
        self.state.lock().unwrap_or_else(PoisonError::into_inner)
    }
}

/// A [`Source`] over a shared reader: the tail comes from the shared stream, and a
/// range of history takes a slot from the pool.
pub struct SharedSource<S> {
    tip: Arc<SharedTip<S>>,
}

impl<S> Clone for SharedSource<S> {
    fn clone(&self) -> Self {
        Self {
            tip: Arc::clone(&self.tip),
        }
    }
}

impl<S> std::fmt::Debug for SharedSource<S> {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("SharedSource").finish_non_exhaustive()
    }
}

impl<S> std::fmt::Debug for SharedTip<S> {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        let state = self.state();
        f.debug_struct("SharedTip")
            .field("position", &state.position)
            .field("subscribers", &state.subscribers.len())
            .field("slots_free", &self.slots.available_permits())
            .finish_non_exhaustive()
    }
}

impl<S: Source> std::fmt::Debug for SharedStream<S> {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        let state = match self.state {
            StreamState::Direct { .. } => "direct",
            StreamState::CatchingUp { .. } => "catching up",
            StreamState::Attached { .. } => "attached",
            StreamState::Detached => "detached",
        };
        f.debug_struct("SharedStream")
            .field("state", &state)
            .field("delivered", &self.delivered)
            .finish_non_exhaustive()
    }
}

impl<S: Source + 'static> Source for SharedSource<S> {
    type Stream = SharedStream<S>;

    async fn open(
        &self,
        from: Version,
        until: Option<Version>,
    ) -> Result<SharedStream<S>, IngestError> {
        // A bounded range is history: it can never be served by a reader that is at
        // the tip, so it takes a slot and reads for itself.
        if until.is_some() {
            let slot = self.slot().await;
            let stream = self.tip.source.open(from, until).await?;
            return Ok(SharedStream {
                tip: Arc::clone(&self.tip),
                delivered: None,
                state: StreamState::Direct {
                    stream,
                    _slot: slot,
                },
            });
        }
        let mut stream = SharedStream {
            tip: Arc::clone(&self.tip),
            delivered: from.get().checked_sub(1).map(Version::new),
            state: StreamState::Detached,
        };
        stream.rejoin(from).await?;
        Ok(stream)
    }
}

impl<S> SharedSource<S> {
    async fn slot(&self) -> OwnedSemaphorePermit {
        // The pool never closes, so acquiring can only wait, never fail.
        Arc::clone(&self.tip.slots)
            .acquire_owned()
            .await
            .unwrap_or_else(|_| unreachable!("the backfill pool is never closed"))
    }
}

/// One project's view of the stream: its own for as long as it is behind, shared once
/// it reaches the tip, and its own again if it ever falls off.
pub struct SharedStream<S: Source> {
    tip: Arc<SharedTip<S>>,
    /// The last version handed to the pipeline, so a rejoin knows where to resume.
    delivered: Option<Version>,
    state: StreamState<S>,
}

enum StreamState<S: Source> {
    /// A range of history, on a slot of its own.
    Direct {
        stream: S::Stream,
        _slot: OwnedSemaphorePermit,
    },
    /// Catching up to where the shared reader is, with the queue already collecting
    /// everything after it.
    CatchingUp {
        stream: S::Stream,
        _slot: OwnedSemaphorePermit,
        queue: mpsc::Receiver<Arc<Batch>>,
    },
    /// At the tip, on the shared reader.
    Attached { queue: mpsc::Receiver<Arc<Batch>> },
    /// Between the two, only while rejoining.
    Detached,
}

impl<S: Source + 'static> SharedStream<S> {
    /// Attach to the reader and arrange to cover everything from `from` up to where it
    /// already is.
    ///
    /// Registering *before* reading the position is the whole trick: the queue starts
    /// collecting at the boundary the catch-up will finish on, so the two meet exactly.
    async fn rejoin(&mut self, from: Version) -> Result<(), IngestError> {
        let Some((queue, position)) = self.tip.attach() else {
            return Err(IngestError::ReaderStopped);
        };
        let behind = position.is_some_and(|position| from <= position);
        if !behind {
            // Already at or ahead of the reader: nothing to catch up on.
            self.state = StreamState::Attached { queue };
            return Ok(());
        }
        let source = SharedSource {
            tip: Arc::clone(&self.tip),
        };
        let slot = source.slot().await;
        let stream = self.tip.source.open(from, position).await?;
        self.state = StreamState::CatchingUp {
            stream,
            _slot: slot,
            queue,
        };
        Ok(())
    }
}

impl<S: Source + 'static> BatchStream for SharedStream<S> {
    async fn next(&mut self) -> Result<Option<Arc<Batch>>, IngestError> {
        loop {
            match &mut self.state {
                StreamState::Direct { stream, .. } => {
                    let batch = stream.next().await?;
                    if let Some(batch) = &batch {
                        self.delivered = covers(batch).or(self.delivered);
                    }
                    return Ok(batch);
                }
                StreamState::CatchingUp { stream, queue, .. } => {
                    if let Some(batch) = stream.next().await? {
                        self.delivered = covers(&batch).or(self.delivered);
                        return Ok(Some(batch));
                    }
                    // Caught up. Everything after the catch-up's last version is
                    // already waiting in the queue.
                    let queue = std::mem::replace(queue, mpsc::channel(1).1);
                    self.state = StreamState::Attached { queue };
                }
                StreamState::Attached { queue } => {
                    if let Some(batch) = queue.recv().await {
                        self.delivered = covers(&batch).or(self.delivered);
                        return Ok(Some(batch));
                    }
                    // Detached, because this project couldn't keep up or because the
                    // reader restarted. Take a slot, catch up, and rejoin.
                    let from = self
                        .delivered
                        .and_then(Version::next)
                        .unwrap_or(Version::GENESIS);
                    self.state = StreamState::Detached;
                    self.rejoin(from).await?;
                }
                StreamState::Detached => {
                    let from = self
                        .delivered
                        .and_then(Version::next)
                        .unwrap_or(Version::GENESIS);
                    self.rejoin(from).await?;
                }
            }
        }
    }
}
