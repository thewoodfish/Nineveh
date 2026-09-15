# Architecture decision records

An ADR records one significant decision: the context that forced it, what we chose, and
what we accept as a result. ADRs are how a reader six months from now learns *why* the
code is shaped the way it is, without archaeology through PR threads.

## When to write one

Write an ADR when a decision is hard to reverse, crosses crate boundaries, or constrains
how contributors must write code: a wire format, a storage layout, a dependency policy,
an invariant. Don't write one for choices a reviewer can judge from the diff alone.

## How

1. Copy [`template.md`](template.md) to `NNNN-short-title.md` with the next number.
2. Open it as `Proposed` in the PR that needs it.
3. Merge it as `Accepted` once the decision is agreed.

ADRs are immutable once accepted. To change a decision, write a new ADR that supersedes
the old one and set the old one's status to `Superseded by NNNN`.

## Index

| ADR | Decision | Status |
| --- | --- | --- |
| [0001](0001-own-stream-client-vendor-protos.md) | Own the Transaction Stream client; vendor the protos | Accepted |
| [0002](0002-decode-json-against-pinned-layouts.md) | Decode the stream's JSON against pinned Move layouts | Accepted |
| [0003](0003-table-item-sources.md) | Table items are a first-class source kind | Accepted; matching superseded by 0012 |
| [0004](0004-server-side-filtering-policy.md) | Filter server-side only when it can't under-cover | Accepted |
| [0005](0005-pure-fold-atomic-commit.md) | Pure fold, atomic commit, one-way dependencies | Accepted |
| [0006](0006-transactional-outbox.md) | Change feeds come from a transactional outbox | Accepted; feed granularity superseded by 0014 |
| [0007](0007-typed-total-expression-language.md) | Reducers use a typed, total expression language | Accepted |
| [0008](0008-move-postgres-type-mapping.md) | Move → Postgres → API type mapping | Accepted |
| [0009](0009-toolchain-and-engineering-baseline.md) | Toolchain and engineering baseline | Accepted |
| [0010](0010-lock-format-and-decode-boundary.md) | Pin layouts in a JSON lock; decode protos without a runtime | Accepted |
| [0011](0011-project-config-semantics.md) | Project config: two-phase loading and explicit row semantics | Accepted |
| [0012](0012-attribute-table-items-by-handle.md) | Attribute table items by handle, learned from their parent | Accepted |
| [0013](0013-fold-semantics-and-row-shapes.md) | Fold semantics, row shapes, and the missing-key retry | Accepted |
| [0014](0014-state-schema-layout.md) | Store each build in its own schema, keyed by exact bytes and fingerprinted | Accepted |
