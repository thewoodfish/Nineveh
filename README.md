# Nineveh

The reactive backend for Aptos applications.

Point Nineveh at your contract and describe the state you want. You get a live,
queryable database with real-time subscriptions, kept continuously in sync with the
chain, and no indexing infrastructure to run.

> **Status: alpha, and live at [nineveh.dev](https://nineveh.dev).** A contract address
> in; live tables, a REST API, a change feed and signed webhooks out, driven from a
> browser dashboard. Testnet and devnet for now, and GraphQL is not built. See
> [What Nineveh is not](docs/guide.md#5-what-nineveh-is-not) before you plan around it.

```sh
createdb nineveh
export APTOS_API_KEY_TESTNET=aptoslabs_...    # https://geomi.dev
export NINEVEH_DATABASE_URL=postgres:///nineveh

nineveh up --streams 1                        # the control plane
cd studio && npm install && npm run dev       # the dashboard, on :3000
```

Then point it at `0x1` on testnet and follow one event — you'll have live rows in a
couple of seconds. **[Your first backend](docs/first-backend.md)** walks the whole path,
and **[Running Nineveh yourself](SELF_HOSTED.md)** covers operating it.

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

Written as markdown in this repo, and built into a site by `site/`:

- [`docs/guide.md`](docs/guide.md) — **start here**: what it is and the one idea.
- [`docs/first-backend.md`](docs/first-backend.md) — a working project, end to end.
- [`docs/reducers.md`](docs/reducers.md) — the language your tables are written in.
- [`docs/reading.md`](docs/reading.md) — REST, the change feed, webhooks.
- [`docs/running.md`](docs/running.md) — changes, limits, and what to check.
- [`docs/config.md`](docs/config.md) — every key in `nineveh.yaml`.
- [`docs/expressions.md`](docs/expressions.md) — the expression language.

For running it yourself: [`SELF_HOSTED.md`](SELF_HOSTED.md).
For changing it: [`CLAUDE.md`](CLAUDE.md), [`docs/adr/`](docs/adr/) for why each
decision was made, and [`docs/research/`](docs/research/) for what the Transaction
Stream actually costs.

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
