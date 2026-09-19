//! Whether a project's fold is worth running right now (ADR 0023).
//!
//! State tables exist to be read, subscribed to, or delivered from. A project none of
//! those are happening to is computing rows for nobody, and folding is the part worth
//! stopping — the record log keeps filling either way, so the inputs stay current and
//! catching up later is a local pass rather than a chain read.
//!
//! Two of the three signals are declarations rather than guesses. A webhook endpoint is
//! a standing instruction to deliver, and deliveries come from the outbox, which only
//! exists if the fold runs; a live subscriber is present or it isn't. Only the third,
//! "has anyone queried it lately", has to be observed, which is why the timestamp in
//! `nineveh.project_reads` exists at all.
//!
//! What keeps waking up fast is not the clock but the backlog. Catch-up reads the log,
//! so its cost is records: a quiet contract idle for six months catches up faster than
//! a busy one idle for an hour, and any rule phrased in days measures the wrong thing.

use std::time::Duration;

/// How long without a read before a project may stop folding.
///
/// Deliberately not load-bearing. It decides when to *stop*; [`Demand::backlog_bound`]
/// decides how far behind a project may get, which is the number anyone feels.
pub const IDLE_AFTER: Duration = Duration::from_secs(24 * 60 * 60);

/// Records a rebuild folds in a second, measured on the vault workload in a release
/// build (`bench_rebuild_from_the_log`). The bound below is derived from it, so when
/// the measurement moves, the bound moves with it.
pub const FOLD_RECORDS_PER_SECOND: i64 = 7_963;

/// The worst first-query latency an idle project may cost someone.
pub const WAKE_BUDGET: Duration = Duration::from_secs(2);

/// What is waiting on a project's folded state.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct Demand {
    /// The config declares at least one webhook endpoint.
    pub webhooks: bool,
    /// Live change-feed listeners.
    pub listeners: usize,
    /// Seconds since the API was last read, or `None` if it never has been.
    pub read_ago: Option<i64>,
    /// Records logged but not folded.
    pub backlog: i64,
}

/// Why a project is folding, or isn't.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Verdict {
    /// Something is waiting on the rows.
    Wanted(Reason),
    /// Nothing is waiting, and the backlog is small enough that waking up stays quick.
    Idle,
}

/// What kept a project folding.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Reason {
    /// Deliveries come from the outbox, which only exists if the fold runs.
    Webhooks,
    /// Somebody is watching the change feed.
    Listeners,
    /// The API was read recently.
    Read,
    /// Nobody is waiting, but waking up would take too long if it fell further behind.
    Backlog,
}

impl Demand {
    /// The most records a project may leave unfolded before waking up costs more than
    /// [`WAKE_BUDGET`].
    #[must_use]
    pub const fn backlog_bound() -> i64 {
        FOLD_RECORDS_PER_SECOND * WAKE_BUDGET.as_secs().cast_signed()
    }

    /// Whether the fold should run.
    ///
    /// The order is deliberate: the two declarations are checked before the two
    /// measurements, so a project with a webhook endpoint never depends on a timestamp
    /// or a count being right.
    #[must_use]
    pub fn verdict(&self, idle_after: Duration) -> Verdict {
        if self.webhooks {
            return Verdict::Wanted(Reason::Webhooks);
        }
        if self.listeners > 0 {
            return Verdict::Wanted(Reason::Listeners);
        }
        let idle_secs = i64::try_from(idle_after.as_secs()).unwrap_or(i64::MAX);
        match self.read_ago {
            Some(ago) if ago < idle_secs => return Verdict::Wanted(Reason::Read),
            // A project nobody has *ever* read is a project nobody is waiting on. It
            // still folds while its backlog is small, so a first visit is quick.
            _ => {}
        }
        if self.backlog >= Self::backlog_bound() {
            return Verdict::Wanted(Reason::Backlog);
        }
        Verdict::Idle
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn nobody() -> Demand {
        Demand {
            webhooks: false,
            listeners: 0,
            read_ago: None,
            backlog: 0,
        }
    }

    #[test]
    fn nothing_waiting_and_nothing_pending_is_idle() {
        assert_eq!(nobody().verdict(IDLE_AFTER), Verdict::Idle);
    }

    #[test]
    fn a_declared_endpoint_outranks_every_measurement() {
        let wanted = Demand {
            webhooks: true,
            read_ago: Some(i64::MAX),
            ..nobody()
        };
        assert_eq!(
            wanted.verdict(IDLE_AFTER),
            Verdict::Wanted(Reason::Webhooks),
            "an endpoint is a standing instruction, however long since anyone looked"
        );
    }

    #[test]
    fn a_watcher_keeps_it_awake() {
        let watched = Demand {
            listeners: 1,
            ..nobody()
        };
        assert_eq!(
            watched.verdict(IDLE_AFTER),
            Verdict::Wanted(Reason::Listeners)
        );
    }

    #[test]
    fn a_recent_read_keeps_it_awake_and_an_old_one_does_not() {
        let recent = Demand {
            read_ago: Some(60),
            ..nobody()
        };
        assert_eq!(recent.verdict(IDLE_AFTER), Verdict::Wanted(Reason::Read));

        let stale = Demand {
            read_ago: Some(30 * 24 * 60 * 60),
            ..nobody()
        };
        assert_eq!(stale.verdict(IDLE_AFTER), Verdict::Idle);
    }

    /// The bound, not the clock, is what keeps waking up quick.
    #[test]
    fn a_backlog_folds_however_long_nobody_has_looked() {
        let behind = Demand {
            read_ago: Some(365 * 24 * 60 * 60),
            backlog: Demand::backlog_bound(),
            ..nobody()
        };
        assert_eq!(behind.verdict(IDLE_AFTER), Verdict::Wanted(Reason::Backlog));

        let nearly = Demand {
            backlog: Demand::backlog_bound() - 1,
            ..behind
        };
        assert_eq!(nearly.verdict(IDLE_AFTER), Verdict::Idle);
    }

    /// The point of measuring the fold rate: the bound is derived from it, so a
    /// project waking from idle never costs more than the budget.
    #[test]
    fn the_bound_is_the_wake_budget_at_the_measured_rate() {
        let seconds = Demand::backlog_bound() / FOLD_RECORDS_PER_SECOND;
        assert_eq!(seconds, i64::try_from(WAKE_BUDGET.as_secs()).unwrap());
    }
}
