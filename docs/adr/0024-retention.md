# 0024. Retain the outbox by time and the record log by size

- Status: Proposed
- Date: 2026-09-19

## Context

Two tables grow without limit and neither is pruned.

`nineveh.changes` is the transactional outbox (ADR 0006): a row per state-row change,
carrying the whole row as JSON, written in the commit that made it. It has two readers.
The change feed serves browsers, which resume by `Last-Event-ID` and hold no cursor
server-side. Webhook endpoints do hold one — `nineveh.webhooks.version, seq` — and an
endpoint that is down keeps its place so it can catch up (ADR 0020).

`nineveh.records` is the record log (ADR 0022): every record the decoder emitted for a
project, kept so a rebuild replays locally instead of re-reading the chain. Its reader
is the fold, which consumes it in version order and keeps its own cursor.

They look like one problem and are two, which is where an earlier assumption — that
retention is a single policy over both — turns out to be wrong.

**The outbox is a buffer.** Its contents are only interesting until every reader has
seen them. What it needs is a window long enough that a receiver which was down over a
weekend can still catch up, and nothing beyond that.

**The record log is history.** Its whole value is that a project's past does not have to
be bought from the chain twice, and past a fortnight the chain is the only other source
(`docs/research/spike-a-stream.md`). Pruning it by time would cap the feature it exists
to provide: a seven-day window on a year-old contract means editing one rule still
costs a full re-stream, which is exactly the cost ADR 0022 was written to remove.

Its size is the customer's own activity, not the chain's, and measured at 503 bytes a
record:

| Project | Log growth | A 1 GB allowance holds |
| --- | --- | --- |
| quiet (10 records/day) | 5 KB/day | centuries |
| modest (1k/day) | 0.5 MB/day | about six years |
| busy (100k/day) | 50 MB/day | three weeks |

So a generous record log is nearly free for the projects a free tier is for, and
expensive only for projects large enough to be paying.

## Decision

**Two policies, because they answer different questions.**

**The outbox is retained by time.** Rows older than the project's window are deleted,
where the window defaults to seven days. A browser that has been away longer resyncs,
which it must already do after a rebuild (ADR 0016), so nothing new is asked of it.

**The outbox prune is floored by the slowest webhook endpoint.** Deleting past an
endpoint's cursor would silently drop deliveries it was owed, turning at-least-once
into at-most-once without an error anywhere:

```
prune changes below  min( now − window,  min(version, seq) over this schema's endpoints )
```

An endpoint stuck far behind therefore holds the outbox open, which is correct — its
deliveries are still owed — and visible, because its failure count and last error are
already reported.

**The record log is retained by size, oldest first.** A project over its allowance has
its oldest records deleted until it is under. Time does not enter into it: what the log
is for is depth of history, and the tier's storage is the honest limit on that.

**The record log prune only takes what the fold has consumed.** Records at or below the
fold cursor have already become state; records above it have not, and deleting those
would leave rows that can never be computed, from inputs that only the chain still has.
This is the floor ADR 0023 named, stated as a rule the pruner follows:

```
prune records only where  version ≤ the project's fold cursor
```

A project that is idle, halted, or far behind therefore keeps its records however large
they grow — it is over quota and the answer is to say so, not to destroy the inputs it
needs.

**Pruning the log is not an error.** A rebuild that finds the log no longer reaches back
to where it must start already refuses and falls back to the stream
(`replay::Unavailable::StartsLate`). Pruning simply makes that case more common on
projects that keep less history, which is what a smaller allowance means.

**Both run on the same sweep, per project, and both are bounded.** A prune deletes at
most a fixed number of rows per pass so a large backlog is worked off over several
rather than holding a long transaction.

## Alternatives considered

**One window over both tables.** Simpler to explain, and what an earlier sketch of this
assumed. Rejected once the two purposes were separated: a time window on the record log
caps rebuild depth, so a modest project on a year-old contract would lose the ability to
edit a rule cheaply for no saving worth having — its whole log is a fifth of a gigabyte.

**Retain the record log by time, generously — a year, say.** Closer, but it still
answers the wrong question. Two projects with the same age of log can differ by three
orders of magnitude in size, and the cost being managed is bytes.

**Prune the outbox by size too.** Tempting for symmetry. Rejected: the outbox's job is
to be caught up with, and the number that matters is how long a receiver may be down,
not how much it accumulated meanwhile.

**Let the record log prune ahead of the fold when a project is far behind.** It would
bound storage for a project that has stopped folding. Rejected: it destroys inputs that
cannot be recovered except from the chain, to save space on a project that is already
over quota and can be told so.

## Consequences

Storage becomes a number a tier can be sold on, and the one that matters is the record
log, because the outbox is bounded by a week whatever happens.

A webhook endpoint that stays broken now has a visible cost: it holds its project's
outbox open. That wants surfacing in Studio beside the endpoint's failure count, rather
than discovering it as unexplained growth.

Rebuild depth becomes a tier feature rather than a constant. "How far back can I change
a rule without re-reading the chain" is answered by how much of the log a plan keeps,
which is a clearer thing to sell than a time window.

A project that stops folding stops pruning its records, so its log grows until something
folds it or the project is stopped. That is the right trade — the alternative destroys
history — but it means quota enforcement has to act on projects, not just on tables.

## Open questions

- Whether a project should be able to keep more history than its plan's storage by
  paying for the storage alone, which is the honest shape of "I want a deep rebuild
  next month".
- Whether the outbox window should differ per endpoint, for a receiver known to be
  offline for longer than a week.
