# 0003. Table items are a first-class source kind

- Status: Accepted
- Date: 2026-09-14

## Context

The brief defines two source kinds, `event:` and `resource:`, and warns that events
alone under-cover contract state. Resources alone do too. State kept in
`aptos_std::table::Table`, `smart_table::SmartTable` or `big_ordered_map::BigOrderedMap`
never appears as a resource write:

- The parent resource stores only a handle, e.g. `{"handle": "0x…"}`. Its write doesn't
  recur when items change.
- Each item is its own `WriteTableItem { handle, key, data { key, key_type, value,
  value_type } }` or `DeleteTableItem`.
- `SmartTable` stores entries in buckets: the item value is a
  `vector<smart_table::Entry<K, V>>` keyed by a `u64` bucket index, not by the user's
  key.

Many real contracts keep their core state (positions, orders, balances) in exactly these
structures. With only `resource:` sources, Nineveh would miss that state without
reporting any error.

## Decision

- Add `table:` as a third source kind. It names the struct field that holds the table,
  e.g. `table: 0xabc::vault::Vault.positions`.
- **Matching:** by default, table items are matched on the `key_type` and `value_type`
  pinned in `nineveh.lock`. This is deterministic and needs no history, so it works
  from any `start_version`. When `nineveh validate` finds another table with the same
  key and value types, the source must use handle attribution. The handle is learned
  from parent resource writes in the stream and persisted by the pipeline. An item
  whose handle hasn't been attributed is a fatal, located error, never a guess.
- **`SmartTable`:** the decoder expands buckets into per-entry records. Removals are
  detected by diffing a bucket against its previous contents, which the pipeline keeps
  as internal state.
- **`DeleteTableItem`** becomes a delete record for the matched key.
- M1 ships `Table`, `SmartTable` and `BigOrderedMap`. M0 fixtures show `BigOrderedMap`
  in real use: an order-book DEX (`0xe7da…::market_types`) and the framework's
  `nonce_validation` keep their state in it. Nodes arrive as
  `storage_slots_allocator::Link<big_ordered_map::Node<K, V>>` items, with
  `Occupied`/`Vacant` enum variants and `SortedVectorMap` leaves. Like `SmartTable`,
  node rewrites are diffed against previous contents to derive per-entry upserts and
  deletes.

## Alternatives considered

- **Fold table items into `resource:` sources.** This hides a different identity
  (handle and key, not address) and different delete semantics behind one keyword.
- **Always attribute by handle.** This requires having seen the parent's creation, so it
  fails for any project that starts mid-history.

## Consequences

- Coverage matches how contracts actually store state. This is part of the
  decode-coverage moat.
- `SmartTable` and `BigOrderedMap` diffing needs persisted bucket or node state: one
  internal table per such source, written in the same commit as the state (see ADR
  0005).
