# What Nineveh does, for the landing page

Raw material for the site: the pitch, how it works, and what it doesn't do yet. Not
reference documentation — that's [config.md](config.md) and
[expressions.md](expressions.md). Keep the claims here true; the last section exists
so nobody writes copy the product can't back.

## The one-liner

> **Turn your Aptos contract into application data.**

Alternatives, same idea:

- Point Nineveh at your contract. Get a database and an API that stay in sync with it.
- Your contract's data, queryable. No indexer to write, nothing to run.
- Firebase for Aptos contracts.

**Don't lead with "backend".** It makes a reader think auth, business logic, servers and
payments — and the docs then spend a paragraph taking all of that back. The words that
are both accurate and wanted are *application data*, *application state* and *indexing*.
Nineveh removes one specific painful layer; saying so plainly is stronger than implying
it removes all of them.

## The problem

Your contract's data is all on-chain, but the chain answers only one kind of question:
*what is X right now?* One account's balance. One listing by id. One resource at one
address.

It can't sort, total, join, or give you a feed. And the interesting data isn't even in
storage — it's in **events** and **storage writes**, because keeping totals on-chain
costs gas on every transaction.

So every team writes an indexer: a processor, a database, a server, a deploy pipeline.
A week of work, and something to maintain forever. Nineveh is that week, done.

## What a developer does

**1. Paste your contract address.** Nineveh reads the contract off the chain and lists
what it found: the events it emits, the resources it stores, the tables inside them.

**2. Confirm what to follow.** Everything is ticked by default, with a plain summary —
*"4 events — a log table each. 1 resource — a mirror of its latest value."* Click
create.

**3. Query it.** Seconds later there are tables with rows in them, served over REST
with filtering, sorting and paging, plus a live change feed. Point your frontend at it,
or have Nineveh post changes to your backend as signed webhooks.

**4. Then add your own tables** — the part that matters. A handler, or a form in
Studio that writes one for you:

```ts
export const sellers = table({
  key:     { seller: address },
  columns: { sold: u64.default(0), revenue: u64.default(0) },
})

on(sold, (s) => {
  const row = sellers.row(s.seller)
  row.sold    += 1
  row.revenue += s.price - s.fee
})
```

Read it as a sentence: *when a `sold` event arrives, find that seller's row, count the
sale and add what they made.* That's per-seller revenue the contract never stored, and
never had to.

The language is six statements and reads like TypeScript, which is the point: the
people who need this write TypeScript. Nothing is executed — it compiles to a
deterministic fold, so changing a rule replays your stored records in seconds rather
than re-reading the chain.

## How it works

**The chain pushes.** Nineveh sits on Aptos' transaction firehose. Every transaction
arrives in order as it's committed — no polling, no "did anything change?"

**It decodes what you follow.** Each transaction carries its events *and* its write
set: the exact storage slots it changed. That second part is why resources and table
entries work at all — Nineveh sees what the chain actually wrote, even when the
contract emitted no event about it.

**It folds.** Each record runs through your tables: events append to log tables,
resource writes overwrite mirror rows, and your own rules run. Everything from one
transaction lands in **one database write**, together with a bookmark saying "processed
up to here".

**It serves.** Your app reads the tables. Nothing reads the chain at request time, so a
query is just a database query.

Three properties make it trustworthy, and they all fall out of that single commit:

- **In order** — a balance that goes 5 → 12 → 7 lands as 7, never as 12.
- **Exactly once** — rows and bookmark save together, or neither does.
- **Crash-safe** — a restart resumes from the bookmark. Nothing counted twice, nothing
  skipped. And because Aptos finalizes, there is no rollback to handle.

## The idea underneath

**State is a fold of history, and reducers are the only thing that writes it.**

Everything else follows. The fold is pure — no clock, no randomness, no network — so
replaying the chain rebuilds byte-identical state. Which means:

- **Change a rule and Nineveh rebuilds that table from history in the background**, then
  swaps it in. The old data keeps serving. There's no migration to write.
- **You can see a rule's output before you save it.** Nineveh folds it over real recent
  transactions and shows you the rows it would produce.
- **Arithmetic is exact and checked.** A rule that would take a balance below zero stops
  the project at that transaction and names the rule. It never wraps, never skips, never
  quietly corrupts a total.

## What you never do

Install anything. Run a database. Write a processor. Think about backfills, cursors,
retries or crash recovery. Change your contract to expose data it already emits.

## What people build with it

Each of these is a question the chain can't answer and an app needs:

| | What the chain can't do | What Nineveh gives you |
|---|---|---|
| **Game leaderboard** | rank players (it can read one player's record) | `leaderboard?order=wins.desc` |
| **Marketplace** | list what's for sale (listings live in a `SmartTable`, with no address to read) | the live listings, plus revenue per seller |
| **Token or points app** | list holders, or rank them | every holder, sortable, plus each account's history |
| **Social app** | a feed, or "everything by this author" | append-only history, queryable |
| **Protocol dashboard** | totals, volume, who's near liquidation | your own aggregates, updated live |

Nobody in that list changed their contract. The data was already on-chain — it just
wasn't queryable.

## What isn't built yet

Don't put these on the site:

- **GraphQL.** The API is REST (`/v1/tables`, `/v1/tables/{name}`) plus the change feed
  (`/v1/changes`, Server-Sent Events) and signed webhooks. GraphQL is planned, not
  shipped.
- **Quotas and limits.** No usage metering, no plans, no billing.
- **Hosted deployment.** It runs, it's built to be hosted, and nothing is deployed for
  customers yet.
- **Live aggregate subscriptions.** You can subscribe to rows and tables changing, not
  to "the top 10 by volume, kept correct" — that's a much harder problem and
  deliberately out of scope for v1.
