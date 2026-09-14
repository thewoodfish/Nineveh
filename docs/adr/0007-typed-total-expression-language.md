# 0007. Reducers use a typed, total expression language

- Status: Accepted
- Date: 2026-09-14

## Context

Reducers like `set: { balance: "balance + amount" }` need an expression language. It
must meet three requirements:

- **Deterministic:** same inputs, same output, on every machine, forever.
- **Exact over Move's integers:** up to `u256`, with no floating point anywhere.
- **Safe to run untrusted:** a user's config runs on hosted infrastructure.

Non-Rust teams write these expressions, so an error message pointing at the YAML line is
part of the product.

## Decision

Build a small, purpose-built language in a new crate, `nineveh-expr`:

- **Typechecked at `nineveh validate`** against source field types (from `nineveh.lock`)
  and state column types, with span-accurate diagnostics.
- **Exact integer arithmetic** over `u8` through `u256`, implemented with `ruint`. All
  operations are checked. Overflow, underflow and division by zero are deterministic
  errors that halt the project at that version. No floats.
- **Total:** no loops, recursion, I/O, clock or randomness, so every expression
  terminates.
- A hand-written Pratt parser, because the grammar is small and error quality matters
  more than parser-generator convenience.
- The config reserves `reduce: { wasm: … }` for a later escape hatch: `wasmtime` with
  fuel metering, no WASI, and canonicalized NaNs.

## Alternatives considered

- **CEL.** It's non-Turing-complete and proven, but its integers are 64-bit, so it can't
  represent `u128` or `u256` amounts without lossy workarounds.
- **Rhai.** It's Turing-complete, has floats, and needs a larger sandbox surface to make
  deterministic and safe.
- **WASM from day one.** It's powerful but heavy for the common case, and it forces
  every user through a build step.

## Consequences

- We own a language: roughly 1–2k lines plus property tests (against a bigint reference)
  and fuzzing of the parser. That's small, and fully under our control.
- Expressions change behavior only when we change them. The expression-semantics version
  stamped on each state schema (ADR 0005) forces a rebuild when that happens.
