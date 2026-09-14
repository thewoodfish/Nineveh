# Spike A: Transaction Stream measurements

- Date: 2026-09-14
- Tool: `crates/nineveh-ingest/examples/stream_probe.rs`, release build, one stream
- Client: macOS on a residential link, which caps uncompressed throughput. A
  data-centre host will be faster; ratios matter more than absolutes here.
- Networks: testnet (head ≈ 11,198,227,786) and mainnet (head ≈ 7,205,930,377)

## Access

- The gRPC stream rejects anonymous requests with `Unauthenticated`. A Geomi API key
  is required, sent as `authorization: Bearer <key>`.
- The REST API allows anonymous reads but returns `429` after a handful of paged
  requests. It's unusable for scanning without a key.

## Throughput

Compression comparison on the same 20,000 recent testnet transactions:

| Compression | Txns/s | Decoded MiB/s |
| --- | ---: | ---: |
| none | 269 | 5.5 (link-bound) |
| gzip | 1,900 | 38.8 |
| **zstd** | **5,994** | **122.4** |

zstd re-measured on a fresh range of 100,000 transactions (starting 11,196,227,786):
5,978 txns/s, 120.7 MiB/s decoded, 21.2 KB per transaction on average, first response
after 1.9 s.

Older ranges have smaller transactions and stream faster (300,000 transactions each,
zstd):

| Start version | Txns/s |
| --- | ---: |
| 3,000,000,000 | 26,658 |
| 6,000,000,000 | 17,890 |

**Decision: zstd is the default** (`StreamConfig::compression`).

### Mainnet

| Range | Compression | Txns/s | Decoded MiB/s | Avg bytes/txn |
| --- | --- | ---: | ---: | ---: |
| recent 10,000 | none | 250 | 5.7 (link-bound) | 24,113 |
| same 10,000 | zstd | 3,099 | 71.3 | 24,113 |
| recent 100,000 | zstd | 3,653 | 73.9 | 21,202 |
| 300,000 from 1B | zstd | 7,139 | 56.7 | 8,331 |
| 300,000 from 3B | zstd | 7,089 | 50.1 | 7,416 |
| 300,000 from 5B | zstd | 5,896 | 73.3 | 13,034 |
| 300,000 from 6.5B | zstd | 5,853 | 75.2 | 13,473 |

### Filtered (event-only) coverage, mainnet

Filtering on one perp DEX event (`0x50ead…::perp_positions::TradeEvent`), 1,000,000
versions each run:

| Start | Matched | Versions covered/s |
| --- | ---: | ---: |
| 7,202,930,377 | 15,643 | 10,456 |
| 6,000,000,000 | 4,893 | 10,886 |

**A server-side filter saves bandwidth, not scan time.** Filtered coverage is only
1.5–3× the unfiltered rate, because the server still walks every version. With a filter,
`transactions_count` counts versions scanned, not matches.

## History

The stream served every range we asked for on both networks, from genesis (version 0)
up. The REST API is pruned: `oldest_ledger_version` is 11,038,382,517 on testnet and
7,056,130,378 on mainnet. **Backfill must come from the stream.** `nineveh init` can't
rely on REST for anything historical, such as finding a module's publish version.

## Responses

- Every response, filtered or not, carried `processed_range` starting exactly at the
  cursor.
- Our contiguity check passed on all of the ~4 million versions streamed during the
  spike, filtered and unfiltered, on both networks.
- Responses reach **52.6 MB** (about 2,200 transactions) on both networks. That's far over tonic's 4 MiB
  default and justifies the 128 MiB cap. Peak memory is roughly the message size times
  the pipeline's channel depth. M1 should size `batch_size` and channel depth together.

## Mix (100,000 recent transactions)

**Mainnet:** 828,819 `write_resource`, 118,667 `write_table_item`, 15 `delete_resource`,
1 `delete_table_item`; 66 failed transactions. Custom-app traffic is dominated by one
perp DEX (`0x50ead…`: order book, matching engine, collateral, positions). It keeps its
state in plain `Table`s and `BigOrderedMap`s, which is exactly the shape Nineveh
targets and events alone wouldn't cover.

**Testnet:**

- Write-set changes: 983,085 `write_resource`, 138,276 `write_table_item`,
  2 `delete_table_item`. That's about 11 changes per transaction, dominated by
  `ObjectCore` and `FungibleStore`.
- Table items are 12% of all changes, and much of that is `BigOrderedMap` nodes. An
  order-book DEX (`0xe7da…::market_types`) and the framework's `nonce_validation` both
  keep their state in `BigOrderedMap`. See ADR 0003.
- 6 failed transactions. Their write sets are still committed (gas, sequence number).

## Implications

- **One stream covers roughly 3.5–11k versions per second, filtered or not.** All of
  mainnet (≈7.2B versions) is about 14 days unfiltered, or about 8 days filtered, on a
  single stream. Testnet (≈11.2B versions) is about 8–9 days unfiltered.
- **Parallel range backfill is required** for any project that starts far back, whether
  event-only or not: several streams over disjoint version ranges, reassembled in
  version order before the single fold task. This keeps ADR 0005 intact. It moves into
  M1, and Geomi's per-key stream quotas are the open question.
- **`start_version: auto`** (the contract's publish version) is essential, not a nicety.
  It has to come from the stream or an indexer, because REST is pruned.
- ADR 0004 stands: filtering is purely a bandwidth optimization and never trades away
  coverage.
