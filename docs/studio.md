# Studio

The dashboard. Point it at a contract, tick what to follow, and watch tables fill —
without writing a config file or running anything yourself.

Everything here is also reachable over HTTP and from the CLI, so nothing in Studio is a
dead end: the config it writes is the file you would have written, and the Settings page
hands it to you to keep in version control.

## Getting in

Two ways to run the thing Studio talks to:

| | |
| --- | --- |
| **Hosted** | Sign in with GitHub. Your projects, your keys, nothing to run. |
| **Your own machine** | `nineveh up`, then Studio at `http://localhost:3000`. Local mode: no sign-in, loopback only. |

`nineveh up` serves every project in its database. `nineveh run --serve` serves exactly
one, and Studio notices — the pages that only make sense for many projects say so
instead of offering you something that cannot work.

## The words

Studio uses the model's vocabulary rather than inventing a friendlier one, because the
words are the model. Six of them carry most of it:

| | |
| --- | --- |
| **Source** | Chain data you follow: a Move `#[event]`, a resource, or the items of a `Table` inside one. What arrives. |
| **Record** | One occurrence of a source — a single event, a single resource write. Kept, so a reducer can be changed and replayed without going back to the chain. |
| **Reducer** | The rule that turns records into rows. Deterministic, so the same records always give the same table. |
| **State table** | What your app reads. Built by a reducer, served over REST, and the thing a change feed is about. |
| **Version** | Aptos' transaction number. The cursor is the last one folded, and everything is replayable from a version. |
| **Fold** | What a reducer does to a record: apply it to the table and move the cursor. Studio says "folded" where another product would say "processed". |

A state table comes in one of three kinds, and the kind decides what it can answer:

- **`mirror`** — the latest value of each resource or table item, deleted when the chain
  deletes it. The current shape of something.
- **`log`** — every record, in order, never updated. What happened.
- **`reduce`** — a fold you wrote: totals, counts, balances. The numbers the contract
  never stored because storing them would have cost gas.

## Creating a project

**New project**, then paste a contract address. Studio reads the modules at that address
and lists everything followable: each event, each resource, each table inside a
resource, with the ones it cannot follow listed too and the reason why — a generic
struct, a `BigOrderedMap`, a field whose name is not a legal column.

Then three answers:

1. **What to follow.** Everything is ticked. Narrowing it makes the first build quicker.
2. **A name.** Lowercase letters, digits and `_`. It appears in the project's URL.
3. **Where to start.** `auto` finds the first transaction that touched the address, so
   you get the contract's whole history. `now` starts at the chain's head and is
   instant.

Creating writes the config, builds the schema, and starts following. You land on
Overview with a backfill in progress.

## Overview

One card answers the only question that matters at a glance, and it changes with the
project:

- **Backfilling** — a percentage, climbing. The project is reading history.
- **Following** — how many versions behind the chain it is. Zero means caught up.

Under it, demoted on purpose: the cursor, the chain's head, lag in seconds, and
throughput in versions per second. Then cards for the API, storage and the build.

### When something looks wrong

| What you see | What it means | What to do |
|---|---|---|
| **Backfilling** for a long time | Deep history takes a while — a contract with a year behind it is hours, not seconds. | Watch the percentage. It only goes up. |
| **Idle** | Following fine, nothing has happened. | Make something happen. A quiet contract looks exactly like a broken one. |
| **Retrying** | A transient failure — a dropped stream, a rate limit. It recovers by itself. | Nothing. |
| **Paused** | Somebody paused it. The API keeps answering, with rows that have stopped advancing. | Resume it in Settings. |
| **Stopped at version N** | A record the reducers could not fold. Nothing is lost: it stopped rather than skipping, so what it served is correct. | Fix the rule it names, and it resumes from there. |
| **No rows, but it says Following** | Usually a source that has never matched. Studio raises this after 50,000 quiet versions and names the likely cause. | Check the source's type against the contract — a wrong event name matches nothing, silently. |

## Sources

**Sources** is what the project reads off the chain: each source, the Move type it
follows, and how many records it has matched. One that has matched nothing says so in
words rather than showing a zero, because a zero reads as a count and this is a
question — usually a quiet contract, occasionally the wrong type. Open a source to see
the fields a record carries, which is exactly what a rule on it can read.

