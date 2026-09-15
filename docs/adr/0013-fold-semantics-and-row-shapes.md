# 0013. Fold semantics, row shapes, and the missing-key retry

- Status: Accepted
- Date: 2026-09-15

## Context

ADR 0005 fixed the fold's signature, `fold(&StateView, &[Record]) -> ChangeSet`, and
the atomic commit around it. Building `nineveh-engine` settled what the fold does,
record by record. `nineveh-store`, the API and replay all depend on those details, so
they're recorded here.

## Decision

**Order.**
- Transactions fold in version order.
- Each transaction folds in two passes. The first learns table handles from every
  parent the transaction reveals (ADR 0012). The second applies records in stream
  order: events, then write-set changes.
- A record feeds its source's state tables in config order, and within a `reduce`
  table its rules in list order. Each rule sees the effect of the one before.
- `SmartTable` buckets rewritten in a transaction are netted at the end of that
  transaction. All the rewritten buckets are compared with their previous contents
  as one map, then deletes apply before writes. An entry that moved between buckets
  in a split is neither deleted nor rewritten, so write rules don't fire for moves.

**Rules.**
- A rule's key expressions read only the record. The key picks the row, so the row
  can't be an input to it.
- A missing row starts with its key columns set from the key, other columns set to
  their `default` or null, and columns the rule must set left empty. The config's
  completeness check (ADR 0011) guarantees the rule sets them.
- `when` reads the row as it stands.
- All `set` expressions see the row as it was before the rule, like a simultaneous
  assignment: `set: { a: "b", b: "a" }` swaps.
- A rule that leaves a row unchanged writes nothing and emits no change.

**Row shapes.** The store maps these to columns.

| Table | Key | Row |
| --- | --- | --- |
| `reduce` | the key columns | the configured columns, in order |
| `mirror` of a resource | `[address]`, plus `[type]` for a generic source that matches every instantiation | the key, then the value |
| `mirror` of a table | `[handle, key]` | `[handle, key, value]` |
| `log` | `[version, event index]` | the key, then the event value |
| internal `Handles` | `[handle, source]` | empty |
| internal `Buckets` | `[source, handle, bucket]` | `[entries]` |

Internal tables are committed with the state they gate, so replay reproduces them.

**Change feed.** Every change to a state-table row (insert, update or delete) is
recorded in order with its version, and the change set carries these for the outbox
(ADR 0006). Because the fold is deterministic, the committed feed is the same
however the stream is batched.

**Missing keys.** A view answers each lookup with *absent*, *present*, or
*not loaded*. When the fold meets a key that isn't loaded, it continues, treats the
row as absent, and then returns `NotLoaded` with every key it missed, even if a halt
happened after the first miss. A halt reached that way may be caused by the missing
data rather than being real. The caller loads the missing keys and folds again.
That's safe because the fold is pure, and it converges because each retry loads
more. So the store's preload is an optimization, not something correctness depends
on.

**Halts.** A failing expression halts the batch with a located `Halt`: the version,
the record, the table and rule, and the failing subexpression's span in
`nineveh.yaml`. Halts are never retryable.

## Consequences

- The M1 replay property holds and is tested (`nineveh-engine/tests/replay.rs`). For
  random workloads, random batch boundaries and random crashes, the state and the
  change feed equal a single pass and a model of the contract. The same holds when
  keys load lazily through retries.
- Two limits remain, and config resolution rejects both rather than letting them
  under-cover:
  - `BigOrderedMap` sources, whose small maps live inside the parent.
  - Generic resource-group members that match every instantiation.
- A parent must appear as a resource or as a table value to be watched. A parent
  nested deeper inside another value isn't watched yet.
