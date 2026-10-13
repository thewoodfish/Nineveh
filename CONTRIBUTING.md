# Contributing to Nineveh

Nineveh's value is correctness: state tables that are an exact, replayable projection of
the chain. Contributions are held to that bar, and most of it is enforced mechanically so
review can focus on design.

Read this before writing code. Not the CI table — the part above it.

## How this project is run

Nineveh is **founder-led**, deliberately. One person holds the direction, the scope and
the merge button, and that isn't a stage the project is waiting to grow out of.

Two reasons, both practical. The first is correctness: the invariants here are
load-bearing and mostly invisible in the types. A reducer is the only writer of state; the
fold is pure; decoding never guesses. Each of those is one careless merge away from being
untrue, and a project that is wrong about the chain has no reason to exist. The second is
coherence: this is a product, not a pile of capabilities. Most of the work is deciding
what Nineveh *won't* do, and design-by-pull-request is very good at eroding exactly that.

What it means in practice:

- Direction and scope are mine. So is the final call on any disagreement — stated with
  reasoning, not by silence.
- Every change is reviewed and merged by me. Being technically sound isn't sufficient; a
  change also has to be something the project wants.
- **Ask before you build anything substantial.** An unsolicited large PR is the most
  likely thing to be declined, and that costs you a weekend, not me an afternoon. Open an
  issue describing the change and wait for a yes.
- Small and obvious — a bug with a failing test, a wrong doc, a missing fixture — just
  send it. No ceremony.

None of that is distrust, and it isn't a closed door. It's that the answer to "should this
exist?" is held in one place while "does this work?" is held by everyone.

## Owning a surface

There's a bigger version of contributing than a PR: owning a whole surface — an SDK, a
client library, a framework integration. That's genuinely available and I'd like more of
it.

The shape is the same every time: **the design and the code are yours, the repo and the
release are mine.** You're named as its author and maintainer, you hold the design
authority over how it feels to use, and where your judgement and my taste disagree on
ergonomics, yours wins — that's the whole reason to hand a surface over. Changes land as a
PR I review and merge; published releases I cut.

If you want that, say what surface and why you're the person for it.

## What's wanted

In rough order of how much it helps:

- **Fixtures from real transactions.** Decoding is the riskiest layer in the system and
  every rendering convention needs a real payload behind it. A fixture that breaks the
  decoder is the single most valuable thing you can send.
- **Bug fixes with a test that fails without them.**
- **Docs that are wrong, stale or confusing.** Especially anywhere the docs claim
  behaviour the code doesn't have.
- **Examples** — contracts, apps, demos that show Nineveh doing something real.
- **Studio and the site.** The frontend has the most room and the fewest hard invariants.
- **Performance, with numbers.** A benchmark before and after, not an assertion.

## What will be declined

Not because it's bad work — because the answer is already decided. The reasoning for each
is in [`CLAUDE.md`](CLAUDE.md) and the ADRs; these are summaries, and if you think one is
wrong, that's an ADR to propose, not a PR to send.

- **Reorg or rewind machinery.** Aptos has deterministic finality. Committed transactions
  don't reorg, and the forward-only model is correct here.
- **Live aggregate queries** — subscribing to `SELECT top 10 by volume` and keeping it
  correct as events stream. That's incremental view maintenance; it's a later bet on top
  of a streaming engine, not a v1 feature.
- **Writable app tables, auth, row-level security.** Chain-derived state is a read-only
  projection. The full Supabase-shaped backend is a separate, much larger bet.
- **Anything that lets something other than a reducer write a state table.** This is the
  invariant the rest of the system is built on.
- **Clock, randomness, network or filesystem inside a reducer or the fold.** Replay stops
  being deterministic and the project's core claim stops being true.
- **The API reading the chain.** It reads state tables. That's the whole architecture.
- **Scope creep** — wallets, gas stations, generic analytics, or reimplementing what
  Aptos' hosted Indexer already serves (fungible assets, NFTs, tokens, ANS). Nineveh is
  for custom app state.

## Before you write code

- Read [`CLAUDE.md`](CLAUDE.md) (the operating manual) and the ADRs in
  [`docs/adr/`](docs/adr/) that touch your change. They explain the invariants the types
  alone don't show.
- If your change is hard to reverse, crosses crate boundaries, or changes an invariant,
  propose an ADR first (see [`docs/adr/README.md`](docs/adr/README.md)). An ADR is how you
  argue with a decision — including any of the ones listed above. Don't edit an existing
  ADR to change its mind; supersede it with a new one.

## Rules the tooling can't fully check

- **Library code never aborts.** Return `Result` with a `thiserror` enum, and say whether
  each error is retryable. `anyhow` belongs only in binaries, examples and `xtask`.
- **Reducers and the fold are pure.** No clock, randomness, network or filesystem.
- **Decoding never guesses.** A value that doesn't match its layout is a located, fatal
  error. Every new rendering convention needs a fixture from a real transaction.
- **Integer conversions are explicit.** Use `TryFrom` and handle the failure. Versions and
  amounts are `u64` to `u256`.
- **Tests come with behavior.** A bug fix includes a test that fails without it.

## What CI checks

`scripts/check.sh` runs everything CI does, in one pass, and names what failed. Run it
before you open a PR; `--fast` is the inner loop while you work.

| Check | Run locally with |
| --- | --- |
| Formatting | `cargo fmt --all --check` |
| Lints | `cargo clippy --workspace --all-targets` (CI adds `-D warnings`) |
| Tests | `cargo test --workspace` |
| Postgres-backed tests | `NINEVEH_TEST_DATABASE_URL=postgres:///nineveh_test cargo test` |
| Docs | `RUSTDOCFLAGS="-D warnings" cargo doc --workspace --no-deps` |
| MSRV (1.89) | `cargo +1.89 check --all-targets` |
| Generated bindings current | `cargo xtask codegen --check` |
| Dependency direction | `scripts/check-deps.sh` |
| Supply chain | `cargo deny check` |
| Studio and site | `npm run typecheck && npm run build` in each |

The Postgres tests **skip silently** without that environment variable, so a green
`cargo test` does not mean you ran them. CI has a database and will.

## Updating the Aptos protos

```sh
scripts/sync-protos.sh <aptos-core-commit-sha>
cargo xtask codegen
```

Commit the protos, `proto/UPSTREAM` and the regenerated bindings together, and note what
changed upstream in the PR description.

## Commits

One logical change per PR. Commit subjects are `scope: what changed, in lowercase`, where
the scope is the crate or area — `ingest:`, `decode:`, `studio:`, `docs:`, `site:`. Say
what the change does or why it matters, not which files moved:

```
decode: an enum variant with no fields still has a tag
store:   the outbox is never pruned past a webhook that hasn't delivered
```

## Licensing

Nineveh is Apache-2.0 and contributions are accepted under it — inbound matches outbound.
There's no CLA to sign. By opening a PR you're licensing that work under Apache-2.0,
patent grant included.

## Reporting security issues

Don't open a public issue. Use GitHub's private vulnerability reporting on this
repository.