**Follow something new** adds to the list. It reads the contract this project already
follows — the address is shown rather than typed, because a different contract is a
different project — and gives you the same catalog **New project** does, with everything
already followed marked as such and un-tickable.

Saving adds the source, and the raw table it brings, to the project's config. Your
comments and your own `reduce` tables come back exactly as they were; a name that
clashes with one already in use is numbered rather than written over.

**Then it rebuilds, and this is the expensive config change.** Nineveh keeps every
record it has decoded, which is what makes editing a rule a local replay of minutes.
A new source has no such history — nothing was following it — so the rebuild re-reads
the project's whole version range from the Transaction Stream. Your tables keep serving
the rows they have while that runs and swap over in one transaction when it finishes,
but they stop advancing until then, and the change feed starts again from the new
build, so anything reading it has to resync. The page says so, with the number of
versions, before you press the button.

## Tables

The sidebar lists every state table, `reduce` first, with a dot beside the ones
changing right now. Each table has five tabs:

- **Data** — the grid. Sort by any column, filter a column to an exact value, page
  through. Rows flash as the feed changes them, so you can watch a transaction land.
- **Reducers** — the rules that build this table, with completion and inline errors.
- **Schema** — the columns and their types, which is what the generated TypeScript is
  built from.
- **API** — real requests against this table, ready to run and copy.
- **Changes** — this table's change feed as it arrives.

**Wide integers are strings.** A `u64` does not fit a JavaScript number, so the grid and
the API render `u64` and wider as decimal strings. The grid labels the type above each
column so this is visible rather than surprising.

## Building a state table

**New state table** opens the project's reducers file — the whole file, not one table,
so a file declaring two tables makes two.

It checks as you type: **checking…**, then **checks out** or **won't build** with the
error located in the file. Nothing is saved until it compiles. **Try it on real data**
folds a sample of the project's stored records through your rules and shows the rows
they would produce, before you commit to them.

Saving replays the project's stored records through the new rules. That is minutes for a
project with real history, not another backfill — the records are already there. Adding
a *source* is the expensive one, because no history exists for something you never
followed.

The language is small and total: [Reducers](reducers.md) and
[Expressions](expressions.md) are the references.

## API

Every request an app makes against this project, with the table to choose. Each one says
what it is for, runs against your real data, and copies four ways: the URL, a `curl`, a
`fetch`, and the React hook call from [`@nineveh/react`](react.md) — usually the one you
want, since it holds the change feed open for you.

Each table has the same thing under its own **API** tab, narrowed to that table.

## Webhooks

An endpoint Nineveh calls when a table changes, so your server stops asking whether
anything has. Each one is signed, retried with backoff, and reports its own health: what
it last answered, how long it took, and how far behind it is. **Send a test** proves the
URL before you trust it.

## Change feed

Every change the project commits, across every table, as it happens — the newest at the
top. Filter to one table, or pause the stream to read what went past.

This is the same feed the API serves and webhooks are delivered from, shown raw. It is
most useful for the question "is anything happening at all", which a table's own
**Changes** tab answers only for that table.

## Settings

The project's config, read-only — copy it into your repo, since it is the artefact of
record. API keys: one per app that reads this project, each revocable on its own, shown
once. And the two heavy actions, both of which say what they cost before you confirm:
pause, and delete.

## What Studio cannot do yet

- **Stop following a source.** You can add one; taking one away still means editing
  `nineveh.yaml` and running the CLI. It would be the cheap direction, too — a rebuild
  that drops a source replays the records Nineveh already has.
- **Edit the config in Studio.** The sheet shows it; it does not take changes. Adding a
  source and writing a reducer are the two things that change it, and both go through a
  form rather than the text.
- **Aggregate.** The API compares and orders; it does not group or sum. A number you
  want to read is a number to keep in a `reduce` table, computed once as rows are
  written rather than searched for on every request.

Next: **[Reading your data](reading.md)** for the HTTP contract, or
**[Reading from React](react.md)** for the hooks.
