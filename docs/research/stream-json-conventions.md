# Transaction Stream JSON rendering conventions

The Transaction Stream carries Move values as JSON strings rendered by the fullnode
(ADR 0002). This catalogue lists every convention `nineveh-decode` must handle, each
backed by a fixture captured from testnet or mainnet on 2026-09-14 (M0 spike B).

Fixture paths are relative to `fixtures/<network>/`. Inspect one with:

```sh
cd crates/nineveh-proto/proto && protoc --decode=aptos.transaction.v1.Transaction \
  -I . aptos/transaction/v1/transaction.proto < ../../../fixtures/<network>/<file>.pb
```

> Fixtures are re-encoded by prost against the pinned protos. Fields the server sends
> that our pinned protos don't know about are dropped at capture time.

## Values

| Move type | Rendering | Fixture | Notes |
| --- | --- | --- | --- |
| `u8`, `u16` | JSON number | `testnet-11196227787.pb` | `"inner_max_degree":102` (u16) |
| `u32` | JSON number (expected) | not yet observed | |
| `u64`, `u128`, `u256` | decimal **string** | `testnet-11196227786.pb` | `FungibleStore.balance` = `"18441553330519219599"`, **above 2^63**, so it overflows `BIGINT` (ADR 0008) |
| `i64`, `i128` | decimal **string**, `-` for negatives | `mainnet-7205731421.pb` | `AccumulativeIndex.index` (i128) = `"-6294071852848"`; `PerpPosition` i64 fields. Signed integers are used by real contracts. |
| `i8`, `i16`, `i32`, `i256` | JSON number / decimal string (expected, like unsigned) | not yet observed | |
| `bool` | `true` / `false` | `testnet-11196227786.pb` | `"frozen":false` |
| `address` | `0x` hex with **leading zeros stripped** | `testnet-11196227807.pb` | `proposer` has 63 hex digits; `0xa` in `testnet-11196227786.pb`. Normalize to 64. |
| `vector<u8>` | `0x` hex string | `testnet-11196227807.pb` | `"previous_block_votes_bitvec":"0xf600"`. From the JSON alone this looks the same as a short address, so only the layout can tell them apart. |
| `String` | JSON string (expected) | to pin in M1 | needs the module ABI to identify a `String` field |
| `vector<T>` | JSON array | `testnet-11196227787.pb` | `"ask_prices":["125350000",…]` |
| `Option<T>` | `{"vec":[]}` / `{"vec":[x]}` | `testnet-11196227786.pb` | `PairedFungibleAssetRefs.burn_ref_opt` |
| `Object<T>` | `{"inner":"0x…"}` | `testnet-11196227786.pb` | inner address also short-form (`"0xa"`) |
| Move 2 enum | object with `"__variant__"` plus the variant's fields | `testnet-11196227787.pb` | `{"__variant__":"V1","ask_prices":…}`; nested enums too (`BPlusTreeMap`, `Occupied`) |
| `Table` handle | `{"handle":"0x…"}` in the parent | `testnet-11196227787.pb` | |

## Records

| Shape | What arrives | Fixture | Notes |
| --- | --- | --- | --- |
| Module event (`#[event]`) | `key {account_address:"0x0"}`, no creation number, sequence 0 | `testnet-11196227786.pb` | `0x1::fungible_asset::Withdraw` |
| Handle event (legacy) | `key {creation_number, account_address}`, real `sequence_number` | `testnet-11196227807.pb`, `testnet-6000000000.pb` | `NewBlockEvent`, `coin::DepositEvent` |
| Generic resource | type params in `type_str` | `testnet-6000000000.pb` | `0x1::coin::CoinStore<0x1::aptos_coin::AptosCoin>` |
| Generic event | type params in `type_str` | `mainnet-7205748025.pb` | `…::identity::WitnessDropEvent<…>` |
| Resource group member | an individual `WriteResource` per member; **all members at one address share the group's `state_key_hash`** | `testnet-11196227786.pb` | Identity is `(address, type)`, never `state_key_hash`. |
| `WriteTableItem` | top-level `key` is **raw BCS hex**; `data.key`/`data.value` are JSON; `data.key_type`/`value_type` are type strings | `testnet-11196231182.pb` | a u64 key is `key:"0x0000000000000000"` and `data.key:"\"0\""`. Decode from `data`, not the BCS. |
| `DeleteTableItem` | top-level BCS `key` plus `data { key (JSON), key_type }`; no value | `testnet-6000029471.pb` | struct key `market::Order` as JSON; every delete observed carried `data` |
| `SmartTable` bucket | value `vector<smart_table::Entry<K,V>>` of `{hash,key,value}`, keyed by `u64` bucket index | `testnet-11196231182.pb` | `fund_token::ShareHolder`; entries must be diffed per bucket for removals (ADR 0003) |
| `BigOrderedMap` node | value `storage_slots_allocator::Link<big_ordered_map::Node<K,V>>`, enum `Occupied`/`Vacant`, leaves hold `SortedVectorMap` entries | `testnet-11196227787.pb`, `mainnet-7205730378.pb` | used by real order-book DEXes on both networks (`0xe7da…`, `0x50ead…`) and `nonce_validation` |
| `aggregator_v2::Aggregator<u64>` | materialized `{"max_value":"…","value":"…"}` | `mainnet-7205730378.pb` | a concurrent counter arrives as a concrete value, not a delta |
| Custom-app transaction | custom events, generic resources, tables and `BigOrderedMap` in one transaction | `mainnet-7205731421.pb` | perp DEX `0x50ead…`; 234 KB, the end-to-end decode fixture for M1 |
| `SmartTable<address, u64>` | bucket of `{hash,key,value}` entries | `mainnet-7205730568.pb` | minimal `SmartTable` case |
| `DeleteResource` of a group | **one delete of the group type** (`0x1::object::ObjectGroup`), with no per-member deletes and no member writes | `mainnet-7205805457.pb` | A `resource:` source for a group member never sees a delete of its own type. The decoder must translate a group delete into deletes of every member at that address, so `nineveh.lock` must record group membership. |
| Member removed, group survives | unknown: possibly no record at all | to capture in M1 | Needs a targeted fixture from our own test contract. If nothing arrives, member removal is only visible by diffing the group's member set. |
| `WriteModule` | module bytecode + ABI | `testnet-6000041078.pb` | 145 KB fixture |
| Failed transaction | `success` false plus `vm_status`, **write set still committed** (gas, sequence number, supply) and a `FeeStatement` event | `testnet-11196281560.pb` | Don't skip failed transactions wholesale. |

## Still to capture

`u32`, `i8`–`i32`, `i256`, `String` (pinned against an ABI), a standalone (non-group)
`DeleteResource`, and removing one member while its group survives. Capture these from a
purpose-deployed test contract in M1.

## Tests

`crates/nineveh-decode/tests/fixtures.rs` decodes every event, resource and table item in
every fixture against a lock built from the trimmed module ABIs in `fixtures/abi/`, and
asserts each convention above against the fixture it names.
