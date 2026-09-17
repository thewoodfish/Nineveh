# 0019. Let a reducer read other tables by key

- Status: Accepted
- Date: 2026-09-17

## Context

A reducer could read two things: the record that fired it, and the row it was about to
write. That covers counters and running totals, and nothing else. The state a real
backend wants is usually a join:

- a sale's fee needs the rate from the market's config resource;
- a leaderboard row wants the player's name from the profile a different event set;
- a rule wants to skip records from an address some other table says is retired.

Without a way to read across, every one of those has to be denormalized into the
record itself — which means asking the contract to emit it, which is exactly what
Nineveh exists to avoid.

Three facts made this cheap to add:

- the store already keeps the fold's own row (`_row`) for every table except `log`
  ones, so a reducer can read a `mirror` table's value back exactly as the fold
  wrote it (ADR 0013, ADR 0014);
- `TableSchema` already says, for every table, what its columns are and where each
  one sits in that row (ADR 0014), so one mechanism covers `reduce` and `mirror`
  tables alike;
- the fold already tells its caller which keys it needs and is folded again once they
  are loaded (`FoldError::NotLoaded`, ADR 0013), and that path doesn't care which
  table a key belongs to.

## Decision

**Syntax.** An expression may read another table's row by key:

```yaml
set:
  fee: "amount * unwrap_or(markets[market].fee_bps, 0) / 10000"
```

`table[key, ...]` names a row; only `.column` of one is a value. Key expressions are
typechecked against that table's key columns, in key order, and there must be exactly
as many as the table has.

**A lookup is always an option.** `t[k].c` has type `Option<C>`, and is null when the
row isn't there or the column has no value in it. That is SQL's outer join, and it
keeps the language total: no rule fails because a row hasn't arrived yet. `unwrap_or`
turns it into a value; `is_some(t[k].<key column>)` asks whether the row exists, since
a key column is never null in a row that does.

**What can be read.** Any `reduce` or `mirror` table in the project, itself included.
A `log` table can't: logs are append-only history and the store doesn't keep the
fold's row for them. Naming one is an error that says so.

**What a lookup sees.** Committed state, plus every change the fold has already made
in this batch. Concretely, a rule sees the effect of every record before it, and of
every table and rule ordered before it within its own record (ADR 0013's order).
Order is fixed by the config, so this is deterministic: the same inputs fold to the
same state whatever the batch boundaries, which is what `tests/replay.rs` checks.

**No recursion guard.** A rule may read the table it writes. It reads a row as of
before its own write; there's no cycle to detect because a lookup never triggers a
rule.

**Loading.** A lookup goes through the same `StateView` as the row being written, so
a key the view doesn't hold is reported as `NotLoaded` and the pipeline loads it and
folds the batch again. Nothing new was needed in the store or the pipeline.

## Alternatives considered

- **Per-entity tables auto-created for every address field of every event.** Rejected
  with the model: it adds tables nobody asked for and doesn't answer "what does that
  other table say?", which is the actual question.
- **An absent row reads as the column's default**, so lookups have a plain type.
  Tempting, and it matches how a new row starts — but a `mirror` table's columns have
  no defaults, so the feature would have been unusable on exactly the tables that
  carry contract state. It also hides the difference between "no row" and "zero".
- **`Option<Option<T>>` for a nullable column**, keeping "no row" and "null value"
  apart. Honest, but it forces a double `unwrap_or` on the common path for a
  distinction almost no rule cares about. Flattened, like SQL.
- **A join declared in the config** (`join: markets on market`) rather than an
  expression. More machinery, less reach: an expression can key a lookup on anything,
  including another lookup.
- **Reading log tables too**, by keeping their rows. That would double what a log
  costs to store for a feature whose point is state, not history.

## Consequences

- Rules can express joins, so state tables can hold what an app actually queries
  rather than what the contract happened to emit.
- A batch may now load keys from several tables before it folds. The retry loop
  handles that already, but a rule that looks up a different key for every record
  turns one round trip per batch into more; `MAX_LOAD_ROUNDS` still bounds it.
- The fold's `missing` set is behind a `RefCell` because expressions read state while
  a rule is being applied. It stays within one `fold` call, which is single-threaded.
- `SEMANTICS_VERSION` is unchanged: a config written before this ADR folds exactly as
  it did, so no project rebuilds itself on upgrade.
- Studio's config editor and `control.check` validate lookups with no change, since
  both compile the config through `nineveh-config`.
