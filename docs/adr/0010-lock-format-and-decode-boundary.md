# 0010. Pin layouts in a JSON lock; decode protos without a runtime

- Status: Accepted
- Date: 2026-09-14

## Context

ADR 0002 decided that `nineveh-decode` decodes the stream's JSON against layouts
pinned in `nineveh.lock`, but left three things open: the lock's format, where the
decoder's input types come from, and how records whose identity isn't in the stream
(group deletes, table deletes) are routed. Building the decoder against the M0
fixtures settled them.

- **The decoder has to read stream messages.** The message types were generated into
  `nineveh-ingest`, whose gRPC client pulls in tokio. ADR 0005 forbids tokio in
  `nineveh-decode`.
- **REST ABIs describe structs but not resource-group membership.**
  `#[resource_group_member]` lives in the module's metadata, not its ABI. The perp DEX
  ABIs (`fixtures/abi/mainnet/`) also use Move 2 enums (`is_enum`, `variants`) and
  signed integers (`i64`, `i128`).
- **`DeleteTableItem` carries a key type but no value type.** Many tables share key
  types (every `SmartTable` and `BigOrderedMap` item is keyed by `u64`), so a delete
  can't be matched to a source by type alone.

## Decision

**Protos.** The message types move to a new crate, `nineveh-proto`, which depends only
on `prost`. `cargo xtask codegen` runs two passes: messages into `nineveh-proto`, and
the gRPC client into `nineveh-ingest`, which refers to the messages by extern path.
The vendored `.proto` files now live in `crates/nineveh-proto/proto/`. CI checks that
`nineveh-proto` never depends on tokio, sqlx or tonic.

**Lock format.** `nineveh.lock` is JSON, written deterministically (structs sorted by
name, fields in declaration order) so an unchanged lock regenerates byte for byte:

```json
{
  "format": 1,
  "network": "mainnet",
  "structs": {
    "0x1::coin::CoinStore": {
      "type_params": 1,
      "fields": [{ "name": "coin", "type": "0x1::coin::Coin<T0>" }]
    },
    "0x…::perp_positions::PerpPosition": {
      "variants": [{ "name": "V1", "fields": [{ "name": "size", "type": "u64" }] }]
    },
    "0x1::object::ObjectCore": {
      "fields": [{ "name": "owner", "type": "address" }],
      "group": "0x1::object::ObjectGroup"
    }
  }
}
```

- A struct has exactly one of `fields` and `variants`. `event` marks `#[event]`
  structs, and `group` names the resource group of a group member.
- Field types may refer to the struct's own parameters as `T0`, `T1`, ….
- Addresses in struct names are normalized. Special addresses are written short
  (`0x1`), all others at full width.
- **A lock is closed.** Every struct a layout mentions has a layout too, and each
  reference has the right number of type arguments. The only exceptions are
  `0x1::string::String` and `0x1::option::Option`, which the fullnode renders specially
  and the decoder handles natively. Loading a lock checks all of this, so decoding can
  never meet a type it has no layout for.
- An unknown `format`, or any unknown key, is an error that tells the user to
  regenerate the lock with `nineveh init`.

`LockBuilder` builds a lock from module ABIs, and it's pure. It reports every module it
still needs in one error, the caller fetches those modules, and the build is retried.
Group membership is supplied separately (`set_group`), because `nineveh init` has to
read it from module metadata.

**Routing records the stream can't attribute.**

- A `DeleteResource` of a group type becomes a `GroupDelete { address, group }` record
  for every selected resource source that's a member of that group. This follows ADR
  0002: object deletes arrive as one delete of `0x1::object::ObjectGroup`.
- A `DeleteTableItem` is matched on its key type and emitted as a *candidate*
  `TableDelete` for every table source with that key type. The engine applies it only
  if the source has already written to that handle. Handles are learned from writes,
  which match on both key and value type, and are persisted with state, as ADR 0003
  already requires.
- `SmartTable` buckets and `BigOrderedMap` nodes are decoded whole, as a `TableWrite`
  of the bucket or node. Per-entry upserts and deletes come from diffing each one
  against its previous contents in the engine, per ADR 0003.

**Signed integers.** The decoder handles Move's `i8`–`i256`. `i64` and `i128` are
decimal strings with a leading `-` for negatives, as in fixture
`mainnet-7205731421.pb`. `i8`–`i32` are assumed to be JSON numbers, like `u8`–`u32`;
that's still unconfirmed, and a mismatch fails with a located error rather than being
coerced.

## Alternatives considered

- **Decoder input as our own "raw record" types, converted from protos in the
  pipeline.** That would put knowledge of Aptos' wire shape in `nineveh-pipeline`,
  outside `nineveh-ingest` and `nineveh-decode`.
- **A TOML lock.** It's readable, but deeply nested arrays of tables are verbose, and
  Studio (TypeScript) reads JSON natively.
- **Match table deletes by key type only, with no handle check.** On a shared key type,
  every delete of any `u64`-keyed table would reach every such source.

## Consequences

- `nineveh-decode` depends on `nineveh-core`, `nineveh-proto` and serde, with no async
  runtime. It's tested entirely offline against the fixtures and their trimmed ABIs
  (`fixtures/abi/`).
- `nineveh init` must read resource-group membership from module metadata, which means
  parsing the compiled module's metadata section, before a group-member `resource:`
  source can see deletes.
- Signed integers need a Postgres and API mapping (ADR 0008 covers only unsigned) and
  expression-language support (ADR 0007). Both are follow-ups.
