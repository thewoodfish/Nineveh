# 0023. Fold on demand; idle is invisible, stopped is deliberate

- Status: Proposed
- Date: 2026-09-19

## Context

A project's pipeline folds continuously: every version that arrives is folded into
state and committed, whether or not anything reads the result. Most hosted projects
will be read rarely and many will be abandoned within a week, so most of that work is
computed for nobody. On a free tier that is the dominant cost of a project nobody
wants any more.

ADR 0022 made the alternative possible. Records are kept per project and the log leads
the fold, so the inputs stay current whether or not the outputs are computed. Catching
up reads the log rather than the chain.

That changes what "behind" costs, and by how much. Measured on the vault workload in a
release build, a rebuild from the log runs at **7,963 records a second**, at 503 bytes
a record (`crates/nineveh-pipeline/tests/pipeline.rs`, `bench_rebuild_from_the_log`).
Catch-up is therefore proportional to a project's own records, not to elapsed time and
not to chain versions — a quiet contract idle for six months catches up faster than a
busy one idle for an hour. Any policy phrased in days is measuring the wrong thing.

There is already a state for "not running": `control_projects.running`, which the user
sets from Studio and which stops the pipeline while the API keeps serving what was
committed. The temptation is to reuse it. It is the wrong shape for this, for a reason
worth writing down: it requires a human to press Start.

## Decision

**Three states, and only one of them is the user's business.**

| | Records | Fold | Entered | Left |
| --- | --- | --- | --- | --- |
| **Running** | yes | yes | — | — |
| **Idle** | yes | no | automatically | automatically |
| **Stopped** | no | no | deliberately | deliberately |

**Idle is invisible and automatic in both directions.** A project stops folding when
nothing is waiting on its output, and folds again the moment something is — a query
catches up from the log and answers it. Nobody presses a button to make their data
correct. A manual resume here would mean an API consumer silently receiving stale
answers with no way to know why, which is a bug wearing the costume of a feature.

**Three things mean something is waiting, and the first two are contracts rather than
guesses:**

- **A webhook endpoint.** It is a standing instruction to deliver, and deliveries come
  from the outbox, which only exists if the fold runs. A project with an endpoint is
  never idle, however long since anyone looked at it.
- **A live change-feed subscriber.** Present or not, not a timestamp.
- **A read within the idle window.** Needs a per-project last-read time, which does not
  exist today: `api_keys.last_used_at` is per key and hosted-only, and local mode
  records nothing. It follows that column's pattern of only writing when the stored
  value is a minute stale, so it doesn't add a write per request.

**The backlog bound, not the clock, is what keeps resuming fast.** A project folds
anyway, idle or not, once its unfolded records exceed a bound chosen from the worst
first-query latency we will accept:

```
bound = fold rate × acceptable latency
      = 7,963 records/second × 2 seconds
      ≈ 16,000 records
```

That is a derivation, and it moves when the measurement moves. The idle window itself
(24 hours) is deliberately not load-bearing — it decides when to *stop*, while the
bound decides how far behind a project may get, which is the number anyone feels.

**The fold cursor may never fall below the retention horizon.** When records are pruned
at some horizon, a fold left behind it could never be brought current: the inputs it
needs are gone, and the only way back is the chain. So retention forces a fold
regardless of idleness, and idleness is bounded by retention rather than only by the
backlog. This is a correctness rule, not a tuning knob, and it means the two policies
cannot be set independently.

**Stopped stays what it is, and becomes where tier enforcement goes.** It is explicit,
it needs a deliberate Start, and it halts the record log as well as the fold — which is
what makes it consequential. A project left stopped past the retention horizon loses
records it can only get back from the chain, so resuming it costs a stream slot and
hours. Studio must say so before it happens, in those terms, rather than offering a
Stop button that looks like Idle with a nicer name.

## Alternatives considered

**Reuse `running` for idleness.** One state, no new concepts. Rejected: it requires a
human to resume, and the whole point is that nobody should have to know. It would also
stop the record log, turning a scheduling decision into a decision about whether the
project's history survives.

**Define idle by time alone — "no reads for N days".** Simple and legible. Rejected
because it measures the wrong thing: resume cost is records, and two projects equally
idle by the clock can be a hundred thousand records apart. It is right as the trigger
to *stop*, which is how it is used here, and wrong as the bound on how far behind a
project may get.

**Stop appending records too, once idle.** Cheaper still, and tempting when the shared
reader lands and appending is the only per-project ingest cost left. Rejected: it makes
waking up a chain read, which is the expensive thing this whole line of work exists to
avoid, and it is indistinguishable from Stopped — which we already have.

**Fold lazily on read, with no backlog bound.** Purest form: compute exactly when
asked. Rejected because the first query after a long sleep would be unbounded, and a
busy contract could make it minutes. The bound is what keeps "invisible" true.

## Consequences

An abandoned project costs its record bytes and nothing else. No fold, no state
commits, no outbox rows — which is what makes a free tier affordable, and it is a
better answer than an idle timer that stops ingest, because the project stays instantly
resumable.

The fold cursor and the record cursor can differ by design, and everything that reports
progress has to say which it means. Studio's lag meter reports the chain's distance from
the *record* cursor, because that is what "are we keeping up" means; how far the fold
is behind is a different number and belongs beside it, not instead of it.

A read path can now block on folding. The API has to be able to ask for a catch-up and
wait for it, bounded, which it has never had to do — and it needs an answer for the case
where the catch-up fails, where today a halted project simply stops advancing.

Idleness is a per-project scheduling decision that the control plane makes, so it
belongs beside the supervision in `plane.rs` rather than inside a pipeline, which only
knows about itself.

Retention gains a second consumer with a harder constraint. The webhook cursor floor
(ADR 0020) says how far the outbox may be pruned; the fold cursor now says how far the
record log may be pruned. Both have to be honoured by the same pruner.

## Open questions

- Whether a project with a halted fold should keep appending records. Appending is
  cheap and keeps the fix to "correct the rule and replay", which argues yes; it also
  accumulates storage for a project that may never be fixed.
- Whether the idle window should differ by tier, or whether the backlog bound alone is
  enough to make it not matter.
