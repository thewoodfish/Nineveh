# 0008. Move → Postgres → API type mapping

- Status: Accepted
- Date: 2026-09-14

## Context

State columns must hold Move values exactly, and the API must return them without loss.
Two traps:

- Postgres `BIGINT` is **signed** 64-bit. Any `u64` above 2^63−1 overflows it. This
  isn't hypothetical: fixture `testnet-11196227786.pb` has a `FungibleStore.balance` of
  `18441553330519219599`.
- JSON numbers lose precision above 2^53 in JavaScript, which is most API clients.

## Decision

| Move | Stream JSON | Postgres | REST / GraphQL |
| --- | --- | --- | --- |
| `bool` | `true` | `boolean` | Boolean |
| `u8`, `u16` | number | `integer` | Int |
| `u32` | number | `bigint` | Int (64-bit scalar in GraphQL) |
| `u64` | `"…"` | `numeric(20,0)` | `U64`, as a string |
| `u128` | `"…"` | `numeric(39,0)` | `U128`, as a string |
| `u256` | `"…"` | `numeric(78,0)` | `U256`, as a string |
| `address`, `Object<T>` | `"0x1"`, `{"inner":"0x…"}` | `text`, normalized `0x` + 64 hex | `Address` |
| `String` | `"…"` | `text` | String |
| `vector<u8>` | `"0x…"` | `bytea` | `Hex`, as a string |
| `Option<T>` | `{"vec":[]}` / `{"vec":[x]}` | nullable `T` | `T` or null |
| struct, `vector<T>` | object / array | `jsonb`, values normalized as above | JSON |

- Addresses are always stored in the full-width normalized form, so `0x1` and
  `0x000…001` can never become two keys.
- Nested values inside `jsonb` use the same normalization: integers as strings,
  addresses at full width.
- The Stream JSON column is confirmed against M0 fixtures (see
  `docs/research/stream-json-conventions.md`). Addresses arrive with leading zeros
  stripped, which the normalization above handles.

## Consequences

- No silent overflow or precision loss anywhere between chain and client.
- `numeric` arithmetic is slower than native integers. Reducers do arithmetic in
  `nineveh-expr`, not in SQL, so this only affects API filters and sorts.
