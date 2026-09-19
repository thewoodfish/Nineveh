# 0021. Acquire the chain once per network; replay projects from their own records

- Status: Proposed
- Date: 2026-09-19

## Context

The control plane runs a pipeline per project. `nineveh-control/src/plane.rs` opens
with it — "many projects in one process, each with its pipeline supervised" — and
`spawn()` starts a task per project, each holding its own gRPC stream and its own
cursor. With N hosted projects, Nineveh holds N concurrent Transaction Stream
connections on the plane's Geomi key. That key belongs to whoever runs the plane:
`nineveh-cli/src/main.rs` takes `APTOS_API_KEY_TESTNET`, `_MAINNET` and `_DEVNET` as
process arguments, and no project carries a key of its own.

Geomi meters the Transaction Stream three ways, and they answer different questions:
**bytes streamed** (what it bills), **concurrent streams** (an organization-wide cap on
how many may be open at once, shared across gRPC and WebSocket, 429 on exceed), and
**Compute Units** (the REST and Indexer APIs, which this pipeline barely touches).

Measurements settle which of those matters (`docs/research/spike-a-stream.md`,
2026-09-19). One mainnet window of 20,000 transactions: 17,437 bytes each, over 147.8
seconds of chain time — about 190 GiB a day, 5.6 TiB a month. At Geomi's confirmed
$0.00255/GiB, streaming the **entire unfiltered mainnet firehose costs about $15 a
month**, and the wire is zstd so the billed figure is lower again.

So bytes are not the constraint. **Concurrent streams are**, and being a count rather
than a bill they cannot be bought per customer: a design that opens a stream per
project has a ceiling on hosted projects that no amount of revenue moves.

The same measurements kill the obvious fix. Warehousing the firehose so that replay
never touches the vendor means storing 190 GiB a day — roughly $115 a month for every
month retained, compounding, against $15 a month to re-read the same data. **Storing
the chain costs about eight times what re-streaming it costs.** Any design that keeps
the raw stream to avoid re-reading it has the economics backwards.

Two existing decisions shape what is left.

**ADR 0004: server-side filters can't select write set changes.** A stream serving
several projects carries the union of what they follow, and one project with a
`resource:` or `table:` source drags that union to everything. A shared stream is an
unfiltered stream in any realistic tenancy — which the price makes a non-issue.

**ADR 0016: a rebuild is an ordinary build from `start_version`.** Today that means
every config change re-streams the project's entire history. A deep rebuild takes
hours and holds a stream the whole time. Rebuilds are not rare: they are the normal
consequence of editing a rule, which is the thing Studio exists to make easy. Replay
demand, not first-time backfill, is what actually strains the stream cap.

## Decision

**Acquire once per network.** The plane opens a single unfiltered Transaction Stream
per network it serves and reads it once. Ingest cost becomes a fixed line per network,
flat in the number of customers, and the marginal ingest cost of one more project is
zero.

**Match before decode.** Decoding is per-project, against the layouts pinned in that
project's `nineveh.lock` (ADR 0010), so a shared reader would otherwise decode every
transaction once per project. Each project declares the addresses and type tags its
sources follow; the reader matches the raw payload against them and hands a transaction
only to the projects that want it. Matching reads addresses and type tags and never
parses a Move value.

**Keep each project's records, not the chain.** As records are decoded for a project,
they are appended to that project's own record log, in version order. The log holds
what that project's sources matched — nothing else. A contract's own traffic is a
rounding error against 11.7M versions a day, so this is small, it grows with the
customer's own activity rather than with the chain, and it is therefore a fair thing to
meter.

**Replay from the record log.** A rebuild (ADR 0016) and a new state table over
existing sources both read the record log rather than the network. Editing a rule stops
costing a full re-stream and becomes local I/O. The fold is unchanged: same inputs, same
order, same single writer, so replay stays deterministic (ADR 0005).

**The log is keyed by source, not by rules.** It records what a source matched, not what
any rule did with it, so changing rules never invalidates it. Adding a *new* source is
the exception: there is no history for something that was never followed, so it needs a
backfill. The rule contributors follow: **changing rules replays locally; adding a
source backfills.**

