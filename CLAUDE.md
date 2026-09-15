# CLAUDE.md

Operating manual for this repo. Read the "Model" and "Constraints" sections before
writing code — the architecture has sharp edges that aren't obvious from the types.

## What Nineveh is

Nineveh is a **reactive backend for Aptos apps**. Think Firebase / Supabase, not The
Graph. You point it at your contract, describe the state you want in a simple config,
and you get a live queryable database (REST + GraphQL) plus real-time subscriptions —
kept continuously in sync with the chain, with no infrastructure to run.

**Why it's needed.** Aptos' first-party state APIs are point-reads only: a resource at
an address, a view function, a table item by key. You cannot ask for aggregates, joins,
history, feeds, or "everything matching X" — on-chain storage is deliberately minimal
(it costs gas), so the rich shape of an app's data lives in **events and writeset
changes**, not in queryable state. Geomi's no-code indexer only does shallow
field-mapping over events; the raw Indexer SDK is Rust-only, self-operated, and (per
Aptos' own docs) gives you no API. Nineveh is the backend that sits in that gap.

## The model (this is the core — internalize it)

Event sourcing with materialized read models (CQRS):

- The **chain is the source of truth.** Its events and resource changes are the inputs.
- **Reducers** fold those inputs into **state tables**. Reducers are the *only* thing
  that ever writes state — single-writer, always. Everything downstream depends on this.
- Reads hit the **state tables**, never the raw log. Apps query state and subscribe to
  changes.

Why single-writer matters: because state changes in exactly one place (a reducer
commit), replay is deterministic and reactivity is nearly free — the commit is the one
place a change notification fires. Never let anything else write a state table.

## Core concepts (use this vocabulary everywhere)

- **Transaction Stream** — Aptos' gRPC firehose of transactions
  (`grpc.{network}.aptoslabs.com:443`, Geomi API key required — anonymous requests are
  rejected). Ordered by version. Move values arrive as **JSON strings rendered by the
  fullnode, not BCS** (ADR 0002).
- **Version** — monotonic transaction number. The processing cursor. Everything is
  replayable from a `start_version`.
- **Source** — an input the user subscribes to. THREE kinds, all first-class:
  - `event:` — a Move `#[event]` type.
  - `resource:` — a resource / writeset change. **Required**, not optional: many
    contracts barely emit events and expose their real state only as resource writes.
    Events alone will silently under-cover the data.
  - `table:` — items of a `Table` / `SmartTable` held in a resource field. Table state
    never appears as a resource write (the parent holds only a handle), so resources
    alone under-cover it too (ADR 0003).
- **Reducer** — a deterministic fold from source records into a state table. Pure,
  replayable, and the sole writer of state.
- **State table** — a materialized read model in Postgres, written only by reducers,
  exposed via auto-generated REST + GraphQL (Supabase-style).
- **Project** — one Nineveh backend: sources + reducers + state tables + API/webhooks.

## Repo layout

Rust workspace (`crates/nineveh-*`) plus a `studio/` web app. Crates are created when
they get real code; the dependency direction between them is enforced by
`scripts/check-deps.sh` (ADR 0005).

- `nineveh-core`     — shared domain types: Version, Network, Address, TypeTag, exact
                       U256/I256, decoded Move `Value`. No I/O, no async.
- `nineveh-proto`    — Transaction Stream message types (prost only) generated from the
                       vendored, pinned protos in `proto/` (ADRs 0001, 0010).
- `nineveh-ingest`   — Transaction Stream gRPC client (tonic). Ordered, gap-checked
                       delivery; backpressure.
- `nineveh-decode`   — type-directed decoding of the stream's JSON-rendered Move values
                       (events, resources, table items) against layouts pinned in
                       `nineveh.lock` (ADR 0010). Nested structs, enums, generics.
- `nineveh-expr`     — the reducer expression language: typed, total, exact integers
                       u8–u256 and i8–i256 (ADR 0007; user reference in
                       `docs/expressions.md`). Config resolution compiles every rule.
- `nineveh-engine`   — the pure fold: `StateView × Records → ChangeSet`. No I/O.
                       Deterministic, replayable (ADRs 0005, 0012, 0013); the replay
                       property test is `tests/replay.rs`.
- `nineveh-pipeline` — wires ingest → decode → fold → commit with bounded channels,
                       supervision, retries, metrics.
- `nineveh-store`    — Postgres via sqlx. A schema per build generated from the project
                       config; the one atomic commit of rows, outbox and cursor; build
                       fingerprints (ADR 0014).
- `nineveh-api`      — axum REST + async-graphql over state tables, generated from config.
                       Reads state only, never the chain.
- `nineveh-realtime` — change feeds + subscriptions + signed webhooks, tailing the
                       transactional outbox written by the reducer commit (ADR 0006).
- `nineveh-control`  — the control plane Studio drives (ADR 0017): contract catalog
                       and config scaffolding, `pin` and the `Runner` the CLI uses too,
                       and `ControlPlane`, which runs many projects from the registry in
                       `nineveh.control_projects`, each served at `/projects/{name}/v1`.
                       Aptos access goes through the `Chain` trait (`Hosted` in prod).
- `nineveh-config`   — parse/validate `nineveh.yaml`, then resolve it against the lock
                       (ADR 0011; user reference in `docs/config.md`). Located,
                       rustc-style diagnostics. Product UX for non-Rust teams.
- `nineveh-cli`      — the `nineveh` binary: `init` (pin ABIs, resolve `start_version:
                       auto`, ADR 0015), `validate`, `run [--serve]` (parallel backfill,
                       then the tail; shadow rebuild on config change, ADR 0016),
                       `serve` (API + change feed on 127.0.0.1:4000), `replay`, and
                       `up` (the control plane on 127.0.0.1:4000).
- `nineveh-testkit`  — shared test workloads: a synthetic vault contract rendered as
                       real stream transactions, with a model to check against.
                       Dev-dependency only.
- `studio/`          — the dashboard (Next.js/React + Tailwind, TypeScript). NOT a Rust
                       crate. Talks to `nineveh-control` + the project's API. See below.
- `xtask/`           — repo automation (`cargo xtask codegen [--check]`). Not published.
- `fixtures/`        — real stream transactions (`<network>/*.pb`) and the trimmed module
                       ABIs they need (`abi/<network>/`), for offline decode tests.
- `docs/adr/`        — architecture decision records. Read the relevant ADR before
                       changing a decision; supersede it with a new ADR, don't edit it.

## Nineveh Studio (the UI — a first-class deliverable)

A clean, fast, opinionated dashboard in the Firebase/Supabase-Studio spirit. It is the
main surface for non-Rust teams and the centerpiece of any demo, so it carries real
product weight — not an afterthought.

It must let a user:
- create a project and define sources (events, resources, tables) and reducers visually;
- browse state tables like a data grid, and watch rows update live;
- hit a REST/GraphQL playground against their own state;
- configure webhooks/subscriptions;
- see processor health: current version cursor, lag, backfill progress, logs, errors;
- manage API keys.

Design direction: minimal, confident, startup-grade. Sensible defaults over knobs,
fast interactions, no clutter. When building UI, read the frontend-design skill first.

## Conventions

- Rust 2024, toolchain pinned in `rust-toolchain.toml`, MSRV 1.89 (ADR 0009). Async on
  tokio. gRPC: tonic. DB: sqlx on Postgres. API: axum + async-graphql. Studio: Next.js +
  Tailwind + TypeScript.
- sqlx compile-time-checked queries (`query!`) for internal tables (cursor, outbox,
  keys). State tables are shaped by config at runtime, so their SQL goes through the
  typed builder in `nineveh-store`: identifiers only from the validated config, never
  from requests; every value bound. GraphQL uses `async_graphql::dynamic`.
- Lints are workspace-wide and CI runs `-D warnings`; unwrap/expect/panic and lossy
  integer casts are denied in library code (tests exempt via `clippy.toml`).
- No `unwrap()` / `expect()` in library code — `Result` + `thiserror`. `anyhow` only at
  binary boundaries.
- Every stateful step persists the last processed `version` and resumes exactly;
  processing is idempotent per version (crash-safe, exactly-once effect).
- **Reducers must be deterministic**: same inputs in → same state out. No wall-clock,
  RNG, external calls, or network inside a reducer. This is what makes replay correct.
- Decoding is the riskiest layer. Never assume a schema; validate against real on-chain
  payloads and keep fixtures from real testnet transactions. Every rendering convention
  the decoder handles needs a fixture (`docs/research/stream-json-conventions.md`).
- Distinguish retryable from fatal errors in every error enum (`is_retryable`). A
  deterministic error (bad data, reducer underflow) halts the project at that version
  with a located error; it is never skipped silently.

## Constraints / gotchas

- **Serialize read-modify-write per state key.** `balance = balance + amount` reads then
  writes, so events for a given key must apply in strict version order, exactly once.
  Partition parallelism BY state key; never parallelize across a single key.
- **Aptos finalizes — do NOT build reorg-rewind machinery.** Aptos uses BFT consensus
  with deterministic finality; committed transactions don't reorg. The forward-only
  "input in → state updated" model is correct here. Safety = version cursor + idempotent
  restart. (This is a gift; don't import EVM reorg complexity you don't need.)
- **Reactivity has two tiers. v1 is row/table-level only.** Change feeds on a table or
  key (transactional outbox + NOTIFY wake-up, ADR 0006) are tractable now. Live *aggregate
  queries* — subscribe to `SELECT top 10 by volume` and keep the result correct as
  events stream — is incremental view maintenance, a hard systems problem (Materialize /
  RisingWave territory). Do NOT attempt it in v1; scope it as a later bet, on top of a
  streaming engine if ever.
- Chain-derived state is a **read-only projection**. Writable app tables / auth / RLS
  (full Supabase-scale backend) is a separate, much bigger bet — out of v1.
- The Aptos Indexer stack (Transaction Stream, custom processors, Indexer API) is in
  **beta**. Pin versions; keep Aptos coupling behind `nineveh-ingest` / `nineveh-decode`.
- Aptos' own Python/TS processor SDKs are unreliable at volume — Rust-grade reliability
  is a feature. Bounded channels, backpressure, no unbounded memory.
- Do NOT reimplement generic data already served by Aptos' hosted Indexer GraphQL
  (fungible assets, NFTs, tokens, ANS). Nineveh is for *custom* app state.
- **Move integers don't fit Postgres integers.** `BIGINT` is signed; `u64` above 2^63−1
  overflows it. `u64`/`u128`/`u256` are `NUMERIC` and serialize as strings in the API
  (ADR 0008).
- **Server-side stream filters can't select write set changes** — only events, senders
  and entry functions. Filter only event-only projects; anything with a resource or
  table source streams unfiltered (ADR 0004).
- **One stream is slow for deep backfill: ~3.5–11k versions/s, filtered or not** (filters
  save bandwidth, not server scan time). All of mainnet is ~2 weeks on one stream. Deep
  backfill = parallel streams over disjoint version ranges, reassembled in version order
  before the single fold task; and `start_version: auto`. Always request zstd — 12–22×
  faster than uncompressed. Numbers: `docs/research/spike-a-stream.md`.
- **Object deletes arrive as one `DeleteResource` of `0x1::object::ObjectGroup`**, not
  per member. Group-member sources must map a group delete to member deletes.

## Don't

- Don't let anything but a reducer write a state table.
- Don't put wall-clock, RNG, or network calls inside reducers.
- Don't build reorg/rewind logic — Aptos finalizes.
- Don't attempt live-aggregate-query reactivity in v1.
- Don't let the API read the chain directly — state tables only.
- Don't widen scope into wallets, gas stations, generic analytics — first-party lane.

## Commands

- `cargo xtask codegen` — regenerate Transaction Stream bindings from vendored protos;
  `--check` fails on drift (CI). Bump protos with `scripts/sync-protos.sh <sha>`.
- `scripts/check-deps.sh` — enforce crate dependency direction (CI).
- `NINEVEH_TEST_DATABASE_URL=postgres:///nineveh_test cargo test -p nineveh-store -p nineveh-pipeline` —
  the Postgres-backed tests (they skip without it, and fail in CI without it).
  Don't set `DATABASE_URL`: it switches sqlx's macros to checking against a live DB.
- `scripts/sqlx-prepare.sh` — regenerate `crates/nineveh-store/.sqlx/` after changing
  a `query!` or a migration; commit the result (CI builds with `SQLX_OFFLINE=true`).
- `cargo deny check` — advisories, licenses, sources (CI).
- Studio: `cd studio && npm install && npm run dev` (http://localhost:3000), against
  `nineveh run --serve` on 127.0.0.1:4000. CI runs `npm run typecheck` and `npm run build`.
  Next 16 differs from older Next: read `studio/AGENTS.md` before changing Studio.
- `nineveh init | validate | run [--serve] | serve | replay --yes` (`cargo run -p
  nineveh-cli --`) — in a
  directory with `nineveh.yaml`. Needs `APTOS_API_KEY` (init, run) and
  `NINEVEH_DATABASE_URL` (run, replay).
- `nineveh up` — the control plane: every project in the database's registry, created
  and managed from Studio. Needs `APTOS_API_KEY` and `NINEVEH_DATABASE_URL`.
- Live checks against testnet, `#[ignore]`d in CI: `cargo test -p nineveh-ingest -p
  nineveh-pipeline -p nineveh-cli -- --ignored` with `APTOS_API_KEY` and
  `NINEVEH_TEST_DATABASE_URL` set.
- `cargo run --release -p nineveh-ingest --example stream_probe -- --network testnet
  --start <v> --count <n>` — measure the stream / find fixture candidates. Needs
  `APTOS_API_KEY` (a Geomi key).

## References

- Custom processors / Indexer SDK: https://github.com/aptos-labs/aptos-indexer-processors
- Indexer + Transaction Stream + self-hosting: https://aptos.dev/build/indexer
- Architecture decisions: `docs/adr/` (index in `docs/adr/README.md`)
