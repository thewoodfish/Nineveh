# 0002. Decode the stream's JSON against pinned Move layouts

- Status: Accepted
- Date: 2026-09-14

## Context

The early design assumed Nineveh would decode BCS. The Transaction Stream doesn't
deliver BCS. In `transaction.proto`, Move values are already-rendered **JSON strings**:

- `Event.data: string`
- `WriteResource.data: string`
- `WriteTableItem.data: WriteTableData { key, key_type, value, value_type }`, all strings

The fullnode renders these from module ABIs. Nested structs and generics arrive already
expanded, but in the renderer's conventions:

- integers wider than 32 bits are decimal strings;
- `vector<u8>` is a hex string;
- `Option<T>` is `{"vec": []}` or `{"vec": [x]}`;
- `Object<T>` is `{"inner": "0x…"}`;
- addresses may appear in short form.

[docs/research/stream-json-conventions.md](../research/stream-json-conventions.md) keeps
the full catalogue, and each entry is backed by a fixture.

The JSON carries no field types. `"123"` could be a `u64` or a `String`, and `"0x1"`
could be an address or bytes. Decoding it correctly needs the struct's layout.

## Decision

- `nineveh-decode` is a **type-directed JSON decoder**. It walks the JSON guided by a
  `Layout` and produces typed values: integers exact to `u256`, addresses normalized to
  32 bytes, and bytes decoded from hex.
- Layouts come from module ABIs, fetched once by `nineveh init` and pinned in
  `nineveh.lock`. **Decoding never touches the network.** Replay from the lock is
  reproducible, and CI can run with no network access.
- A value that doesn't match its layout is a **fatal, located error**: version, source,
  JSON path, expected type, and the offending text. The decoder never coerces and never
  skips a record silently.
- Every rendering convention the decoder handles has a fixture from a real transaction
  and a snapshot test.

## Alternatives considered

- **Schema-less decoding (infer types from JSON).** This is ambiguous for exactly the
  types that matter (`u64` vs `String`, address vs bytes) and would silently mistype
  data.
- **Decode BCS ourselves.** The stream doesn't carry BCS for these fields. We could
  fetch it from a fullnode, but that adds a network dependency to decoding.

## Evidence (M0 spike B, 2026-09-14)

Fixtures confirm the premise and add details the decoder must handle (full catalogue in
`docs/research/stream-json-conventions.md`):

- Addresses arrive with **leading zeros stripped** (63-digit `proposer`, `"0xa"`).
  `"0xf600"` is a `vector<u8>`, and from the JSON alone it looks the same as a short
  address.
- Table items carry the key twice: top-level `key` as **raw BCS hex**, `data.key` as
  JSON. The decoder reads `data`, so BCS stays out of scope.
- Resource-group members share the group's `state_key_hash`. Resource identity is
  `(address, type)`.
- Deleting an object arrives as **one `DeleteResource` of the group type**
  (`0x1::object::ObjectGroup`), with no per-member deletes. `nineveh.lock` must record
  which structs are group members, and the decoder turns a group delete into member
  deletes. Whether removing a single member from a surviving group produces any record
  is still open. That needs a targeted fixture in M1.
- Failed transactions still commit a write set (gas, sequence number). The decoder
  handles them like any other transaction; only their aborted effects are absent.

## Consequences

- The risky layer shifts from byte parsing to the renderer's conventions. The fixture
  suite is what catches drift when Aptos changes them.
- Aptos' upgrade compatibility rules prevent changing an existing struct's layout, so a
  pinned layout stays valid. If one ever fails to decode, we fail at that version rather
  than guess.
- Users get field types for free from `nineveh init` and never hand-type a schema that
  can drift from the chain.
