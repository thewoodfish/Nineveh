# 0017. Run projects under a control plane that Studio drives

- Status: Accepted
- Date: 2026-09-15

## Context

Until now a developer got a backend by writing `nineveh.yaml` by hand, running `nineveh
init` and `nineveh run --serve` in a terminal, and only then opening Studio, which could
look but not change anything. Studio is meant to be the main surface for teams that
don't write Rust, and the goal is that a developer does as little as possible: name a
contract, pick what to follow, and get a live backend.

That needs something Studio can ask to do things: read a contract, create a project,
start and stop its pipeline. `CLAUDE.md` names it `nineveh-control`. A few facts shape
it:

- Everything `nineveh init` and `nineveh run` do is already written, in the CLI binary:
  pinning layouts, resolving `start_version: auto` (ADR 0015), and running with shadow
  rebuilds (ADR 0016). A second copy would drift.
- A contract's shape is readable up front. The fullnode lists an address's modules with
  their ABIs, and an ABI marks `#[event]` structs (`is_event`), resources (the `key`
  ability) and fields holding a `Table` or `SmartTable`.
- Most of a useful backend needs no rules. An event source can be a `log` table, and a
  resource or table source a `mirror` table (ADR 0011). A developer can have live
  tables before they write any `reduce` rule.

## Decision

**`nineveh up` runs the control plane.** One process, one Postgres, any number of
projects, on one listener (127.0.0.1:4000 by default, with no auth, like `nineveh
serve`):

- `/control/v1/…` is the control API:
  - `GET inspect?network=&address=`: the contract's catalog.
  - `POST scaffold`: a config from picks.
  - `GET projects` and `POST projects`: list and create projects.
  - `GET`, `PUT` and `DELETE projects/{name}`, and `POST projects/{name}/start` and
    `/stop`: one project.
- `/projects/{name}/v1/…` is each project's state API and change feed. These are the
  same routes `nineveh serve` has at `/v1/…`, so an app and Studio call a project the
  same way.

**Projects are stored in Postgres,** in `nineveh.control_projects`: name, config text,
lock text, whether it should be running, and timestamps. The config is the same YAML
as `nineveh.yaml`, and Studio can show and export it. Because the registry is in the
database, restarting `nineveh up` brings every running project back. A project builds
into the schema of its name (ADR 0014). A name is refused if a build with that name
already exists in the database outside the registry.

**Creating a project runs `init`, then `run`,** with the same code. The config is
parsed, its layouts are pinned from the chain, `auto` is resolved, and the config is
resolved against the lock. Only a project that resolves is saved. Problems come back
as the same located diagnostics the CLI prints.

**Scaffolding turns picks into a config.** The catalog lists the items below, and
`scaffold` writes a config for the picks. Each item gets a source and a state table
named after the struct in snake case. If two modules define a struct with the same
name, the module name is added in front.

| Catalog item | Found by | Becomes |
| --- | --- | --- |
| Event | an `#[event]` struct, or a struct held in an `EventHandle` field | an `event` source and a `log` table |
| Resource | a struct with `key` | a `resource` source and a `mirror` table |
| Table | a `Table` or `SmartTable` field of a non-generic resource | a `table` source and a `mirror` table |

The catalog marks items the engine can't follow yet and says why: `BigOrderedMap`
fields, tables in generic structs, and mirrors whose fields collide with key columns.
Scaffolding starts at `auto` by default. "From now" pins the chain's current version, for
a live backend without a backfill.

**Each project runs in-process** as a supervised task: the runner, the API and the feed.
A retryable failure is retried inside the pipeline. A deterministic halt stays halted,
showing its error, until the config changes or someone starts the project again, so
it's never skipped. One project halting doesn't affect the others.

**`PUT` of a changed config rebuilds.** The new config is pinned and resolved, then
saved, and the project restarts. The runner sees the new fingerprint and rebuilds into
the shadow schema while the old build stays served (ADR 0016).

**`nineveh-control` is a library,** and it owns the shared path: layout pinning, start
resolution and the runner move out of `nineveh-cli`. `nineveh init` and `nineveh run`
become thin wrappers over it, so the CLI and the control plane can't drift.

## Alternatives considered

- **Store projects as a folder of `nineveh.yaml` files.** Files are easy to put in git,
  but a hosted control plane needs a database, and so does bringing projects back after
  a restart. Export gives developers the file anyway.
- **Generate the YAML in Studio.** That would keep the naming and source rules in
  TypeScript, where the CLI couldn't use them and Rust's tests couldn't check them
  against real ABIs.
- **Spawn a `nineveh run` process per project.** That means managing processes and a
  port for each project. In-process tasks share one listener, and one project's
  deterministic halt already can't take down another.
- **Give each project its own port.** An app would need a port per project. Prefixing
  the path keeps one origin, and the hosted form becomes a hostname per project.

## Consequences

- A developer's whole path is `nineveh up`, then Studio, with no YAML to write. The
  YAML is still there, the same format the CLI reads, for review and for git.
- Project names are unique per database, and each one names its schema.
- Every running project holds its own stream and a small connection pool. The number
  of projects one machine can run is bounded by stream bandwidth, not by the process.
- No auth yet, so the listener stays on localhost. API keys and multiple users come
  with the hosted plane.
- Webhooks (`realtime` in the config) and a visual `reduce` editor build on this. They
  aren't part of this decision.
