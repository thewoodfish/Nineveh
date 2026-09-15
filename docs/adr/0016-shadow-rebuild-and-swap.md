# 0016. Rebuild into a shadow schema and swap it in

- Status: Accepted
- Date: 2026-09-15

## Context

ADR 0005 said a config change that alters derived data rebuilds "into a shadow schema
from `start_version`, then swaps atomically", and that `nineveh replay` is a rebuild
with the same config. Until now, a changed config made `Store::open` refuse with
`Rebuild`, and the only way on was `replay`, which dropped the served tables first. A
deep rebuild can take hours, and the app's data would be gone the whole time.

Building the swap settled the details ADR 0005 left open: where the shadow lives, what
triggers a rebuild, when it swaps, and what happens to the change feed.

## Decision

- **Where.** A rebuild of schema `S` builds into `S__next`, an ordinary build with its
  own `nineveh.projects` row, cursor and outbox. Names ending in `__next` are reserved.
- **When.** `nineveh run` rebuilds whenever the served build's fingerprint (ADR 0014)
  doesn't match the config and lock. `nineveh replay --yes` rebuilds with the same
  config, starting the shadow over. A shadow left by an interrupted rebuild resumes
  from its cursor. If it was built under yet another config, it starts over.
- **Until.** The shadow builds through the chain's version when the run began, or
  through `--until`. Then it's swapped in, and the run carries on following the chain
  from there.
- **Swap.** `Store::swap` does it all in one Postgres transaction:
  1. drops `S`;
  2. renames `S__next` to `S`;
  3. deletes `S`'s project row, which cascades to its change feed;
  4. re-keys the shadow's project row and feed to `S`;
  5. notifies `nineveh_changes`.

  Readers see the old build or the new one, never a mix.
- **While rebuilding, the old build stays served but stops advancing.** Only the new
  config is on disk, so nothing can fold the old one forward.
- **The change feed is replaced.** After a swap, `S`'s feed is the new build's, from its
  start. Its `(schema, version, seq)` identities are new. A consumer that sees `S`'s
  fingerprint change must resync rather than resume. This is for `nineveh-realtime` to
  handle.

## Alternatives considered

- **Refuse and require `replay`, which drops the served build first.** That's what M1
  had. The app loses its data for the length of the rebuild.
- **Keep the old build advancing during the rebuild.** That needs the old config and
  lock kept alongside the new ones. It's worth doing once the control plane stores
  config versions; it's out of scope for a CLI reading one `nineveh.yaml`.
- **Swap by renaming tables inside `S`.** It's as atomic, but it would interleave two
  builds' tables in one schema during the build, and the per-schema grants from ADR
  0005 would no longer separate them.

## Consequences

- Config changes no longer take the app's data down. The cost is a second copy of the
  state while the rebuild runs.
- The served data is stale during a rebuild. The status (cursor, lag) shows it.
- Realtime consumers must treat a fingerprint change as a reset of the feed.
- Project names that are 57 characters or longer can't be rebuilt, because the suffix
  wouldn't fit Postgres' 63-byte identifier limit. `shadow_name` says so.
