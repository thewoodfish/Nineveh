# `nineveh.yaml` reference

A Nineveh project is one YAML file. It names the chain data to follow (**sources**),
the tables built from it (**state**), and how they're served. `nineveh init` reads it
to pin the Move layouts it needs in `nineveh.lock`, and `nineveh validate` reports
every problem at the line it's on.

```yaml
name: vault
network: mainnet
start_version: auto

sources:
  deposits:    { event: 0xabc::vault::DepositEvent }
  withdrawals: { event: 0xabc::vault::WithdrawEvent }
  vaults:      { resource: 0xabc::vault::Vault }
  positions:   { table: 0xabc::vault::Vault.positions }

state:
  balances:
    key: [user]
    columns:
      user:    address
      balance: { type: u128, default: 0 }
    reduce:
      - { on: deposits,    set: { balance: "balance + amount" } }
      - { on: withdrawals, set: { balance: "balance - amount" } }
  vaults:    { mirror: vaults }
  positions: { mirror: positions }
  deposit_log: { log: deposits }

api: { rest: true, graphql: true }
realtime:
  - on: balances.changed
    webhook: https://myapp.example/hooks/balance
```

## Top level

| Key | Required | Meaning |
| --- | --- | --- |
| `name` | yes | The project's name. |
| `network` | yes | `mainnet`, `testnet` or `devnet`. |
| `start_version` | no | `auto` (default) or a transaction version. `nineveh init` resolves `auto` to the first transaction that touched any of the sources' contract addresses, which is at or before their modules were published, and pins it in `nineveh.lock`. So nothing relevant is missed, and every build starts at the same place. |
| `sources` | yes | At least one source. |
| `state` | yes | At least one state table. |
| `api` | no | `rest` and `graphql`, both `true` by default. |
| `realtime` | no | Webhooks fired by state changes. |

**Names** of the project, sources, tables and columns are lower snake case: `a`–`z`,
`0`–`9` and `_`, starting with a letter, at most 63 characters. Names starting with `_`
are reserved for Nineveh's own columns.

## Sources

Each source is one of:

| Kind | Example | Records |
| --- | --- | --- |
| `event` | `{ event: 0xabc::vault::DepositEvent }` | each event of that type |
| `resource` | `{ resource: 0xabc::vault::Vault }` | each write and delete of that resource |
| `table` | `{ table: 0xabc::vault::Vault.positions }` | each item written to or deleted from the `Table`, `SmartTable` or `BigOrderedMap` in that field |

A generic struct named without type arguments (`0x1::coin::CoinStore`) matches every
instantiation. With arguments (`0x1::coin::CoinStore<0x1::aptos_coin::AptosCoin>`), it
matches exactly that one. A `table:` source on a generic struct needs its type arguments.

A `table:` source follows exactly the tables held in that field. Nineveh learns each
table's handle when the struct holding it is written, so two tables with the same key
and value types, or another contract's table of the same types, never mix. The holding
struct must be stored as a resource or as a table value. `BigOrderedMap` fields aren't
supported yet: small maps keep their entries inside the struct itself.

Resources are needed alongside events: many contracts expose their real state only as
resource writes, and state kept in tables never appears as a resource write at all.

A project whose sources are all `event:` sources streams only the transactions that emit
those events. Any `resource:` or `table:` source means streaming every transaction,
because the stream can't filter on resource or table writes.

## State tables

Every table is built one way: `reduce`, `mirror` or `log`.

### `mirror`: the latest value

```yaml
vaults: { mirror: vaults }
```

