# Reducers

A reducer says what changes when a record arrives. It is the only thing that writes
your tables, and it is where you'll spend your time. The whole language is six
statements — you can learn it in one sitting.

## 1. The shape of it

Here is a complete reducers file. Read it before the explanation; most of it explains
itself.

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
```

A table is declared once. Handlers say what each kind of record does to it. When a
deposit arrives, the row for that user gains the amount and its deposit count goes up.

The file is called something like `myproject.nineveh.ts`, and your `nineveh.yaml`
names it:

```yaml
name: myproject
network: testnet
reducers: ./myproject.nineveh.ts

sources:
  deposits:    { event: 0xabc::vault::DepositEvent }
  withdrawals: { event: 0xabc::vault::WithdrawEvent }
```

The config holds *what to follow*. The reducers hold *what to do about it*.

No JavaScript runs — not here, not on your data. The file is parsed and compiled. It
looks like TypeScript so your editor can help you, and because the shape is one every
developer already knows.

## 2. Declaring a table

```ts
export const balances = table({
  key:     { user: address },
  columns: { balance: u128.default(0), memo: string.nullable() },
})
```

**Key columns identify a row.** One deposit from `0xabc` and another from `0xdef` are
two rows; two deposits from `0xabc` are one row, folded together. A key can be more
than one column:

```ts
key: { market: u64, day: u64 },
```

**Every other column needs a value whenever a row is created.** A handler might write
a row that doesn't exist yet, and the columns it doesn't mention still need something.
So each one must either be set by every handler that writes the table, have a
`.default(…)`, or be `.nullable()`. If you forget, Nineveh tells you which column and
where.

The types are Move's, plus `json` for anything structured:

`bool` · `u8` `u16` `u32` `u64` `u128` `u256` · `i8` `i16` `i32` `i64` `i128` `i256` ·
`address` · `string` · `bytes` · `json`

Pick a column wide enough for the total, not for one record. Adding up `u64` amounts
overflows a `u64` column eventually; use `u128`. Overflow halts your project rather
than wrapping silently, so this is a decision you make once rather than a bug you find
later.

## 3. Handlers

```ts
on(deposits, (d) => { … })
```

`deposits` is a source name from your config. The parameter — `d` here, call it what
you like — is the record that arrived. You read its fields with a dot.

What fields it has depends on what kind of source it is:

| Source | The record holds |
| --- | --- |
| `event:` | the event's own fields |
| `resource:` | the resource's fields, plus `address` |
| `table:` | `handle`, `key` and `value` |

Resources and table items can be deleted; events can't. Handle a delete with
`.deleted`:

```ts
on(vaults.deleted, (v) => {
  balances.row(v.address).delete()
})
```

Every expression can also read `tx.version` and `tx.timestamp` — the transaction's
number and its block time in microseconds.

## 4. The six statements

That is the entire language.

| Statement | What it does |
| --- | --- |
| `const x = <expr>` | names a value, so you don't repeat it |
| `const r = <table>.row(<key>, …)` | names the row this rule writes |
| `r.<column> = <expr>` · `+=` · `-=` | sets a column of that row |
| `r.delete()` | deletes that row |
| `if (<expr>) { … } else { … }` | applies the writes inside only when it holds |
| `return` | stops the handler; later writes don't apply |

Writing a row creates it if it isn't there. There is no separate "insert" and "update"
— that distinction doesn't exist here, and not having it removes a whole category of
mistake.

`b.balance += x` reads the row's current value and adds to it. That is the ordinary
case: a reducer's job is usually to accumulate.

## 5. Values

Expressions are the same in reducers as in config, written the way JavaScript writes
them: `&& || !`, `== != < <= > >=`, `+ - * / %`, `1_000_000`, `'text'`, and `.field`
for a struct's fields. [Expressions](expressions.md) is the full reference; three
things are worth knowing now.

**Integers are exact and never convert silently.** Adding a `u64` to a `u128` is an
error until you say which you meant:

```ts
b.balance += u128(d.amount)     // balance is u128, amount is u64
```

That is deliberate. Silent widening is how money bugs happen.

**Arithmetic that can't be represented stops the project.** Subtracting below zero in
an unsigned column, dividing by zero, a conversion that doesn't fit — each halts at
that transaction with an error naming the rule, rather than storing a wrong number. Fix
the reducer and replay.

**Nothing is bare.** A record's field is `d.amount`. A row's column is `b.balance`.
There is no naked `amount` that might mean either — every name says where it came from.

## 6. Reading another table

A reducer can read any other table by key:

```ts
on(sold, (s) => {
  const fee_bps = markets.get(s.market)?.fee_bps ?? 0

  const sale = sales.row(s.id)
  sale.amount = s.amount
  sale.fee    = s.amount * fee_bps / 10000
})
```

`markets.get(s.market)` is a row that **may not be there**, which is what `?.` means in
JavaScript and means here too. `?? 0` supplies a value when it isn't. If you leave out
the `??`, the column you're writing has to be `.nullable()` — one or the other.

You can read any `reduce` or `mirror` table, including the one you're writing, where it
means *the row as it was before this rule*. You cannot read a `log` table: a log is
history, not state.

Rules apply in the order you wrote them, across every table. That matters only when one
handler writes a table a later statement reads back — and then it does exactly what
reading top-to-bottom suggests.

## 7. Patterns worth stealing

**Running total per key.** The commonest table there is.

```ts
export const volume = table({
  key:     { market: u64 },
  columns: { total: u128.default(0), trades: u64.default(0) },
})

