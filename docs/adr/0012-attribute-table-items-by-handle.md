# 0012. Attribute table items by handle, learned from their parent

- Status: Accepted
- Date: 2026-09-15
- Supersedes: the item-matching rule of ADR 0003 (table items stay a source kind)

## Context

ADR 0003 matched a `table:` source's items by their key and value types, and fell back
to handles only when `nineveh validate` saw a collision in the lock. Resolving real
configs (ADR 0011) showed that default is unsound:

- **Collisions inside one contract.** The mainnet perp DEX's `TradingVolumeBucket`
  holds two `Table<address, VolumeHistory>` fields, `user_taker_volume_history` and
  `user_maker_volume_history`. In `mainnet-7205731421.pb`, one transaction writes the
  bucket, with handles `0xb4d3…` and `0x696d…`, and then one `VolumeHistory` item to
  each handle. Their types are identical, so only the handle tells them apart.
- **Collisions across the chain.** Every contract's `Table<address, u64>` has the same
  item types. A type-matched source would ingest items from tables it has nothing to do
  with, and anyone could write such items to inject rows into another project's state.
- **Entries that are never items.** `BigOrderedMap` keeps its root node inline in the
  parent struct (`BPlusTreeMap { root: Node<K, V>, nodes: StorageSlotsAllocator<…> }`).
  The slots table is `Option<TableWithLength<…>>` and stays empty until the map grows,
  so a small map never produces a table item. Its entries appear only in the parent's
  writes.

## Decision

- **A table item belongs to a source only if its handle has been attributed to that
  source.** Type matching remains in the decoder as a cheap pre-filter. The engine drops
  an item whose handle isn't attributed: it's someone else's table.
- **Handles are learned from the parent.** Whenever a value of the source's parent
  struct is decoded, the pipeline reads the handle at the table field and attributes
  it. This happens whether the parent is a resource, a table item, or nested inside
  either. The path depends on the container:
  - `Table`: `.handle`
  - `TableWithLength`: `.inner.handle`
  - `SmartTable`: `.buckets.inner.handle`
  - `BigOrderedMap`: through `nodes.slots`, once that exists

  Attributions are internal state, written in the same commit as the state they gate
  (ADR 0005), so replay reproduces them.
- **Learn before routing.** A parent and its first items usually arrive in the same
  transaction, so each transaction is processed in two passes: first learn every handle
  its changes reveal, then route its items. Deletes are routed the same way, which ends
  ADR 0010's "candidate" deletes.
- **Parent writes feed `BigOrderedMap` entries.** The inline root node is diffed on
  every parent write, alongside nodes arriving as items, as ADR 0003 already does for
  node items.
- **Coverage depends on seeing parents created.** From `start_version: auto`, which is
  the module's publish version, every parent's creation is observed, so attribution is
  complete. A later explicit `start_version` can miss parents created earlier. So
  `nineveh init` refuses that combination unless the source lists the handles to follow
  (`handles: [0x…]`), which are read from current chain state at init.
- **Finding parents.** `nineveh init` pins layouts for where the parent can occur: the
  parent itself if it's a resource, and the resources and table values that contain it.
  The pipeline can then decode every write that can reveal a handle. This extends the
  lock with struct abilities (to know which structs are resources); that's a lock format
  bump when it lands.

## Alternatives considered

- **Keep type matching, reject collisions (the interim ADR 0011 check).** This only
  sees collisions inside the lock. It can't see other contracts' tables, and it misses
  inline `BigOrderedMap` entries.
- **Pin every handle at init.** This fails for tables created after init, such as
  per-user objects, and those are the common case.

## Consequences

- Table sources become exact: one table, no foreign items, and taker and maker kept
  apart. The resolve-time collision check from ADR 0011 goes away once attribution
  lands in the engine.
- Every table source also decodes its parent's writes, which costs some decode work.
- Handle attribution is engine state with its own tests: parent and item in one
  transaction, items before any parent is seen (dropped), and replay equality.
