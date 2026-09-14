# 0001. Own the Transaction Stream client; vendor the protos

- Status: Accepted
- Date: 2026-09-14

## Context

Nineveh ingests from the Aptos Transaction Stream: the gRPC service
`aptos.indexer.v1.RawData/GetTransactions`, hosted at `grpc.{network}.aptoslabs.com:443`.
The service requires a Geomi API key sent as `authorization: Bearer <key>`. Anonymous
requests are rejected with `Unauthenticated`, which we observed during the M0 spike.

There are two ready-made ways to get the protocol types:

- **The `aptos-protos` crate.** Every version on crates.io has been yanked. The last
  publish was January 2024.
- **The Aptos Indexer processor SDK.** It depends on the protos through a git
  dependency on aptos-core. crates.io refuses to publish a crate with git dependencies,
  so Nineveh couldn't be published if we built on it. The SDK also brings its own step
  framework and diesel, while we use sqlx.

The stream client is also where ordering, contiguity, backpressure and reconnect
semantics live. Those are the properties Nineveh's correctness rests on, so we want to
own them.

## Decision

- Vendor the `.proto` files we need into `crates/nineveh-ingest/proto/`, pinned to a
  specific aptos-core commit recorded in `proto/UPSTREAM`. Update them only through
  `scripts/sync-protos.sh <sha>`.
- Generate Rust bindings with `cargo xtask codegen`, using `protox` (a pure-Rust
  protobuf compiler, so no system `protoc`) and `tonic-prost-build`. The generated code
  is checked in. CI runs `cargo xtask codegen --check` and fails on drift.
- Write our own client in `nineveh-ingest`. The Aptos SDK is prior art to read, not a
  dependency.

## Alternatives considered

- **Git dependency on aptos-core's protos.** This blocks crates.io publishing and pulls
  a very large repository into every build.
- **Generate in `build.rs`.** Every downstream build would compile a protobuf compiler,
  and proto changes wouldn't show up as a diff in review.
- **Build on the processor SDK.** See Context.

## Consequences

- A proto bump is an explicit, reviewable PR: new `UPSTREAM` SHA, new protos,
  regenerated bindings.
- The Transaction Stream is beta. We watch aptos-core's `protos/` directory and bump
  deliberately. Aptos-specific types never leak past `nineveh-ingest` and
  `nineveh-decode`.
- Contributors need no protobuf toolchain.
