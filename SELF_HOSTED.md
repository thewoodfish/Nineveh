# Running Nineveh yourself

The [documentation](docs/guide.md) describes using Nineveh. This page is about running
it: on your own Postgres, against your own Aptos stream key.

There is a hosted service at [nineveh.dev](https://nineveh.dev) if you'd rather not.
Everything in the docs applies either way; this page covers what's yours to operate
when you choose this one.

## What you're taking on

Nineveh self-hosted is two long-running things and a database:

- **Postgres** — holds every project's tables, records and change feed.
- **A Nineveh process** — either `nineveh run` for one project, or `nineveh up` for
  many.
- **Studio** (optional) — a Next.js app that talks to `nineveh up`.

And one thing you have to get yourself: **a Geomi API key** for Aptos' Transaction
Stream, from [geomi.dev](https://geomi.dev). Keys are per network. Anonymous requests
are rejected, so nothing works without one.

## Prerequisites

| | |
| --- | --- |
| PostgreSQL | 14 or newer |
| Rust | the version in `rust-toolchain.toml`; rustup installs it on first build |
| Node | 24+, only if you want Studio |
| A Geomi key | per network, from [geomi.dev](https://geomi.dev) |

## Build it

```sh
git clone https://github.com/thewoodfish/Nineveh.git
cd Nineveh
cargo build --release -p nineveh-cli
```

The binary is `target/release/nineveh`. Put it on your `PATH` or call it by path.

## One project, from the CLI

This is the path to use if you want your project's config in your own repository,
reviewed like code and deployed from CI. There is no Studio and no control plane —
one project, one schema, one process.

**Write `nineveh.yaml`.** `nineveh init` reads this file; it does not create one.

```yaml
name: blocks
network: testnet
start_version: auto

sources:
  new_block: { event: "0x1::block::NewBlockEvent" }

state:
  new_block: { log: new_block }
```

**Pin the layouts.** `init` fetches the Move ABIs your sources need, writes
`nineveh.lock`, and resolves `start_version: auto` to a real number. Commit both files.

```sh
export APTOS_API_KEY=your_geomi_key
nineveh init
```

**Check it without touching the network.** `validate` is offline and reports every
problem at the line it's on — worth running in CI on every change.

```sh
nineveh validate
```

**Run it.**

```sh
createdb nineveh
export NINEVEH_DATABASE_URL=postgres:///nineveh

nineveh run --serve
```

That builds the project's tables and keeps them current, and serves the API on
`127.0.0.1:4000`. Ctrl-C stops after the current commit; running it again resumes from
where it stopped.

Useful variations:

| | |
| --- | --- |
| `nineveh run` | pipeline only, no API |
| `nineveh serve` | API only, no pipeline — a read-only replica of the same database |
| `nineveh run --until <version>` | stop once that version is committed; good for tests |
| `nineveh run --schema <name>` | build into a schema other than the project's name |
| `nineveh run --streams 1` | one backfill stream instead of four |
| `nineveh replay --yes` | rebuild from the start beside the live tables, then swap |

### Reducers

If your project has reduce tables, they go in a `.nineveh.ts` file your config names:

```yaml
reducers: ./blocks.nineveh.ts
```

`nineveh init` also writes `nineveh.d.ts` beside your config, so an editor can complete
your sources' fields and your tables' columns. Both belong in your repository.
The language is documented in [`docs/reducers.md`](docs/reducers.md).

## Many projects, with Studio

This is what the hosted service runs. Projects live in the database rather than in
files, and Studio creates and edits them.

```sh
createdb nineveh_control
export NINEVEH_DATABASE_URL=postgres:///nineveh_control
export APTOS_API_KEY_TESTNET=your_testnet_key

nineveh up
```

The control plane listens on `127.0.0.1:4000` and serves each project at
`/projects/{name}/v1`. Then, in another terminal:

```sh
cd studio
npm install
npm run build && npm start      # http://localhost:3000
```

`npm run dev` works too, but the production build is much lighter on a laptop.

### Keys are per network

Geomi issues a key for one network, so there is no such thing as one that works
everywhere. `--api-key` (or `APTOS_API_KEY`) is the fallback, and
`--api-key-testnet`, `--api-key-mainnet` and `--api-key-devnet` name a network each.

**Naming one network means naming them all.** While `APTOS_API_KEY` is the only key
set, it stands for every network, which is what a plane on a single network wants. The
moment you set a per-network key you have said which networks you mean, and Studio
offers only those: it greys out the rest and says there is no key for them, rather than
letting someone create a project that fails at its first stream with
`Unauthenticated`.

So a plane meant to run testnet and devnet sets both:

```sh
export APTOS_API_KEY_TESTNET=your_testnet_key
export APTOS_API_KEY_DEVNET=your_devnet_key
```

`nineveh up` says at boot which networks it can stream, and warns about any the tier
allows that it has no key for.

### Local mode and hosted mode

By default `nineveh up` runs in **local mode**: no sign-in, and it refuses to listen
anywhere but loopback. That's the right thing on your own machine.

Give it a GitHub OAuth app and it switches to **hosted mode** — people sign in, projects
belong to accounts, and project APIs need keys:

```sh
nineveh up \
  --listen 0.0.0.0:4000 \
  --github-client-id …    \
  --github-client-secret … \
  --public-url https://api.example.com \
  --studio-url https://studio.example.com
```

All four also read from the environment: `NINEVEH_GITHUB_CLIENT_ID`,
`NINEVEH_GITHUB_CLIENT_SECRET`, `NINEVEH_PUBLIC_URL`, `NINEVEH_STUDIO_URL`. The OAuth
app's callback URL must be `<public-url>/auth/github/callback`.

Put a TLS terminator in front of it. Nineveh speaks plain HTTP.

## Operating it

**Run one control plane per database.** Two planes writing the same project conflict at
commit time, and the error is deliberately not retryable — it stops rather than spins:

```
the cursor for X moved …: another writer is committing to this project
```

It's detected at commit, not at startup, so a rolling deploy doesn't work. **Stop the
old process, then start the new one.** Two planes also each open their own stream per
network, which eats into the concurrent-stream cap on your key.

**Back up Postgres.** It holds everything: your tables, your records and your projects'
configs. A `nineveh.lock` and a `nineveh.yaml` can be regenerated; the record log is
what makes a rebuild cheap instead of a re-read of the chain, and it exists nowhere
else.

**Upgrading** is: stop the process, build the new binary, start it. Migrations run at
startup. A build whose fingerprint changed rebuilds into a fresh schema and swaps when
it's caught up, so the old tables keep serving throughout.

**Streams are capped per account.** Aptos allows a limited number of concurrent
Transaction Stream connections per organisation. `nineveh up` shares one stream per
network across every project and uses a few more for backfills; `nineveh run` takes
`--streams` of its own. If you see `ResourceExhausted` or `429`, something is holding
more than your key allows.

## Putting it on a server

[`deploy/`](deploy/) has what you need: a systemd unit, a Caddyfile, a backup timer,
and a README that walks the whole thing. The short version is one VPS running Postgres
and one `nineveh up`, with the two frontends hosted anywhere static.

**Sign-in is not optional once it's reachable.** Without a GitHub OAuth app the plane
runs in local mode, where every caller owns every project. It refuses to *listen* on
anything but loopback then — but a reverse proxy in front of loopback satisfies that
check while exposing it to the internet. `deploy/README.md` says this twice; this is
the third.

## What isn't built

Stated plainly, because you'll go looking:

- **No Dockerfile** and no published image.
- **No metrics endpoint.** Logs are structured (`tracing`); there's no Prometheus
  surface.
- **No restore tooling.** `deploy/backup.sh` dumps; restoring is `pg_restore` by hand.

## When something's wrong

`GET /health` on a control plane answers without auth: 200 with
`{"status":"ok","database":true}`, or 503 when Postgres is unreachable.

| What you see | What it is |
| --- | --- |
| A project runs cleanly and stays empty | The source matched nothing. Check the exact type name and the network — `NewBlock` and `NewBlockEvent` are different types. |
| `ResourceExhausted` / `429` | The concurrent-stream cap on your Geomi key. Something else is holding streams. |
| New control endpoints return 404 | A stale `nineveh up` is still holding the port. `lsof -nP -iTCP:4000 -sTCP:LISTEN` |
| Studio says *Lost the API* | The plane isn't answering. Check its logs. |
| `Stream-duration-limit-reached-please-reconnect` | Normal. Aptos closes stream connections at a maximum duration and expects a new one. |
| `another writer is committing to this project` | Two control planes on one database. See above. |

## Tests

```sh
# everything CI runs, in one pass
scripts/check.sh

# the Postgres-backed tests, which skip without a database
createdb nineveh_test
NINEVEH_TEST_DATABASE_URL=postgres:///nineveh_test scripts/check.sh

# live tests against testnet, ignored by default
APTOS_API_KEY=… NINEVEH_TEST_DATABASE_URL=postgres:///nineveh_test \
  cargo test -p nineveh-ingest -p nineveh-pipeline -p nineveh-cli -- --ignored
```

## Where everything else is documented

The config format, the reducer language, the REST API, the change feed and webhooks are
all in [`docs/`](docs/guide.md) — they're the same whoever is running it.

If you're changing Nineveh rather than running it, read `CLAUDE.md` for the
architecture and `docs/adr/` for why each decision was made.