**Backfill takes a slot from a small fixed pool.** A project starting before its record
log begins — a new project, or a newly added source — takes a stream from a pool sized
so that

```
(networks served × 1) + backfill slots  <  the organization's concurrent-stream limit
```

holds with headroom. It fills its record log, then joins the shared stream. Concurrent
streams is then a constant of the deployment, and the only work competing for the
scarce resource is first-time history — which is exactly the operation worth charging
for.

**Fan-out is per-project bounded and never blocks the reader.** Each project has a
bounded queue. A project whose fold falls behind does not slow the shared reader or any
other project: when its queue fills it is detached onto a backfill slot to catch up and
rejoins at the tip. Dropping a record is not an option — ordered, exactly-once delivery
per project is what makes the fold replayable, and a gap would break it silently.

**A deterministic failure still halts one project only.** The reader owes every project
the same transactions; a project that halts stops consuming and is detached. This is a
property to test rather than assume: the shared reader makes it possible to take the
plane down with one bad project, which a stream per project prevented by construction.

## Alternatives considered

**Keep a stream per project.** Simplest, and what exists. Rejected because the
concurrent-stream cap is a ceiling that pricing cannot lift — hosted project count
would be bounded by a Geomi system limit rather than by demand.

**Ask Geomi to raise the cap.** Buys time, not a design. The limit is organization-wide
and shared with WebSocket connections; a product whose scaling story is "ask for a
bigger number" has no scaling story.

**Store the raw firehose and replay everything locally.** Attractive until measured:
190 GiB a day, about eight times the cost of re-reading, compounding with retention,
and 115 TiB to hold the chain to date. Rejected on arithmetic. Per-project records get
the same benefit — local replay, no vendor in the rebuild path — at a size set by the
customer rather than by the chain.

**Group projects into filtered streams by what they follow.** Fewer bytes, but bytes
cost $15 a month. It would reintroduce a stream count that grows with the variety of
what customers follow, which is the number this decision exists to make constant.

**Require each project to bring its own Geomi key.** Moves both meters to the customer
and makes the cap their problem. Rejected as the default: the product's claim is that
there is nothing to run and nothing to configure, and a key is both. It remains the
right escape hatch for unusually deep history, where the customer is asking for the
expensive thing.

## Consequences

Ingest stops scaling with customers. The stream bill is a fixed line per network, so
the marginal cost of a project is Postgres and CPU — which is what makes a free tier
affordable, and what makes "we don't charge you per on-chain event" true rather than
aspirational.

Editing a config stops being expensive. A rebuild reads the record log instead of the
chain, so the shadow rebuild in ADR 0016 keeps its semantics and loses its price.

Record-log retention becomes a real policy, and a tier dimension. How far back a
project can rebuild without touching the network is exactly how much of its record log
is kept. It should be decided alongside outbox retention rather than separately: both
are "how much history does this tier keep".

Project count stops standing for a scarce connection. It remains a meter for fold CPU,
schemas and storage, so it stays a sensible tier dimension for those reasons.

Fairness becomes the plane's problem. Per-project queues, detach-and-catch-up and
failure isolation were free with a stream per project and now have to be built and
tested.

`nineveh-pipeline` gains a reader that is not owned by one project. The crate boundary
in ADR 0005 holds — ingest → decode → fold stays one-way — but the ingest end is shared
and the fold end is many. Supervision in `plane.rs` moves from "one task per project,
each reading" to "one reader per network, many folds".

## Open questions

- The organization's actual concurrent-stream limit, with and without a payment method.
  It sets the backfill pool size and is the one number this design still needs.
- Where the record log lives: Postgres alongside the state, or an append-only file per
  project. Postgres is simpler and already transactional with the commit; a file is
  cheaper per byte and easier to age out.
- Whether a project's existing `log:` state tables can serve as its record log where
  the sources happen to line up, or whether the record log is always separate.
