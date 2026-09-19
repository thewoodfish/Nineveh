# 0022. Keep each project's decoded records; let the log lead the fold

- Status: Proposed
- Date: 2026-09-19

## Context

A state table is derived: the rules are a function from a project's record history to
its rows. Change the function and the rows have to be computed again from the same
history, because the old rows don't contain it — a `when` clause that now excludes
records already counted cannot be undone from a total.

So Nineveh rebuilds, and ADR 0016 makes a rebuild "an ordinary build with its own row,
cursor and outbox": it starts at `start_version` and re-reads the chain. The trigger is
the build fingerprint (`store.rs:606`), a hash over the layout and semantics versions,
the lock, and `Project::canonical()` — which covers the network, the start version,
every source and every state table with all its columns and rules. One edited `when`
clause changes the hash, so **every table in the project is rebuilt from the contract's
publish version**.

Measured, that is expensive in the one resource that is actually scarce
(`docs/research/spike-a-stream.md`): at 4,372 versions/second, a 90-day-old mainnet
contract takes 2.8 days to re-read, and 11.3 days at a year — holding a concurrent
stream slot throughout, while ADR 0016 keeps the old build served but frozen. Editing a
rule is the ordinary action Studio exists for, and it is charged at the price of the
most expensive operation the system performs.

There is no cheaper source to rebuild from. The hosted Indexer GraphQL exposes 36 base
tables and every one is domain-specific — no generic events table, no writeset table —
and the REST API retains about 12.8 days of mainnet history. Past a fortnight the
Transaction Stream is the only way to obtain a contract's history, at any price.

The records themselves, though, are small. A project's records are what its own
contract did, not what the chain did: the busiest contract in a sampled mainnet window
produced about 0.3% of the chain's bytes, and a quiet project produced 1,601 records in
its lifetime.

## Decision

**Each project keeps its decoded records, in version order, in `nineveh.records`.**
One row per record, keyed by `(schema_name, version, ord)`, where `ord` is the record's
position in its transaction in the order the decoder emitted it — events before
write-set changes, as the fold must see them. Each row carries the transaction's
timestamp, because rules may read `tx.timestamp`.

**The log holds what the decoder emitted, not what matched a declared source.** A table
source only works because parent resource writes reveal its handles (ADR 0012), and
those parents are pinned for that purpose rather than declared as sources. Storing only
declared-source matches would lose handle attribution on replay and break table sources
silently. The log's contents are therefore defined as the decoder's output for that
project's selection, which is already what the pipeline produces.

**The log leads the fold.** Two cursors, not one:

- the **ingest cursor** — how far the record log has been written;
- the **fold cursor** — how far state has been committed.

The log is written first, in its own transaction; the fold consumes from it and commits
state, the outbox and its own cursor atomically as ADR 0005 requires. This is what makes
folding skippable for an idle project later, and it improves the failure story now: a
deterministic fold error halts at a version whose records are already durable, so the
rule can be fixed and replayed instead of re-streamed to reproduce the fault.

**A rebuild replays the log.** Where the log covers the range, a rebuild reads records
and feeds them to the engine directly — the same `StateView × Records → ChangeSet`
boundary the pipeline uses, with decode skipped because the records are already decoded.
Rebuilds stop touching the network.

**Two things still force a backfill**, because the log cannot contain what was never
followed or was decoded under a layout since corrected:

- **adding a source** — there is no history for something the selection never matched;
- **re-pinning the lock** — records already stored were decoded under the old layouts.

The rule contributors follow: **changing rules replays locally; adding a source or
re-pinning backfills.**

**Records are stored in a lossless internal encoding, not the API's.** `Value`
serializes today in the API's shape (ADR 0008: wide integers as strings, `Option`
unwrapped, `Object<T>` as its address), which is for reading and cannot be read back —
it doesn't say whether a string was a `u64` or a `u128`. The log needs a round-trip, so
it gets its own encoding with the discriminators the API shape drops, and `Value` gains
a matching `Deserialize`. The two shapes are separate on purpose: one is a contract with
users, the other is a contract with ourselves.

**Postgres, in `nineveh.records`.** It is the same transactional machinery the outbox
already uses, crash-safe by the same argument, and reversible: at present volumes the
choice costs nothing, and moving to append-only segments later is a change of storage
without a change of meaning.

## Alternatives considered

**Keep re-streaming.** What happens today. Rejected: it spends days of a scarce stream
slot on the cheapest action in the product, and past a fortnight there is no other way
to obtain the same bytes, so the cost is unavoidable rather than merely high.

**Store the raw matched slices of the stream and re-decode on replay.** Survives a
re-pinned lock, and replay would be byte-identical to the original decode. Rejected for
now: decode is transaction-scoped and two-pass (ADR 0012 learns handles before routing),
so per-record slices lose the context the decoder needs. Storing the decoder's output
keeps replay at the engine's own input boundary, which is both smaller and the interface
ADR 0005 already defines.

**Write records inside the state commit.** Atomic with the rows they produced, and one
cursor instead of two. Rejected: it makes the log exactly as far along as the fold, so
folding can never be skipped, and a record that fails the fold is never stored — which
is the one record you want to keep.

**Reuse a project's `log:` state tables.** Where a source happens to have one they hold
similar data. Rejected as a mechanism: they are user-declared, shaped by config, and
therefore part of what a rebuild invalidates. The record log has to be independent of
the config it will be replayed under.

## Consequences

Editing a rule stops costing a re-stream. A rebuild becomes a local pass over the
project's own records: minutes rather than days, no stream slot, and the served build
is frozen for correspondingly less time.

There are two cursors to keep straight. The ingest cursor may run ahead of the fold
cursor, and everything that reports progress — Studio's health, the lag meter, the
`Health` type — has to say which one it means.

Retention now covers two things, and they are one policy: how much of the record log is
kept is how far back a project can rebuild without the network, and how much of the
outbox is kept is how far behind a webhook endpoint may fall. Both are "how much
history does this tier keep", and deciding them separately would mean two migrations.

Storage becomes the meter that matters. A project's record log grows with its own
contract's activity, which makes it both the dominant cost of an idle project and a
fair thing to charge for.

The engine gains a second entry point that is not the pipeline. `Records` already exists
as the boundary; replay feeds it from Postgres instead of from decode. Nothing in
`nineveh-engine` changes.

## Open questions

- Whether `nineveh.records` should be partitioned by schema, or by version range, before
  the first project large enough to care.
- Whether the ingest cursor should be allowed to run arbitrarily far ahead of the fold,
  or bounded so an unfolded backlog can't grow without limit.
