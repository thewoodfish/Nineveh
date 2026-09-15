# 0006. Change feeds come from a transactional outbox

- Status: Accepted; feed granularity and `seq` numbering superseded by [0014](0014-state-schema-layout.md)
- Date: 2026-09-14

## Context

Subscriptions and webhooks must fire exactly when state changes, never for a change that
was rolled back, and never miss one. The brief proposed Postgres logical replication or
LISTEN/NOTIFY. Both have problems:

- **`NOTIFY` alone.** Payloads are limited to 8000 bytes. Notifications are lost if no
  listener is connected at commit time, so a restarted realtime service misses events.
- **Logical replication.** It needs `wal_level=logical` and replication slots, which
  many managed Postgres plans restrict. A slot whose consumer dies retains WAL without
  bound, so an outage in realtime could fill the database's disk.

## Decision

- The reducer commit (ADR 0005) inserts rows into an internal `changes` table **in the
  same transaction** as the state and the cursor. Each row holds the project, table,
  key, operation, new row and version, plus a per-commit sequence number.
- After commit, the pipeline sends `NOTIFY` as a **wake-up only**. It carries no
  payload the consumer depends on.
- `nineveh-realtime` tails `changes` by `(version, seq)`. Subscribers can resume from a
  version. Webhooks are delivered at least once, carrying an idempotency key
  `(project, version, seq)` and an HMAC signature.
- Changes are coalesced per key per batch. Subscribers see each committed value, not
  every intermediate value within one batch.
- Retention is time- and position-bounded, and consumers track their own position.

## Consequences

- The single-writer rule covers notifications too: a change is recorded at the one place
  state changes.
- It works on any Postgres that supports transactions. There's no superuser setup and no
  slots to leak.
- Outbox rows cost write volume on every commit, and the table needs pruning. Both are
  bounded and measurable.
