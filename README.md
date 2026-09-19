# Nineveh

The reactive backend for Aptos applications.

Point Nineveh at your contract and describe the state you want. You get a live,
queryable database with real-time subscriptions, kept continuously in sync with the
chain, and no indexing infrastructure to run.

> **Status: alpha.** It works end to end — a contract address in, live tables, a REST
> API, a change feed and signed webhooks out, driven from a browser dashboard. It has
> not been deployed anywhere yet, and GraphQL is not built. See
> [What isn't built](docs/guide.md#12-what-isnt-built) before you plan around it.

```sh
createdb nineveh
export APTOS_API_KEY_TESTNET=aptoslabs_...    # https://geomi.dev
export NINEVEH_DATABASE_URL=postgres:///nineveh

nineveh up --streams 1                        # the control plane
cd studio && npm install && npm run dev       # the dashboard, on :3000
```

Then point it at `0x1` on testnet and follow one event — you'll have live rows in a
couple of seconds. **[The guide](docs/guide.md)** walks the whole path.

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

## Documentation

- **[The guide](docs/guide.md)** — getting started, reading your data, changing a
  project, limits and retention, operating it, and a walkthrough to test against.
- [`docs/config.md`](docs/config.md) — every key in `nineveh.yaml`.
- [`docs/expressions.md`](docs/expressions.md) — the reducer expression language.
- [`docs/adr/`](docs/adr/) — architecture decisions and why they were made.
- [`docs/research/`](docs/research/) — what the Transaction Stream actually costs and
  how fast it goes.

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
