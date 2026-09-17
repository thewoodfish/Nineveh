# Expressions

Reduce rules compute values with small expressions:

```yaml
reduce:
  - on: deposits
    when: "amount > 0"
    set: { balance: "balance + u128(amount)", deposits: "deposits + 1" }
```

Expressions are **typed**: `nineveh validate` checks them against your record's fields
and your table's columns, and points at the exact spot in `nineveh.yaml` when something
doesn't fit. They're **exact**: integers are Move's, from `u8` to `u256` and `i8` to
`i256`, with no floating point and no silent wraparound. And they're **total**: no loops,
I/O, clock or randomness, so replaying the chain always rebuilds the same state (ADR 0007).

## Names

| You write | It reads |
| --- | --- |
| `amount` | a record field or a row column, whichever has that name |
| `row.balance` | the row's current value of `balance` |
| `deposits.amount` | field `amount` of the record from source `deposits` |
| `position.size` | field `size` of a struct value |
| `tx.version`, `tx.timestamp` | the transaction's version, and its block time in microseconds |

If a name is both a column and a record field, a bare name is an error: write
`row.x` or `<source>.x`. A column can be read only if a new row has a value for it,
which means it's a key column, has a `default`, or is `nullable`.

An `Object<T>` reads as its address. A `String` reads as a string, a `vector<u8>` as
bytes, and an `Option<T>` as an option (see below).

## Values

| Literal | Type |
| --- | --- |
| `42`, `1_000_000` | an integer; its type comes from context (a `u128` column makes it `u128`) |
| `42u128`, `-1i64` | an integer of that exact type |
| `true`, `false` | `bool` |
| `'text'`, `"text"` | `string`; single quotes are easiest inside YAML's double quotes |
| `@0x1` | `address` |
| `null` | an empty option, where a nullable value is expected |

## Operators

Loosest first:

| Operators | Meaning |
| --- | --- |
| `if c then a else b` | conditional; both branches have the same type |
| `\|\|` | or (short-circuit) |
| `&&` | and (short-circuit) |
| `==` `!=` `<` `<=` `>` `>=` | comparison; ordering is for integers; comparisons don't chain |
| `+` `-` | add, subtract |
| `*` `/` `%` | multiply, divide (truncating toward zero), remainder |
| `!` `-` | not, negate (signed integers only) |

Both sides of an arithmetic operator have the same integer type. Nothing converts
implicitly: to add a `u64` to a `u128`, write `balance + u128(amount)`.

## Functions

| Function | Result |
| --- | --- |
| `u8(x)` … `u256(x)`, `i8(x)` … `i256(x)` | `x` converted to that type; fails if it doesn't fit |
| `min(a, b)`, `max(a, b)` | the smaller or larger of two integers of the same type |
| `abs(x)` | absolute value of a signed integer |
| `is_some(o)`, `is_none(o)` | whether an option holds a value |
| `unwrap_or(o, d)` | the option's value, or `d` if it has none |

## Reading another table

A rule can look up a row of any other state table by key, and read a column of it:

```yaml
state:
  markets: { mirror: market_config }   # fee_bps lives on-chain, in a resource
  sales:
    key: [id]
    columns:
      id: u64
      amount: u64
      fee: { type: u64, default: 0 }
    reduce:
      - on: sold
        set:
          amount: "amount"
          fee: "amount * unwrap_or(markets[market].fee_bps, 0) / 10000"
```

`markets[market]` names a row: the key goes in brackets, one expression per key column,
in the table's key order. Only a column of it is a value, so always write `.column`
after it.

**A lookup is always an option**, because the row may not be there:

| You write | You get |
| --- | --- |
| `holders[user].balance` | the balance, or `null` if there's no such row |
| `unwrap_or(holders[user].balance, 0)` | the balance, or `0` |
| `is_some(holders[user].user)` | whether the row exists (a key column is never null in one) |
| `is_none(holders[user].note)` | whether the row is missing *or* its `note` is null |

A rule can read `reduce` and `mirror` tables, including the one it writes — it sees
that row as it was before its own write. It can't read a `log` table: logs are
append-only history, not state.

A lookup sees state as of just before the rule runs: every record that came earlier,
and every table and rule ordered ahead of it for the same record (ADR 0013). That order
comes from your config, so it doesn't depend on how the stream is batched, and a replay
rebuilds exactly the same state (ADR 0019).

## When an expression fails

Overflow (`balance - amount` going below zero for an unsigned column), division by zero,
and a conversion that doesn't fit all stop the project at that transaction, with an
error that names the expression and the version. Nineveh never skips a record or guesses
a value, so a bug in a rule can't silently corrupt state. Fix the rule, then replay.
