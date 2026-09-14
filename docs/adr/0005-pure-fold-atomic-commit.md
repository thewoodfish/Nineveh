# 0005. Pure fold, atomic commit, one-way dependencies

- Status: Accepted
- Date: 2026-09-14

## Context

Nineveh's promise is that state tables are an exact, replayable projection of the
chain. The invariants in `CLAUDE.md` are these: single writer, deterministic reducers,
per-key version order, exactly-once effect, and resume from the version cursor. They
have to hold through crashes, restarts and backfills, and they have to be *provable* by
tests, not just intended.

## Decision

**Pipeline.** Each project runs one pipeline. Its stages are joined by bounded channels,
so a slow database slows the gRPC read and memory stays flat.

```
ingest ─▶ decode (parallel, order-preserving) ─▶ fold (one task) ─▶ commit (one PG txn)
```

- **Fold is a pure function:** `fold(&StateView, &[Record]) -> Result<ChangeSet>`. It
  does no I/O, reads no clock and uses no randomness. It applies records in version
  order. That makes per-key ordering true by construction, and cross-table reads see a
  consistent as-of-version view.
- **Commit is one Postgres transaction** holding the state upserts and deletes, the
  outbox rows (ADR 0006) and the cursor. After a crash, the pipeline resumes from
  `cursor + 1` and re-folds the same inputs into the same result. That gives an
  exactly-once effect without dedup tables.
- **Throughput** comes from parallel decode and from coalescing. A batch loads the
  touched keys once per table (`WHERE key = ANY($1)`), folds in memory, and writes each
  key once (an `UNNEST` upsert).
- **Key-sharded folding** is the scaling path, but it's allowed only with profiling
  evidence that the single fold task is the bottleneck.
- **Deterministic errors halt the project** at that version with a located error.
  Transient errors retry with jittered backoff. `IngestError::is_retryable` and its
  counterparts in other crates make that distinction explicit.

**Single writer, enforced by Postgres.** The pipeline connects as a writer role. The API
and realtime services connect as a role with only `SELECT` on state schemas. A bug
elsewhere can't write state because the database refuses it.

**Upgrades and replay.** Config changes that alter derived data (sources, reducers,
columns) rebuild into a shadow schema from `start_version`, then swap atomically. Other
changes (indexes, API exposure, webhooks) apply in place. `nineveh replay` is a rebuild
with the same config. Each state schema records the config hash and the
expression-semantics version it was built with.

**Dependency direction.** Crates depend one way only, and CI enforces it with
`scripts/check-deps.sh`:

| Crate | Must never depend on |
| --- | --- |
| `nineveh-core`, `nineveh-expr`, `nineveh-engine` | tokio, sqlx, tonic |
| `nineveh-config` | tokio, sqlx, tonic |
| `nineveh-decode` | tokio, sqlx |
| `nineveh-ingest` | sqlx |
| `nineveh-store` | tonic, `nineveh-ingest` |
| `nineveh-pipeline` | axum |
| `nineveh-api`, `nineveh-realtime` | `nineveh-ingest`, `nineveh-engine` |

## Consequences

- The headline test becomes cheap to run: for random batch boundaries and random crash
  points, the final state must equal a single clean pass. It runs against an in-memory
  `StateView`, with no database and no network.
- A single fold task caps per-project fold throughput. That's acceptable because the
  measured bottleneck is expected to be database writes, which coalescing addresses.
