# 0011. Project config: two-phase loading and explicit row semantics

- Status: Accepted
- Date: 2026-09-15

## Context

`nineveh.yaml` is the product surface for teams that don't write Rust (see
`docs/config.md`). Its errors have to point at the YAML line, and its meaning has to be
settled before the engine exists, because the engine, store and Studio all consume it.
Several questions were open:

- **Lock timing.** `nineveh init` needs the config's struct names *before* a lock
  exists, but the config can only be fully checked *against* the lock.
- **Row keys.** Nothing said where a reduce row's key comes from, or what happens to
  columns a rule doesn't set when it creates a row.
- **Enums.** Real contracts use Move 2 enums for versioned layouts, including events.
  The mainnet perp DEX's `TradeEvent` has variants `V1` and `V2`, and its table-holding
  structs are enums.
- **YAML itself.** `serde_yaml` is deprecated. YAML also has its own traps: duplicate
  keys that silently override, `no` read as `false`, and alias bombs.

## Decision

**Two phases.** `parse` checks everything that doesn't need the lock and returns a
`Config`, whose `roots()` tell `nineveh init` what to pin. `Config::resolve(&Lockfile)`
checks types, fields and implicit keys, and returns a `Project` carrying the decode
`Selection` and each rule's record scope. Both phases collect every problem rather than
stopping at the first, and every diagnostic carries a byte span into the YAML.
Rendering follows rustc's layout.

**YAML via `serde-saphyr`.** It gives every value a location (`Spanned<T>`), enforces
parse budgets against alias bombs, and makes duplicate keys an error by default. We add
strict booleans. Expression fields accept unquoted YAML scalars (`1`, `true`, `uri`)
as expression text, and floats are rejected everywhere.

**Three kinds of state table.**

- `mirror`: the latest value per resource or table item, deleted when the chain deletes
  it.
- `log`: one append-only row per event.
- `reduce`: rows folded by rules.

**Reduce rules.**

- **Key.** A key column takes its value from the rule's `key:` expression. If the rule
  doesn't give one, it takes the record field of the same name, and resolution checks
  that field's existence and type.
- **Completeness.** Any `set` rule may create a row, so each must leave every non-key
  column with a value: set it, give it a `default`, or make it `nullable`. This is
  checked statically, so no row can ever be half-initialized at runtime.
- **Triggers.** `on: <source>` fires on events and writes; `on: <source>.deleted` fires
  on deletes of resources and table items.
- **Column types.** A column's type matches its Move type exactly: no implicit widening.
  `Object<T>` stores as `address`, `Option<T>` as a nullable `T`, `String` as `string`,
  `vector<u8>` as `bytes`, and anything structured as `json` (ADR 0008).

**Record scope.**

- Events expose their struct fields.
- Resource writes expose their fields plus `address`.
- Table items expose `handle`, `key` and `value`.
- Deletes expose only their identity.
- A struct field shadows a built-in name.
- For an enum, only the fields that every variant declares with the same type are in
  scope. A `table:` field on an enum parent resolves if every variant that declares it
  agrees on its type.

Expressions stay source text with spans until `nineveh-expr` typechecks them against
these scopes.

**Names** are lower snake case, at most 63 bytes (Postgres' identifier limit), and
never start with `_`, which is reserved for Nineveh's columns.

## Alternatives considered

- **Single-phase loading that requires the lock.** `init` would have to parse the
  config with a separate code path.
- **Implicit zero defaults for every column.** Silent zeros hide mistakes. An explicit
  `default` costs one word.
- **Hiding enum records' fields.** The target contracts version everything with enums,
  so rules could never read their fields.
- **`saphyr` nodes plus a hand-written mapper.** Better control, but much more code
  than serde with `Spanned` for the same diagnostics.

## Consequences

- `nineveh validate` can report every config problem in one run, located.
- Resolution rejects a `table:` source when another table in the lock has identical key
  and value types, because type matching can't tell their items apart. This closes the
  known case of silent mis-attribution. It doesn't cover tables outside the lock that
  happen to share those types, such as an unrelated contract's `Table<address, u64>`.
  That needs handle attribution, which ADR 0003 describes as a fallback; making it the
  default is the next decision for the engine.
