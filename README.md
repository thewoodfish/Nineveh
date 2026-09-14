# Nineveh

The reactive backend for Aptos applications.

Point Nineveh at your contract and describe the state you want. You get a live,
queryable database (REST + GraphQL) with real-time subscriptions, kept continuously in
sync with the chain, and no indexing infrastructure to run.

> **Status: pre-alpha (milestone M0).** The workspace, CI and Transaction Stream client
> are in place. Nothing is usable end to end yet. See [`docs/adr/`](docs/adr/) for the
> architecture decisions made so far.

## How it works

Nineveh is event sourcing with materialized read models:

- **The chain is the source of truth.** Nineveh reads its events, resource changes and
  table items from the Aptos Transaction Stream.
- **Reducers fold them into state tables.** Reducers are deterministic, replayable and
  the only writer of state.
- **Your app queries state and subscribes to changes.** It never reads the raw log.

State, change notifications and the processing cursor commit in one Postgres
transaction. A crash anywhere resumes exactly where it left off, and a replay rebuilds
exactly the same state.

## Development

You need a Rust toolchain; `rust-toolchain.toml` pins the version, and `rustup`
installs it on first use.

```sh
cargo test --workspace                  # unit tests
cargo clippy --workspace --all-targets  # lints (CI runs with -D warnings)
cargo xtask codegen --check             # generated gRPC bindings are current
scripts/check-deps.sh                   # crate dependency direction
```

Talking to the live Transaction Stream needs a [Geomi](https://geomi.dev) API key:

```sh
export APTOS_API_KEY=aptoslabs_...
cargo run --release -p nineveh-ingest --example stream_probe -- \
    --network testnet --start <version> --count 10000
```

See [CONTRIBUTING.md](CONTRIBUTING.md) for how changes are made.

## License

Apache-2.0. See [LICENSE](LICENSE).
