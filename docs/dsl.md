# Reducers in `.nineveh.ts`

A reduce table can be written two ways. In `nineveh.yaml` you describe each table and
the rules that change it. In a `.nineveh.ts` file you describe each **event** and
everything that changes when it arrives:

```ts
export const balances = table({
  key:     { user: address },
  columns: { balance: u128.default(0), deposits: u64.default(0) },
})

on(deposits, (d) => {
  const b = balances.row(d.user)
  b.balance  += u128(d.amount)
  b.deposits += 1
})

on(withdrawals, (w) => {
  if (w.amount == 0) return
  balances.row(w.user).balance -= u128(w.amount)
})

on(vaults.deleted, (v) => {
  balances.row(v.address).delete()
})
```

They build the same thing. Point `nineveh.yaml` at the file:

```yaml
name: vault
network: testnet
reducers: ./vault.nineveh.ts

sources:
  deposits:    { event: 0xabc::vault::DepositEvent }
  withdrawals: { event: 0xabc::vault::WithdrawEvent }
  vaults:      { resource: 0xabc::vault::Vault }

state:
  vaults: { mirror: vaults }
```

`nineveh.yaml` keeps what is configuration — the network, the sources, `mirror` and
`log` tables, the API and webhooks. The DSL file keeps what is behaviour.

No JavaScript runs, here or on the chain's data. The file is parsed and compiled to the
same rules the YAML produces, and the same deterministic fold applies them.

An `import { on, table } from "nineveh"` line at the top is accepted and ignored — it's
there if your editor wants it, and nothing is loaded.

## The whole language

Six statements. That is all of it.

| Statement | What it does |
| --- | --- |
| `const x = <expr>` | names a value |
| `const r = <table>.row(<key>, …)` | names the row this rule writes |
| `r.<column> = <expr>`, `+=`, `-=` | sets a column of that row |
| `r.delete()` | deletes that row |
| `if (<expr>) { … } else { … }` | applies the writes inside only when it holds |
| `return` | stops the handler; later writes don't apply |

`on(<source>, (<name>) => { … })` handles every record from a source, and
`on(<source>.deleted, …)` handles its deletes — resources and table items delete;
events don't. The parameter is yours to name: it is the record, and you read its fields
with `.`.

## Declaring a table

```ts
export const balances = table({
  key:     { user: address },
  columns: { balance: u128.default(0), memo: string.nullable() },
})
```

Key columns identify a row and always have a value. Every other column needs one from
each rule that writes the table: set it, give it `.default(…)`, or make it
`.nullable()`. The types are the same as [`nineveh.yaml`'s](config.md#column-types):
`bool`, `u8`–`u256`, `i8`–`i256`, `address`, `string`, `bytes`, `json`.

## Expressions

The [expression language](expressions.md) is the same one the YAML uses, written the
way JavaScript writes it: `&& || !`, `== != < <= > >=`, `+ - * / %`, `1_000_000`,
`'text'`, and `.field` for a struct's fields. Integers are exact and never convert
silently, so `balance + u128(amount)` is the way to add a `u64` to a `u128`.

Three things are spelled differently from the YAML reference:

| `nineveh.yaml` | Here |
| --- | --- |
| `if c then a else b` | `c ? a : b` |
| `unwrap_or(markets[m].fee_bps, 0)` | `markets.get(m)?.fee_bps ?? 0` |
| `42u128`, `@0x1` | `u128(42)`, `address("0x1")` |

Reading another table is always optional, because the row may not be there — which is
exactly what `?.` and `??` mean:

```ts
on(sold, (s) => {
  const fee_bps = markets.get(s.market)?.fee_bps ?? 0
  const sale = sales.row(s.id)
  sale.amount = s.amount
  sale.fee    = s.amount * fee_bps / 10000
})
```

A rule reads its own row directly — `b.balance` is the value before this rule's write,
and `b.balance += x` is that value plus `x`. To read a *different* row, use
`.get(…)`. You can read any `reduce` or `mirror` table, including the one you're
writing. You can't read a `log`: logs are history, not state.

`tx.version` and `tx.timestamp` (microseconds) are available in any expression.

## How it becomes rules

Worth knowing, because it explains what the compiler will and won't accept:

> Every write is grouped by **the row it targets** and **the condition it sits under**.
> Each group becomes one rule.

So a row named once and assigned three times is one rule setting three columns. The same
row assigned in both arms of an `if` is two rules with opposite conditions. An early
`return` contributes its negation to everything after it.

Rules apply in the order you wrote them, across every table. That matters when one
handler writes a table that a later statement reads back.

## What isn't here

No `let`, no loops, no functions of your own, no imports of other packages, no calls
except the built-ins. Not as a restriction bolted on: a reducer has to be deterministic
and replayable, and there is nothing in this language that isn't.

`Date.now()` isn't rejected by a list of banned names — it fails because `Date` isn't
anything. The only names in scope are your sources, your tables, the handler's
parameter, the `const`s you wrote, `tx`, and the built-ins: `u8(…)`–`u256(…)`,
`i8(…)`–`i256(…)`, `min`, `max`, `abs`, `address`.

Three operations the YAML has and the DSL doesn't need: there is no `create` or
`update`, because writing a row creates it if it isn't there; and no `increment`,
because `+=` already is one.

## Editor support

`nineveh init` writes `nineveh.d.ts` beside your config, with your sources' fields and
your tables' columns. With it, an editor completes `d.` and `b.`, and `tsc` catches a
misspelled field before you save.

It's a convenience, not the checker. It simplifies integer widths to `bigint`, and it
says nothing about exactness or overflow. `nineveh validate` is what decides whether a
project is correct, and it checks against the layouts pinned in `nineveh.lock`.

## When something is wrong

Every problem is reported at the line it's on, in the file it's in:

```text
error: `balances` has no column `depsits`
  --> vault.nineveh.ts:10:5
   |
10 |   b.depsits += 1
   |     ^^^^^^^
   = help: did you mean `deposits`?
```

Expression type errors are reported against the whole expression rather than the exact
token inside it. Everything else points at the token.
