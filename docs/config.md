# Configuration

Every key in `nineveh.yaml`. A project's config names the chain data to follow
(**sources**), the tables copied straight from it (**state**), and how they're served.
The tables you fold yourself live in a [reducers file](reducers.md) beside it.

**You don't normally write this file.** Studio writes it as you add sources, tables and
webhooks, and the config page shows it to read rather than to edit. This page is here
for when you want to know what Studio wrote.

It's also the file you do write by hand if you
[run Nineveh yourself](https://github.com/thewoodfish/Nineveh/blob/main/SELF_HOSTED.md),
where your config lives in your own repository and is reviewed like code: `nineveh init`
reads it to pin the Move layouts it needs, and `nineveh validate` reports every problem
at the line it's on.

```yaml
name: vault
network: mainnet
start_version: auto
reducers: ./vault.nineveh.ts

sources:
  deposits:    { event: 0xabc::vault::DepositEvent }
  withdrawals: { event: 0xabc::vault::WithdrawEvent }
  vaults:      { resource: 0xabc::vault::Vault }
  positions:   { table: 0xabc::vault::Vault.positions }

state:
  vaults:      { mirror: vaults }
  positions:   { mirror: positions }
  deposit_log: { log: deposits }

api: { rest: true }
webhooks:
  my_backend:
    url: https://myapp.example/hooks/nineveh
    on: [balances.changed]
```

`balances` isn't in `state` above. It's a fold — a running total per user, which no
single record holds — so it's declared in `vault.nineveh.ts` along with the handlers
that write it. Webhooks subscribe to it the same way regardless of which file declares
it.

## Top level

| Key | Required | Meaning |
| --- | --- | --- |
| `name` | yes | The project's name. |
| `network` | yes | `mainnet`, `testnet` or `devnet`. |
| `start_version` | no | `auto` (default) or a transaction version. `nineveh init` resolves `auto` to the first transaction that touched any of the sources' contract addresses, which is at or before their modules were published, and pins it in `nineveh.lock`. So nothing relevant is missed, and every build starts at the same place. |
| `sources` | yes | At least one source. |
| `state` | yes* | The `mirror` and `log` tables: copies of what the chain already holds. Optional when `reducers` declares the project's tables instead. |
| `reducers` | yes* | A [reducers file](reducers.md), named relative to this file, holding the tables you fold yourself and the handlers that write them. Optional when `state` alone is enough. A project needs at least one table from one of the two. |
| `api` | no | `rest`, `true` by default. |
| `webhooks` | no | Where state changes are delivered. |

**Names** of the project, sources, tables and columns are lower snake case: `a`–`z`,
`0`–`9` and `_`, starting with a letter, at most 63 characters. Names starting with `_`
are reserved for Nineveh's own columns.

## Sources

Each source is one of:

| Kind | Example | Records |
| --- | --- | --- |
| `event` | `{ event: 0xabc::vault::DepositEvent }` | each event of that type |
| `resource` | `{ resource: 0xabc::vault::Vault }` | each write and delete of that resource |
| `table` | `{ table: 0xabc::vault::Vault.positions }` | each item written to or deleted from the `Table`, `TableWithLength` or `SmartTable` in that field |

A generic struct named without type arguments (`0x1::coin::CoinStore`) matches every
instantiation. With arguments (`0x1::coin::CoinStore<0x1::aptos_coin::AptosCoin>`), it
matches exactly that one. A `table:` source on a generic struct needs its type arguments.

A `table:` source follows exactly the tables held in that field. Nineveh learns each
table's handle when the struct holding it is written, so two tables with the same key
and value types, or another contract's table of the same types, never mix. The holding
struct must be stored as a resource or as a table value.

`BigOrderedMap` fields aren't supported yet, and neither is a table held inside
another struct — `Profile.positions.inner`, where `positions` is a type of your own
wrapping a table. The field a `table:` source names has to hold the table itself. A small map keeps its entries inside the
parent struct rather than in a table of its own, and the engine doesn't read those yet,
so follow the parent with a `resource:` source meanwhile.

Resources are needed alongside events: many contracts expose their real state only as
resource writes, and state kept in tables never appears as a resource write at all.

A project whose sources are all `event:` sources streams only the transactions that emit
those events. Any `resource:` or `table:` source means streaming every transaction,
because the stream can't filter on resource or table writes.

## State tables

A table is built one of three ways. Two of them are copies of what the chain already
holds, and they're configured here:

- **`mirror`**: the latest value of a resource or a table item, one row each.
- **`log`**: every record a source produced, append-only, one row each.

The third is a **reduce** table — a fold you write, like a running total or a count per
day. Those are declared in the [reducers file](reducers.md), not here; see
[your own folds](#your-own-folds) below.

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

A table value that isn't a struct or an enum is stored in one `value` column. A field
that isn't a valid column name, or has the same name as a key column, is an error.
Build that table with `reduce` instead.

**Enums.** A Move enum value, such as a versioned `V1`/`V2` event or resource, gets a
column per field that any of its variants declares, in the order the fields first
appear:

- A field every variant declares with the same type is typed like a struct's field.
- A field only some variants declare is nullable, and null for the others.
- A field declared with different types in different variants is `json`.

An enum with more than one variant also gets a `_variant` column holding the value's
variant, such as `"V2"`. Names starting with `_` are Nineveh's, so no field clashes
with it.

### `log`: every event

```yaml
deposit_log: { log: deposits }
```

One append-only row per event from an `event` source. The key is `version` and
`event_index` (the event's position in its transaction), followed by one column per
field of the event. An enum event gets its columns as described for
[`mirror`](#mirror-the-latest-value).

### Your own folds

Everything above is a copy of something the chain already holds. The tables that are
yours — a total per user, a count per day, a leaderboard — are **reduce** tables, and
they aren't written here. They're declared in the [reducers file](reducers.md) that
`reducers:` names, where a table's columns and the handlers that write them sit
together:

```ts
export const balances = table({
  key:     { user: address },
  columns: { balance: u128.default(0) },
})

on(deposits, (d) => {
  balances.row(d.user).balance += u128(d.amount)
})
```

One file, both halves of the project: `nineveh.yaml` says what to follow and what to
serve, the reducers file says what to compute. Studio writes the first and you write
the second — and [Reducers](reducers.md) is the page for it.

### Column types

| Type | Holds |
| --- | --- |
| `bool` | `bool` |
| `u8` … `u256`, `i8` … `i256` | integers of exactly that width |
| `address` | `address`, and `Object<T>` (stored as its address) |
| `string` | `0x1::string::String` |
| `bytes` | `vector<u8>` |
| `json` | any struct, vector or other structured value |

These are the same type names the reducers file uses, so a column means the same thing
whichever file declares it.

An `Option<T>` field fits a nullable column of `T`'s type. Integers never widen
implicitly: put a `u64` into a `u128` column with an expression.

## `webhooks`

Where Nineveh sends state changes. Each entry is a named endpoint, so several changes
can share one URL, one secret and one delivery cursor:

```yaml
webhooks:
  my_backend:
    url: https://myapp.example/hooks/nineveh
    on: [balances.changed, holders.inserted]   # or .updated, .deleted
    rows: true                                  # optional; see below
```

| Key | Required | Meaning |
| --- | --- | --- |
| `url` | yes | Where deliveries go. |
| `on` | yes | The changes it wants: `<table>.changed`, `.inserted`, `.updated` or `.deleted`. A `log` table only ever inserts, so `.updated` and `.deleted` on one never fire. |
| `rows` | no | Whether a delivery carries the changed row, not only its key. Default `true`. |

`on` may name any of the project's state tables — those `state:` declares and those a
`reducers:` file does, since both are the same kind of table once they're built.

URLs must use `https`; plain `http` is accepted only for `localhost`. Deliveries are
signed with the endpoint's own secret, so URLs never carry credentials.

`rows: false` sends only the key: "this row changed, come and look". That keeps
deliveries small, and it's self-correcting: however the deliveries are retried or
reordered, a fetch always returns current state.

Changing a project's webhooks never rebuilds its tables: they don't shape what's
built.

## YAML notes

- Duplicate keys are errors, not silent overrides.
- Booleans are exactly `true` and `false`; `yes`, `no`, `on` and `off` are rejected.
- Floating-point numbers are rejected everywhere.