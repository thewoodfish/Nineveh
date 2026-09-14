# 0004. Filter server-side only when it can't under-cover

- Status: Accepted
- Date: 2026-09-14

## Context

`GetTransactionsRequest.transaction_filter` (`filter.proto`) supports boolean
combinations of these filters:

- `TransactionRootFilter`: success and transaction type;
- `UserTransactionFilter`: sender and entry function (address, module, function);
- `EventFilter`: event struct type and a substring match on event data.

A matching transaction is delivered. Non-matching versions are skipped, and responses
carry a `ProcessedRange` so the client can confirm coverage. There is **no filter on
write set changes.**

For event sources, an event-type filter is exact. For resource and table sources, no
filter is exact. An entry-function filter on the contract's address misses writes made
when another module calls into the contract, such as an aggregator calling a DEX. Those
writes carry the caller's entry function, not the contract's.

## Decision

- A project whose sources are **all `event:`** gets a generated server-side filter: the
  `OR` of its event struct types.
- A project with **any `resource:` or `table:` source** streams unfiltered. Correctness
  is the default.
- The contiguity check (`nineveh-ingest`) enforces coverage in both modes. Unfiltered,
  every version must arrive. Filtered, the `ProcessedRange`s must tile the version space
  with no gaps.
- We add no opt-in "fast but lossy" mode. If unfiltered cost proves prohibitive (M0
  spike A measures it on mainnet), the fix is a better filter from Aptos or our own
  pre-filtering service, never silent under-coverage.

## Evidence (M0 spike A, 2026-09-14)

See `docs/research/spike-a-stream.md` for the full numbers.

- **Unfiltered, zstd:**
  - mainnet: 3.1–3.7k txns/s on recent traffic (21–24 KB each) and 5.9–7.1k txns/s on
    older ranges;
  - testnet: 6k on recent traffic and 18–27k on older ranges.
  - Uncompressed is 12–22× slower.
- **Filtered (event-only), mainnet:** 10.5–10.9k *versions* covered per second. The
  server still scans every version, so filtering saves bandwidth, not time.
- A single-stream backfill of all of mainnet takes about 14 days unfiltered, or about 8
  days filtered. **Parallel range backfill is required either way** and moves into M1.
  So does `start_version: auto`.

## Consequences

- Event-only projects use far less bandwidth, but they backfill only 1.5–3× faster.
- Resource and table projects pay full-stream bandwidth on backfill and live tail
  (about 75 MiB/s decoded on mainnet; about 12–20× less on the wire with zstd).
- Backfill speed comes from parallel range streams, not from filters.
