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
| [0005](0005-pure-fold-atomic-commit.md) | Pure fold, atomic commit, one-way dependencies | Accepted; rebuild details in 0016 |
| [0006](0006-transactional-outbox.md) | Change feeds come from a transactional outbox | Accepted; feed granularity superseded by 0014 |
| [0007](0007-typed-total-expression-language.md) | Reducers use a typed, total expression language | Accepted |
| [0008](0008-move-postgres-type-mapping.md) | Move → Postgres → API type mapping | Accepted |
| [0009](0009-toolchain-and-engineering-baseline.md) | Toolchain and engineering baseline | Accepted |
| [0010](0010-lock-format-and-decode-boundary.md) | Pin layouts in a JSON lock; decode protos without a runtime | Accepted |
| [0011](0011-project-config-semantics.md) | Project config: two-phase loading and explicit row semantics | Accepted |
| [0012](0012-attribute-table-items-by-handle.md) | Attribute table items by handle, learned from their parent | Accepted |
| [0013](0013-fold-semantics-and-row-shapes.md) | Fold semantics, row shapes, and the missing-key retry | Accepted |
| [0014](0014-state-schema-layout.md) | Store each build in its own schema, keyed by exact bytes and fingerprinted | Accepted |
| [0015](0015-resolve-auto-start-at-init.md) | Resolve `start_version: auto` at init from the Indexer API, pinned in the lock | Accepted |
| [0016](0016-shadow-rebuild-and-swap.md) | Rebuild into a shadow schema and swap it in | Accepted |
| [0017](0017-local-control-plane.md) | Run projects under a control plane that Studio drives | Accepted |
| [0018](0018-accounts-sessions-and-api-keys.md) | Sign in with GitHub; reach projects with API keys | Accepted |
| [0019](0019-reducers-read-other-tables.md) | Let a reducer read other tables by key | Accepted |
| [0020](0020-signed-webhook-deliveries.md) | Deliver state changes as signed webhooks, one cursor per endpoint | Accepted |
| [0021](0021-shared-tip-ingest-and-queued-backfill.md) | Acquire the chain once per network; replay projects from their own records | Proposed |
| [0022](0022-per-project-record-log.md) | Keep each project's decoded records; let the log lead the fold | Proposed |
| [0023](0023-idle-projects-fold-on-demand.md) | Fold on demand; idle is invisible, stopped is deliberate | Proposed |
| [0024](0024-retention.md) | Retain the outbox by time and the record log by size | Proposed |
