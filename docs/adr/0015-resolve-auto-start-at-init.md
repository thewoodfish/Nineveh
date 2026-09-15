# 0015. Resolve `start_version: auto` at init from the Indexer API, pinned in the lock

- Status: Accepted
- Date: 2026-09-15

## Context

`start_version: auto` is the default. It promises to start at the version that
published the project's modules, so nothing earlier is scanned and nothing relevant is
missed. ADR 0012 depends on that promise: table items are attributed by handle, and a
handle is only learned by seeing its parent written, so a build has to start before
the first parent was created.

Finding that version is harder than it looks:

- The fullnode REST API is pruned. On 2026-09-15 testnet served versions from about 11.06
  billion only (`docs/research/spike-a-stream.md`). Code published to an object has no
  account transactions at all.
- The Transaction Stream can't be queried by address. Finding a publish by scanning
  would take days on mainnet.
- Aptos Labs' hosted Indexer API keeps `account_transactions`: every transaction that
  touched an address, from genesis. On testnet, `0x1` first appears at version 0 and
  the market contract `0x0e3117b…` at 5,774,816,547, before any of its events.

A resolved start also has to be stable. Replay must rebuild the same state, and the build
fingerprint (ADR 0014) covers the config and the lock, not a network lookup made at run
time.

## Decision

- `nineveh init` resolves `auto` to the **earliest first transaction, per the Indexer
  API, of every address whose types the sources name**: the event or resource
  struct's address, or for a `table:` source, the parent struct's. Any transaction that
  publishes a module writes to its address, so this is at or before every source
  module's publish. Starting earlier is safe: those versions just hold nothing the
  sources select.
- The result is **pinned in `nineveh.lock`** as `start_version` (lock format 3, a
  decimal string). Builds, restarts and replays read it from there and never ask the
  network again. Running `nineveh init` again can re-resolve it, and since the lock is
  in the fingerprint, a different start means a rebuild.
- If an address has no transactions, `init` fails and asks for an explicit
  `start_version`. So does an endpoint without an Indexer API.
- `nineveh run` refuses `auto` without a pinned start, rather than guessing.

## Alternatives considered

- **Scan the stream for the publish transaction.** Correct and self-contained, but it
  takes days on mainnet for every new project.
- **REST account transactions.** Pruned, and empty for object-deployed code.
- **Resolve at every run.** Needs the network at run time, and a replay could start
  somewhere else than the build it replaces.
- **Start at the oldest version REST serves.** This would silently miss everything
  before it, including parents whose handles table sources need.

## Consequences

- `init` depends on the hosted Indexer API; decoding and running don't. Projects on
  other endpoints set `start_version` explicitly.
- Correctness of `auto` now rests on the indexer's `account_transactions` being
  complete for the address. If it started late, the build would miss early data.
  A later check could confirm the first transaction the stream delivers at the pinned
  start matches the indexer's.
- Sources on framework types (`0x1::…`) resolve to genesis, a full-chain backfill. That
  is what such a source asks for. Parallel backfill (the pipeline's `Parallel`) is how
  it stays tractable.