Keeps the latest value of each resource (by address) or table item (by table and key)
from a `resource` or `table` source, and deletes it when the chain does. Columns come
from the source's layout: the key columns first, then one column per field of the value's
struct, typed as in [Column types](#column-types).

| Source | Key columns |
| --- | --- |
| `resource` | `address`, plus `type` for a generic struct named without type arguments |
| `table` | `handle`, `key` |

A table value that isn't a struct is stored in one `value` column, and so is an enum,
since its fields depend on the variant. A field that isn't a valid column name, or has
the same name as a key column, is an error. Build that table with `reduce` instead.

### `log`: every event

```yaml
deposit_log: { log: deposits }
```

One append-only row per event from an `event` source. The key is `version` and
`event_index` (the event's position in its transaction), followed by one column per
field of the event.

### `reduce`: your own fold

```yaml
balances:
  key: [user]
  columns:
    user:    address
    balance: { type: u128, default: 0 }
    memo:    { type: string, nullable: true }
  reduce:
    - on: deposits
      set: { balance: "balance + amount" }
    - on: withdrawals
      when: "amount > 0"
      set: { balance: "balance - amount" }
    - on: vaults.deleted
      key: { user: "address" }
      delete: true
```

- **`key`**: the columns that identify a row.
- **`columns`**: each column's type, written either as `name: type` or as
  `name: { type, default, nullable }`. A column is `nullable` if its value may be absent.
  `default` is its value when a new row is created by a rule that doesn't set it.
- **`reduce`**: rules applied to each record, in version order:
  - `on`: a source name for its events or writes, or `<source>.deleted` for its
    deletes (resources and tables only).
  - `when`: optional; the rule applies only when this is true.
  - `key`: optional expressions for key columns. A key column that isn't listed takes
    the record's field of the same name, which must exist with the column's type.
  - Then either `set: { column: "expression" }`, which creates or updates the row, or
    `delete: true`, which deletes it.

Any `set` rule may be the one that creates a row, so every non-key column needs a value
from each `set` rule: set it there, give it a `default`, or make it `nullable`.

Rules run in the order they're listed, each seeing the one before. Within one `set`,
every expression sees the row as it was before the rule, so `set: { a: "b", b: "a" }`
swaps. A `key` expression reads only the record, since the key is what finds the row.

**What a rule's expressions can read.** Columns of the row are read by name. From the
record:

| Record | Fields |
| --- | --- |
| event | the event struct's fields |
| resource write | the resource's fields, plus `address` |
| resource delete | `address` |
| table write | `handle`, `key`, `value` |
| table delete | `handle`, `key` |

For a Move enum (versioned layouts like `V1`/`V2`), a rule can read the fields every
variant has with the same type.

Expressions are typed, exact integer arithmetic up to 256 bits, with no floating point.
The full language is in [expressions.md](expressions.md). Quote expressions when they
contain YAML punctuation: `"balance + amount"`.

### Column types

| Type | Holds |
| --- | --- |
| `bool` | `bool` |
| `u8` … `u256`, `i8` … `i256` | integers of exactly that width |
| `address` | `address`, and `Object<T>` (stored as its address) |
| `string` | `0x1::string::String` |
| `bytes` | `vector<u8>` |
| `json` | any struct, vector or other structured value |

An `Option<T>` field fits a `nullable` column of `T`'s type. Integers never widen
implicitly: put a `u64` into a `u128` column with an expression.

Defaults are written as YAML values: `0`, `true`, `"text"`. Wide integers can be quoted
decimal strings (`"340282366920938463463374607431768211455"`). Addresses must be quoted
(`"0x1"`), because YAML reads an unquoted `0x1` as the number 1. Bytes are `"0x"`
followed by hex.

## `realtime`

```yaml
realtime:
  - on: balances.changed    # or .inserted, .updated, .deleted
    webhook: https://myapp.example/hooks/balance
```

Webhooks must use `https`; plain `http` is accepted only for `localhost`. Deliveries
are signed, so URLs never carry credentials.

## YAML notes

- Duplicate keys are errors, not silent overrides.
- Booleans are exactly `true` and `false`; `yes`, `no`, `on` and `off` are rejected.
- Floating-point numbers are rejected everywhere.
