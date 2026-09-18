# 0021. Share one tip stream per network; queue backfill against a fixed pool

- Status: Proposed
- Date: 2026-09-19

## Context

The control plane runs a pipeline per project. `nineveh-control/src/plane.rs` opens
with it — "many projects in one process, each with its pipeline supervised" — and
`spawn()` starts a task per project, each holding its own gRPC stream and its own
cursor. With N hosted projects, Nineveh holds N concurrent Transaction Stream
connections on the plane's Geomi key.

That key belongs to whoever runs the plane. `nineveh-cli/src/main.rs` takes
`APTOS_API_KEY_TESTNET`, `_MAINNET` and `_DEVNET` as process arguments; no project
carries a key of its own, in the config or in the database. Every hosted project
streams on the operator's key. (`nineveh.api_keys` is unrelated — those are the keys
an app presents to reach a project's REST API, ADR 0018.)

Geomi meters the Transaction Stream three ways, and they are not one bucket:

- **Bytes streamed** — what the stream is billed on. Server-side filtering is the
  lever, because it stops the bytes before they leave Aptos.
- **Concurrent streams** — an organization-wide limit on how many streams may be open
  at once, shared across gRPC and WebSocket. Exceeding it is a 429. The ceiling is
  higher with a payment method attached, but it is still a fixed count.
- **Compute Units** — the REST and Indexer APIs, which this pipeline barely touches.

The second one is the problem. It is a count, not a bill: it cannot be bought per
customer. A design that opens a stream per project therefore has a hard ceiling on
hosted projects that no amount of revenue moves, and it is reached at a number of
customers far below the number worth having.

Two facts already on the record constrain the fix.

**ADR 0004: server-side filters can't select write set changes.** A stream that serves
several projects must carry the union of what they follow, and one project with a
`resource:` or `table:` source drags that union to "everything". Resources are not a
corner case — a contract that barely emits events exposes its state only as resource
writes, which is why they are a first-class source kind at all. So a shared stream is
an *unfiltered* stream in any realistic tenancy.

**`docs/research/spike-a-stream.md`: one stream covers 3.5–11k versions/s, filtered or
not,** because the server walks every version either way. All of mainnet is about 14
days on a single stream, and the spike's own conclusion is that parallel range backfill
is required for any project starting far back. A tip-following stream cannot serve a
project that starts at an old version, so backfill needs streams of its own whatever
else changes.

## Decision

**One shared tip stream per network.** The plane opens a single Transaction Stream per
network it serves, reads it once, and fans each transaction out to the projects on that
network. Each project keeps its own fold, its own cursor and its own commit; only the
read is shared.

**The shared stream is unfiltered.** It serves the union of every project on the
network, so by ADR 0004 it cannot be filtered without under-covering someone. Ingest
bytes therefore become a fixed cost per network, flat in the number of customers, and
the marginal ingest cost of one more project is zero.

**Backfill runs against a fixed pool of stream slots, and is queued.** A project that
starts behind the tip takes a slot, catches up, and hands the slot back to join the
shared stream. The pool size is a deployment setting, chosen so that

```
(networks served × 1) + backfill slots  <  the organization's concurrent-stream limit
```

holds with headroom. Concurrent streams is then a constant of the deployment rather
than a function of the customer count, and the only thing competing for the scarce
resource is deep backfill — which is exactly the operation worth charging for.

**A project is matched before it is decoded.** Decoding is per-project, against the
layouts pinned in that project's `nineveh.lock` (ADR 0010), so a shared reader would
otherwise decode every transaction once per project. Each project declares the
addresses and types it follows; the reader matches the raw payload against them and
only hands a transaction to the projects that want it. Matching reads addresses and
type tags; it never parses a Move value.

**Fan-out is per-project bounded and never blocks the reader.** Each project has a
bounded queue. A project whose fold falls behind does not slow the shared reader and
does not slow any other project: when its queue fills it is detached onto a backfill
slot to catch up, and rejoins the shared stream when it reaches the tip. Dropping a
transaction is not an option — ordered, exactly-once delivery per project is what makes
the fold replayable (ADR 0005), and a gap would break it silently.

**A deterministic failure still halts one project only.** The reader owes every project
the same transactions; a project that halts stops consuming and is detached. This is a
property to test, not to assume: the shared reader makes it possible to take the plane
down with one bad project, which the per-project design made impossible by construction.

## Alternatives considered

**Keep a stream per project.** Simplest, and what exists. Rejected because the
concurrent-stream cap is a hard ceiling that pricing cannot lift: the number of hosted
projects would be bounded by a Geomi system limit rather than by demand or cost.

**A stream per project, but ask Geomi to raise the cap.** Buys time, not a design. The
limit is organization-wide and shared with WebSocket connections; a product whose
scaling story is "ask for a higher number" has no scaling story.

**Group projects into filtered streams by what they follow.** Event-only projects could
share a filtered stream and pay for fewer bytes. Rejected for now: it keeps the byte
bill down but reintroduces a stream count that grows with the variety of what customers
follow, which is the meter we are trying to make constant. It stays available later as
an optimization *within* the shared design — a second shared stream, filtered, for the
event-only projects — once bytes are measured and known to matter.

**Require each project to bring its own Geomi key.** Moves both meters to the customer
and makes the cap their problem. Rejected as the default: the product's claim is that
there is nothing to run and nothing to configure, and a key is both. It remains the
right escape hatch for deep backfill, where the customer is asking for the expensive
thing — recorded separately when that is built.

## Consequences

Ingest cost stops scaling with customers. The stream bill becomes a fixed line per
network, so the marginal cost of a project is Postgres and CPU only — which is what
makes a free tier affordable, and what makes "we don't charge you per on-chain event"
true rather than aspirational.

Project count stops being a proxy for stream usage. It remains a meter for fold CPU,
schemas and storage, so it stays a sensible tier dimension; it just no longer stands
for a scarce connection.

Bytes become the meter to watch. An unfiltered mainnet stream is the largest possible
byte bill, and it is paid whether one customer wants that data or a hundred do. The
number that decides whether this is affordable — whole-chain bytes per day per network
— is not yet measured. It must be, against Geomi's published pricing, before the shared
mainnet stream is switched on.

Deep backfill becomes the rationed operation. It is the only work that consumes
concurrent streams in proportion to demand, so it needs a queue, a visible position in
that queue, and a tier that governs how far back a project may start.

Fairness becomes the plane's problem. Per-project queues, detach-and-catch-up, and
failure isolation are all properties the per-project design got for free and this one
has to implement and test.

`nineveh-pipeline` gains a reader that is not owned by one project. The crate boundary
in ADR 0005 still holds — ingest → decode → fold stays one-way — but the ingest end is
now shared, and the fold end is many. The supervision in `plane.rs` moves from "one
task per project, each reading" to "one reader per network, many folds".

## Open questions

- Whole-chain byte volume per network per day, priced against Geomi's published rates.
  This decides whether an unfiltered shared mainnet stream is viable at all.
- The organization's actual concurrent-stream limit, with and without a payment method.
  It sets the backfill pool size.
- Whether an event-only shared stream (filtered) is worth running alongside the
  unfiltered one, once bytes are measured.
