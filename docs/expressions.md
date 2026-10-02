# Expressions

The small language that computes a column's value, on the right of every `=` in a
reducer. [Reducers](reducers.md) is the page that teaches writing them; this one is the
reference for what the language has in it.

```ts
on(deposits, (d) => {
  if (d.amount > 0) {
    const b = balances.row(d.user)
    b.balance  += u128(d.amount)
    b.deposits += 1
  }
})
```

Expressions are **typed**: every one is checked against your record's fields and your
table's columns, and a mismatch is reported at the character it's on — as you type, in
Studio's editor, and by `nineveh validate` if you run the CLI yourself. They're
**exact**: integers are Move's, from `u8` to `u256` and `i8` to `i256`, with no floating
point and no silent wraparound. And they're **total**: no loops, I/O, clock or
randomness, so replaying the chain always rebuilds the same state.

## Names

**Nothing is bare.** Every name says where its value came from, so there is never a
question of whether `amount` meant the record's or the row's.

| You write | It reads |
| --- | --- |
| `d.amount` | field `amount` of the record this handler is folding, where `d` is the handler's parameter |
| `b.balance` | column `balance` of the row being written, as it was before this statement |
| `d.position.size` | field `size` of a struct field |
| `tx.version`, `tx.timestamp` | the transaction's version, and its block time in microseconds |

The two names are yours to choose: `on(deposits, (d) => …)` names the record and
`const b = balances.row(d.user)` names the row. `d` and `b` are just what this
documentation calls them.

A column can be read only if a newly created row is certain to have a value for it,
which means it's a key column, has a `.default(…)`, or is `.nullable()`.

An `Object<T>` reads as its address. A `String` reads as a string, a `vector<u8>` as
bytes, and an `Option<T>` as an option (see below).

## Values

| Literal | Type |
| --- | --- |
| `42`, `1_000_000` | an integer; its type comes from context (a `u128` column makes it `u128`) |
| `42u128`, `-1i64` | an integer of that exact type |
| `true`, `false` | `bool` |
| `'text'`, `"text"` | `string` |
| `@0x1` | `address` |
| `null` | an empty option, where a nullable value is expected |

## Operators

Loosest first:

| Operators | Meaning |
| --- | --- |
| `c ? a : b` | conditional; both branches have the same type |
| `\|\|` | or (short-circuit) |
| `&&` | and (short-circuit) |
| `==` `!=` `<` `<=` `>` `>=` | comparison; ordering is for integers; comparisons don't chain |
| `+` `-` | add, subtract |
| `*` `/` `%` | multiply, divide (truncating toward zero), remainder |
| `!` `-` | not, negate (signed integers only) |
| `?.` `??` | read through a row that may not be there, and supply a value when it isn't — see [reading another table](#reading-another-table) |

Both sides of an arithmetic operator have the same integer type. Nothing converts
implicitly: to add a `u64` to a `u128`, write `b.balance + u128(d.amount)`.

## Functions

| Function | Result |
| --- | --- |
| `u8(x)` … `u256(x)`, `i8(x)` … `i256(x)` | `x` converted to that type; fails if it doesn't fit |
| `min(a, b)`, `max(a, b)` | the smaller or larger of two integers of the same type |
| `abs(x)` | absolute value of a signed integer |
| `is_some(o)`, `is_none(o)` | whether an option holds a value |
| `unwrap_or(o, x)` | the option's value, or `x` if it has none |

`o ?? x` is the usual way to write `unwrap_or(o, x)`, and the one Studio generates.
Both compile to the same thing.

## Reading another table

A reducer can look up a row of any other state table by key, and read a column of it.
Here `markets` is a mirror of an on-chain resource holding the fee, and `sales` is the
table being written:

```ts
export const sales = table({
  key:     { id: u64 },
  columns: { amount: u64, fee: u64.default(0) },
})

on(sold, (s) => {
  const sale = sales.row(s.id)
  sale.amount = s.amount
  sale.fee    = s.amount * (markets.get(s.market)?.fee_bps ?? 0) / 10000
})
```

`markets.get(s.market)` names a row: one expression per key column, in the table's key
order. Only a column of it is a value, so there is always a `.column` after it.

**A lookup may find nothing**, which is what `?.` means here and in JavaScript:

| You write | You get |
| --- | --- |
| `holders.get(d.user)?.balance` | the balance, or null if there's no such row |
| `holders.get(d.user)?.balance ?? 0` | the balance, or `0` |
| `holders.get(d.user) != null` | whether the row exists |

If you leave the `??` off, the column you're writing into has to be `.nullable()`. One
or the other — a null has to go somewhere that can hold it.

You can read any `reduce` or `mirror` table, including the one you're writing, where it
means *the row as it was before this statement*. You can't read a `log` table: logs are
append-only history, not state.

A lookup sees state as of just before the statement runs: every record that came
earlier, and every table and rule ordered ahead of it for the same record. That order is
the order you wrote them in, so it doesn't depend on how the stream is batched, and a
replay rebuilds exactly the same state.

## When an expression fails

Overflow (`balance - amount` going below zero for an unsigned column), division by zero,
and a conversion that doesn't fit all stop the project at that transaction, with an
error that names the expression and the version. Nineveh never skips a record or guesses
a value, so a bug in a rule can't silently corrupt state. Fix the rule, then replay.