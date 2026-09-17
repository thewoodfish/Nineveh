# Nineveh — Project Brief

> Name: **Nineveh** — after the ancient Assyrian capital and the Library of
> Ashurbanipal, the great archive that turned an empire's raw records into organized,
> retrievable knowledge — which is what this does to the chain's firehose. The `nineveh`
> crate name appears free on crates.io; reserve it early. Binary: `nineveh`; workspace
> crates: `nineveh-*`.

## One line

The reactive backend for Aptos apps: point it at your contract, describe the state you
want, and get a live queryable database (REST + GraphQL) with real-time subscriptions —
kept in sync with the chain, no infrastructure to run. Firebase/Supabase for Aptos.

## The problem

Every Aptos app needs its own contract data in a shape it can query — feeds, balances,
leaderboards, histories, joins. Aptos gives you none of that. Its first-party state APIs
are point-reads only: a resource at an address, a view function, a table item by key.
No aggregates, no joins, no history, no "everything matching X." That's by design —
on-chain storage costs gas, so the rich shape of an app's data lives in **events and
writeset changes**, not in queryable state.

So every team rebuilds the same backend: stream the chain, decode it, fold it into
tables, serve an API, keep it live. The two first-party options are both dead ends for
most teams:

1. **Geomi no-code indexing** — hosted but shallow: declarative field-mapping and simple
   upserts over events, primitive types only, no generic-typed events, no computation,
   no joins. Reactive features are roadmap, not shipped.
2. **The raw Indexer SDK** — full power, but you clone a Rust repo, run your own
   Postgres, hand-write decode logic, operate the processor forever, and — per Aptos'
   own self-hosting docs — get no API attached. Aptos' Python/TS SDKs are flagged
   unreliable at volume, so anything production-grade means writing Rust.

The team that has a real contract, isn't a Rust shop, and doesn't want to run infra has
nowhere to land. Nineveh is that backend.

## The model

Event sourcing with materialized read models (CQRS):

- The **chain is the source of truth** — its events and resource/writeset changes.
- **Reducers** fold those inputs into **state tables**. Reducers are the *only* writer
  of state (single-writer, always).
- Apps **query** state tables and **subscribe** to changes. Reads never touch the log.

Single-writer is the keystone decision: because state changes in exactly one place,
replay is deterministic and reactivity is nearly free (fire the notification at the
reducer commit). Four consequences shape the build:

- **Sources are events, resources AND tables.** A `source` is `event: X::Y::Z`,
  `resource: X::Y` or `table: X::Y.field`. Many contracts barely emit events and expose
  real state only as resource writes — events alone under-cover the data. And state kept
  in `Table`/`SmartTable` never appears as a resource write at all — resources alone
  under-cover it too (ADR 0003).
- **Per-key ordering.** Read-modify-write reducers must apply a key's events in strict
  version order, exactly once. Parallelize by state key, never across one.
- **No reorgs.** Aptos has BFT deterministic finality; committed txns don't revert. The
  forward-only model is correct — no reorg-rewind machinery. Safety = version cursor +
  idempotent restart.
- **State is a read-only projection.** Writable app tables / auth (full Supabase scope)
  is a separate, later bet.

## What Nineveh is

A service (self-hostable and hosted) that:

1. Ingests the Aptos **Transaction Stream** (gRPC) with cursor tracking, backfill, resume.
2. **Decodes** events, resource changes and table items — including nested structs,
   `vector<Struct>`, generic types that Geomi can't — into typed records. The stream
   delivers Move values as fullnode-rendered JSON (not BCS); Nineveh decodes it against
   struct layouts pinned in `nineveh.lock`, so decoding is exact and never touches the
   network (ADR 0002).
3. Runs user-defined **reducers**: deterministic folds into materialized state tables.
4. **Materializes** state into Postgres; schema generated from config, safe upgrades.
5. Auto-exposes **REST + GraphQL** over the state tables (Supabase-style).
6. Emits **real-time change feeds / subscriptions / webhooks** on reducer commit.
7. Lets non-Rust teams define all of it in a declarative **YAML config** or visually in
   **Nineveh Studio**, with a typed escape hatch for custom logic.

## Nineveh Studio (the UI)

A clean, fast, startup-grade dashboard in the Firebase/Supabase-Studio spirit. This is
the primary surface for non-Rust teams and the centerpiece of the grant demo — a slick
live UI drives adoption and demos far more than a CLI, and adoption is the grant gate.

Studio lets a user:
- create a project; define sources (events + resources) and reducers visually;
- browse state tables as a live data grid, watching rows update in real time;
- run a REST/GraphQL playground against their own state;
- configure subscriptions and webhooks;
- monitor the processor: version cursor, lag, backfill progress, logs, errors;
- manage API keys.

Design direction: minimal and confident, opinionated defaults over knobs, fast
interactions, no clutter. Tech: Next.js/React + Tailwind + TypeScript, talking to
`nineveh-control` and the project's own API.

## Architecture

```
Aptos Transaction Stream (gRPC)
        │  ordered txns by version, auth token
        ▼
   nineveh-ingest ──► cursor/version tracking, backfill, resume, backpressure
        │
        ▼
   nineveh-decode ──► JSON → typed records against pinned layouts (events, resources, tables)
        │
        ▼
   nineveh-engine ──► reducers: pure deterministic fold → change set
        │                     (single-writer, per-key ordered, replayable)
        ▼
   nineveh-store  ──► one PG txn: state + outbox + cursor; schema from config; shadow rebuilds
        │
        ├──► nineveh-api      ──► REST + GraphQL over state tables
        ├──► nineveh-realtime ──► tails the outbox: subscriptions / signed webhooks
        └──► nineveh-control ──► project mgmt, keys, status  ◄── studio/ (web UI)
```

