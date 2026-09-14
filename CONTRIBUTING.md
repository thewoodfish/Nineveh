# Contributing to Nineveh

Nineveh's value is correctness: state tables that are an exact, replayable projection of
the chain. Contributions are held to that bar, and most of it is enforced mechanically
so review can focus on design.

## Before you write code

- Read [`CLAUDE.md`](CLAUDE.md) (the operating manual) and the ADRs in
  [`docs/adr/`](docs/adr/) that touch your change. They explain the invariants the types
  alone don't show.
- If your change is hard to reverse, crosses crate boundaries, or changes an invariant,
  propose an ADR first (see [`docs/adr/README.md`](docs/adr/README.md)).

## What CI checks

Every PR must pass these checks:

| Check | Run locally with |
| --- | --- |
| Formatting | `cargo fmt --all --check` |
| Lints | `cargo clippy --workspace --all-targets` (CI adds `-D warnings`) |
| Tests | `cargo test --workspace` |
| Docs | `RUSTDOCFLAGS="-D warnings" cargo doc --workspace --no-deps` |
| MSRV (1.89) | `cargo +1.89 check --all-targets` |
| Generated bindings current | `cargo xtask codegen --check` |
| Dependency direction | `scripts/check-deps.sh` |
| Supply chain | `cargo deny check` |

## Rules the tooling can't fully check

- **Library code never aborts.** Return `Result` with a `thiserror` enum, and say
  whether each error is retryable. `anyhow` belongs only in binaries, examples and
  `xtask`.
- **Reducers and the fold are pure.** No clock, randomness, network or filesystem.
- **Decoding never guesses.** A value that doesn't match its layout is a located, fatal
  error. Every new rendering convention needs a fixture from a real transaction.
- **Integer conversions are explicit.** Use `TryFrom` and handle the failure. Versions
  and amounts are `u64` to `u256`.
- **Tests come with behavior.** A bug fix includes a test that fails without it.

## Updating the Aptos protos

```sh
scripts/sync-protos.sh <aptos-core-commit-sha>
cargo xtask codegen
```

Commit the protos, `proto/UPSTREAM` and the regenerated bindings together, and note
what changed upstream in the PR description.

## Commits

Use [Conventional Commits](https://www.conventionalcommits.org/) (`feat(ingest): …`,
`fix(decode): …`, `docs(adr): …`). Keep each PR to one logical change.

## Reporting security issues

Don't open a public issue. Use GitHub's private vulnerability reporting on this
repository.