on(trades, (t) => {
  const v = volume.row(t.market)
  v.total  += u128(t.size)
  v.trades += 1
})
```

**Latest value per key.** No accumulation — each record overwrites.

```ts
on(prices, (p) => {
  const m = markets.row(p.market)
  m.price   = p.price
  m.updated = tx.version
})
```

**One row per day, for a chart.** The day isn't in the record, so compute it from the
transaction's own clock and make it part of the key.

```ts
export const daily = table({
  key:     { market: u64, day: u64 },
  columns: { total: u128.default(0) },
})

on(trades, (t) => {
  daily.row(t.market, tx.timestamp / 86_400_000_000).total += u128(t.size)
})
```

**Something that appears and disappears.** Two handlers, one table.

```ts
on(opened, (o) => {
  const p = positions.row(o.id)
  p.owner = o.owner
  p.size  = o.size
})

on(closed, (c) => {
  positions.row(c.id).delete()
})
```

**Counting only some records.** A condition around the write.

```ts
on(trades, (t) => {
  if (t.size < 100) return
  whales.row(t.taker).trades += 1
})
```

## 8. How it becomes rules

You don't have to know this, but it explains every error message you'll get, so it's
worth two minutes.

Your handlers are compiled into rules. One rule is: *when a record from this source
arrives, and this condition holds, write this row's columns*. The compiler builds them
with one rule:

> Every write is grouped by **the row it targets** and **the condition it sits under**.
> Each group becomes one rule.

So:

- A row named once and written three times is **one** rule setting three columns.
- The same row written in both arms of an `if` is **two** rules with opposite
  conditions.
- An early `return` contributes its negation to everything after it.

Two consequences you will meet:

**A column can only be set once per row per condition.** If you write `b.count` twice
under the same condition, which one wins is a coin flip, so it's an error instead.
Combine them into one expression.

**A row is either written or deleted, not both.** Deciding between them is what `if`
and `else` are for.

## 9. What isn't here

The language is small on purpose. Everything it can express is deterministic and
replayable, which is what makes editing a reducer cheap instead of a re-read of the
chain.

There are no loops, no functions of your own, no `let`, no imports, and no calls beyond
the built-ins. `Date.now()` isn't blocked by a list of forbidden names — it fails
because `Date` isn't anything. The only names in scope are your sources, your tables,
the handler's parameter, the `const`s you wrote, `tx`, and:

`u8(…)`–`u256(…)` · `i8(…)`–`i256(…)` · `min` · `max` · `abs` · `address`

There is no `create` or `update` — writing a row creates it. There is no `increment` —
`+=` already is one.

## 10. When you get it wrong

Every problem is reported at the line it's on, in the file it's in:

```text
error: `balances` has no column `depsits`
  --> myproject.nineveh.ts:10:5
   |
10 |   b.depsits += 1
   |     ^^^^^^^
   = help: did you mean `deposits`?
```

A few you're likely to meet:

**`nothing here is called 'x'`** — a name that isn't a source, a table, the parameter or
a `const`. Usually a typo, and it suggests the closest match.

**`'b' isn't the row this rule writes`** — you read a column of a row you named but
aren't writing. A rule reads its own row directly; for any other, use
`table.get(key)?.column`.

**`'balance' is set twice for the same row`** — see §8. Combine the two writes.

**`this row is both written and deleted`** — put the two outcomes in `if` and `else`.

**A type error against the whole expression** rather than one token. Expressions are
checked once the contract's real types are known, and at that point the location is the
expression, not the character. The message names the column and both types.

## 11. Editor support

`nineveh init` writes a `nineveh.d.ts` beside your config, holding your sources' fields
and your tables' columns. With it, an editor completes `d.` and `b.`, and `tsc` catches
a misspelled field before you save.

It is a convenience, not the checker. It simplifies integer widths to `bigint` and says
nothing about exactness or overflow. `nineveh validate` is what decides whether a
project is correct, and it checks against the real Move types.