## Config example

Both source kinds; a reducer that folds them into a state table:

```yaml
name: vault
network: mainnet
start_version: auto          # `nineveh init` resolves this to the module's publish version

sources:                     # field types come from nineveh.lock, fetched once by `nineveh init`
  deposits:    { event: 0xABC::vault::DepositEvent }
  withdrawals: { event: 0xABC::vault::WithdrawEvent }
  vaults:      { resource: 0xABC::vault::Vault }          # writeset change, not an event
  positions:   { table: 0xABC::vault::Vault.positions }   # Table<address, Position> items

state:
  balances:
    key: [user]
    columns:
      user:    address
      balance: { type: u128, default: 0 }
    reduce:
      - { on: deposits,    set: { balance: "balance + amount" } }   # typed, exact, sandboxed
      - { on: withdrawals, set: { balance: "balance - amount" } }   # underflow halts, located
  vaults:
    mirror: vaults           # last-write snapshot per address; deleted on DeleteResource

api:      { rest: true, graphql: true }
realtime:
  - on: balances.changed
    webhook: https://myapp.example/hooks/balance
```

Reducer expressions run in a typed, total, sandboxed evaluator — exact integer arithmetic
up to `u256`, no floats, no wall-clock, RNG or I/O (ADR 0007) — so `nineveh replay`
reproduces state exactly.

## Scope

Full scope up front, sequenced so the first slice is a real product (the grant needs a
*live product with real adoption*, not a demo).

**First shippable slice**
- Ingest + resume + backfill (testnet/mainnet).
- Decode: events, resource changes AND table items (`Table`, `SmartTable`); primitives,
  nested structs, `vector<Struct>`, generic types (beat Geomi's ceiling on day one).
- Declarative reducers: upsert-by-key, sum/count, last-write, min/max, simple joins.
- Postgres state tables + config-generated schema + safe upgrades.
- Auto REST + GraphQL API.
- Row/table-level change feeds + webhooks (transactional outbox, ADR 0006).
- CLI (`nineveh`): scaffold / validate / run / replay.
- **Studio v1**: create project, define sources+reducers, live state-table browser,
  API playground, processor status. This is in the first slice, not after it.
- Self-hostable via Docker; one hosted reference deployment.

**Then**
- Windowed / time-bucketed aggregations; multi-source joins.
- Typed Rust escape hatch for custom reducers.
- Full hosted control plane: usage, metrics, team accounts.
- Live-aggregate-query reactivity (incremental view maintenance) — hard; only on top of
  a streaming engine, and only if demand is real.
- Writable app tables / auth (the full-backend bet) — separate decision.

**Explicitly out of scope (v1)**
- Live aggregate-query subscriptions; writable/auth tables; wallets, gas stations,
  generic analytics, NFT tooling (first-party portal's lane).

## Moat and competitive risk

**Moat:** decoding events + resources + table items including complex Move types, the computational
reducer engine, deterministic replay, Rust-grade reliability, the instant API, and a
genuinely good Studio — as one integrated backend. Depth and UX compound; hosting alone
does not.

**Live risk:** the lane is adjacent to the house. If Geomi extends no-code upward or
ships hosted custom processors, the shallow end compresses. Mitigation: keep the
defensible core — resource+event decode coverage, computation depth, replay,
reliability, and the Studio experience — ahead of what a no-code portal will build, and
treat "hosted indexing" as table stakes, never the pitch. Watch Geomi's changelog and
the `aptos-indexer-processors` repo.

## Grant strategy

- **Target:** Aptos Foundation **Ecosystem Grant** ($5K–$50K, milestone-based) — wants a
  live product with real adoption, prefers Aptos-native open-source dev infrastructure.
  A reactive backend with a real Studio is exactly that shape.
- **Sequence:** build → testnet → land 3–5 real integrations (adoption is the gate) →
  apply with traction, framing the problem in Aptos' own words (point-read-only state
  APIs; no API-attach path; unreliable non-Rust SDKs; no-code ceiling).
- **Ladder:** Ecosystem Grant → SecCreds (up to $25K audit) → Google Cloud credits →
  potentially LFM. Verify the Payments Grant's application status before leaning on it.
- **Positioning:** open-source dev infrastructure that *complements* the first-party
  stack. "The reactive backend Aptos developers were missing."
- **Note:** track-2 bet, orthogonal to your main company. Keep it ecosystem-native;
  multichain only as a later act, kept out of the grant narrative.

## Decisions (resolved 2026-09-14)

Recorded as ADRs in `docs/adr/`:

1. **Reducer expression language:** a small, typed, total language (`nineveh-expr`),
   with `reduce: { wasm: … }` reserved for a later escape hatch (ADR 0007). CEL was
   rejected: its 64-bit integers can't represent `u128`/`u256`.
2. **Resource decode strategy:** layouts fetched once from module ABIs by `nineveh init`
   and pinned in `nineveh.lock`; the stream's JSON is decoded against them (ADR 0002).
   Fixtures from real transactions back every rendering convention.
3. **Studio ↔ backend boundary:** config stays the source of truth; Studio edits config.
4. **Hosted vs. self-host for the grant demo:** both — self-host (`docker compose up`)
   in M3, a hosted reference deployment in M4.

Also decided: own the stream client and vendor the protos (ADR 0001); table items as a
source kind (ADR 0003); filter server-side only for event-only projects (ADR 0004); pure
fold + atomic commit (ADR 0005); outbox change feeds (ADR 0006); `NUMERIC` for
`u64`–`u256` (ADR 0008); Rust 2024 and the engineering baseline (ADR 0009).
