# 0014. Store each build in its own schema, keyed by exact bytes and fingerprinted

- Status: Accepted
- Date: 2026-09-15

## Context

`nineveh-store` commits the fold's `ChangeSet` (ADR 0005) and reads rows back for the
next batch. The fold and the API want different things from the same row:

- The API needs typed columns (ADR 0008), such as `numeric` for wide integers and
  `jsonb` for structs.
- The fold needs back exactly what it wrote. A `jsonb` column forgets Move types (a
  `u8` and a `u64` are both a string in JSON), and Postgres `text` and `jsonb` can't
  hold U+0000, which a Move `String` can. A key can be any Move value, structs
  included, so there's no natural typed primary key.

A schema also has to know what it was built from. If the config, the lock or the fold's
semantics change, extending an existing build would mix two different projections
(ADR 0005).

Building the store settled two details of ADR 0006. ADR 0013 already records every
change to a row in order, so that the feed doesn't depend on batching. ADR 0006 said
the outbox would coalesce changes per key per batch and number them per commit. Both
would make the feed depend on where batch boundaries fall.

## Decision

**One schema per build.** A project's state lives in a Postgres schema of its own, one
table per state table. Nineveh's own tables live in the `nineveh` schema, created by
migrations:

- `nineveh.projects` has one row per state schema, holding its fingerprint and cursor.
- `nineveh.changes` is the outbox.

`nineveh`, `public`, `information_schema` and `pg_*` can't be state schemas.

**Table layout.** Every state table has these columns:

| Column | Holds |
| --- | --- |
| `_key` | the engine's key in an exact binary encoding; the primary key |
| `_row` | the engine's row, same encoding; only on tables the fold reads (`reduce`, `mirror`) |
| `_version` | the version of the row's last change |
| typed columns | the row as the API serves it (ADR 0008) |

- The encoding (`nineveh-store/src/codec.rs`) is one-to-one: two values encode to
  the same bytes exactly when they're equal. That lets `_key` identify rows whatever
  the key's shape.
- The fold only reads `_key` and `_row`. The API only reads typed columns.
- Typed text and JSON replace U+0000 with U+FFFD. The exact value stays in `_row`.
- `log` tables are append-only, so they keep no `_row`.
- A non-unique index covers the typed key columns, for API lookups.
- The engine's internal tables, `_handles` and `_buckets` (ADR 0013), live in the same
  schema, so a rebuild reproduces them.

Typed columns for `reduce` tables are the ones the config declares. `mirror` and `log`
tables get theirs from the source's layout: Nineveh's key columns first, then one
column per field of the value's struct.

| Table | Key columns |
| --- | --- |
| `mirror` of a resource | `address`, plus `type` for an any-instance source |
| `mirror` of a table | `handle`, `key` |
| `log` | `version`, `event_index` |

A non-struct table value is stored in one `value` column, and so is an enum. A field
whose type depends on unbound type arguments is stored as `json`. A field whose name
isn't a valid column name, or clashes with a key column, is a config error that
suggests `reduce` instead.

ADR 0008 predates signed integers (ADR 0007). They map by width:

| Move | Postgres |
| --- | --- |
| `i8`, `i16`, `i32` | `integer` |
| `i64` | `bigint` |
| `i128` | `numeric(39,0)` |
| `i256` | `numeric(77,0)` |

**Commit.** One transaction per batch does the following:

1. Locks the project row and checks that the cursor is still the one this store last
   saw.
2. Applies each table's deletes and one `UNNEST` upsert.
3. Inserts the outbox rows.
4. Moves the cursor.
5. Calls `pg_notify('nineveh_changes', <schema>)`, which is delivered only if the
   transaction commits.

If the cursor moved, another writer is committing to the project. The commit fails
with `CursorMoved` rather than interleaving two folds. A batch that doesn't come after
the cursor is refused as `Stale`.

**Outbox granularity** (supersedes ADR 0006 on this point). The outbox records every
change the fold emits, in order, not one coalesced change per key per batch. `seq`
numbers each *version's* changes from zero, not each commit's. The feed and each
change's `(schema, version, seq)` identity don't depend on batching. That makes
`(schema, version, seq)` a stable idempotency key for webhooks across crashes and
re-batching. State writes are still coalesced per key, as ADR 0005 requires.

**Fingerprint.** Each build records a SHA-256 over these inputs, each length-prefixed:

- the store's layout version;
- the fold's semantics version (`nineveh_engine::SEMANTICS_VERSION`);
- the config's canonical form (`Config::canonical`), which covers the network, start
  version, sources and state tables, and leaves out names, comments, formatting,
  `api` and `realtime`;
- the lock.

`Store::open` refuses a schema whose fingerprint differs, with `Rebuild`. In M1, a
rebuild is `Store::reset` followed by replay from `start_version`. ADR 0005's
shadow-schema build and atomic swap still applies. It lands with the pipeline, and a
build can already target any schema name.

## Alternatives considered

- **Typed primary keys only.** These can't represent struct or vector keys. They also
  collide where exact values differ, such as strings that differ only by a NUL.
- **Rebuilding the fold's rows from typed columns.** This loses Move types inside
  `jsonb` and loses NULs in text, so replay from the database wouldn't equal replay in
  memory.
- **One shared schema with a project column.** Every query would need a filter, and
  the per-schema `SELECT` grant (ADR 0005) wouldn't be possible. A rebuild couldn't
  build beside the live data and swap.
- **Coalescing the outbox per batch (ADR 0006 as written).** Subscribers would see a
  different feed depending on batch size and crash points. Webhook idempotency keys
  would stop being stable across a re-fold.
- **Hashing the raw `nineveh.yaml`.** A comment or a reformat would force a full
  rebuild.

## Consequences

- The replay property now holds through Postgres. For random workloads, batch
  boundaries and crashes, the stored rows and the outbox equal a single in-memory
  pass (`nineveh-store/tests/store.rs`).
- Each row is stored twice, once exactly and once typed. That costs space in exchange
  for exact replay and a lossless API.
- The outbox grows with every change, not every key. Pruning is still ADR 0006's
  retention policy.
- Changing the encoding, the table shapes or the fold's semantics means bumping
  `LAYOUT_VERSION` or `SEMANTICS_VERSION`. Existing builds then ask for a rebuild
  instead of being misread.
- Store queries on `nineveh.*` are checked at compile time against `.sqlx/`. Rerun
  `scripts/sqlx-prepare.sh` whenever a `query!` or migration changes.
