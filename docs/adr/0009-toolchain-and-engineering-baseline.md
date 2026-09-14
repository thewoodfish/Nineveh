# 0009. Toolchain and engineering baseline

- Status: Accepted
- Date: 2026-09-14

## Context

Nineveh is open-source infrastructure whose value is correctness. The bar has to be
enforced by tooling from the first commit, because retrofitting lints and policies onto
a grown codebase never fully lands.

## Decision

- **Edition 2024**, resolver 3. The toolchain is pinned in `rust-toolchain.toml`
  (currently 1.98.1) and bumped deliberately in its own PR.
- **MSRV 1.89** (`rust-version`), checked in CI. Resolver 3 selects dependency versions
  compatible with the MSRV.
- **Workspace lints:**
  - `unsafe_code = "forbid"`;
  - `unwrap_used`, `expect_used` and `panic` denied, with tests exempted via
    `clippy.toml`;
  - lossy integer casts denied;
  - `clippy::pedantic` as warnings, with CI running `-D warnings`.
- **Errors:** `thiserror` in libraries, with retryable and fatal errors distinguished.
  `anyhow` only in binaries, examples and xtask.
- **Supply chain:**
  - `cargo-deny` checks advisories, yanked crates, a permissive-only license allowlist
    and registry sources;
  - `Cargo.lock` is committed;
  - GitHub Actions are pinned to full commit SHAs.
- **CI gates:** fmt, clippy, tests (nextest and doctests), rustdoc with `-D warnings`,
  MSRV, the generated-bindings drift check, the dependency-direction check (ADR 0005)
  and cargo-deny.
- **Decisions** are recorded as ADRs in `docs/adr/`.

## Consequences

- New contributors get immediate, mechanical feedback instead of review nits.
- Pedantic lints occasionally need a local `#[allow]` with a reason. That's the intended
  friction.
