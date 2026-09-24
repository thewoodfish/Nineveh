# Start here

Nineveh turns an Aptos contract into a live database with a REST API. You describe the
state you want, and it stays in sync with the chain — no indexer to run, no server to
keep up.

This is the short version: what Nineveh does, the one idea everything rests on, and
where to go next. It takes about five minutes.

## 1. What you get

Point Nineveh at a contract address. It reads that contract's activity from the chain
and folds it into tables you define. Those tables are served three ways:

- a **REST API** over every table, with filtering, ordering and paging
- a **change feed** that pushes every row change as it happens
- **webhooks**, signed, to a URL of yours

You write two things: a short config naming the contract data you care about, and
reducers saying what each record changes. Nineveh does the rest.

## 2. Why this exists

Aptos gives you point reads: one resource at one address, one view function, one table
item by key. That is all the chain itself can answer.

It cannot answer *how much volume did this market do today*, or *show me every open
position*, or *what changed since I last looked*. Those questions need the contract's
history folded into shape, and on-chain storage costs gas, so contracts deliberately
don't keep it. The shape of your app's data lives in **events and resource changes** —
a stream of things that happened, not a table you can query.

Nineveh is the thing that turns that stream into tables.

## 3. The one idea

Everything else follows from this, so it's worth a minute:

```
chain  →  records  →  reducers  →  state tables  →  your app
```

The chain is the input. Records are what arrived — the events and resource writes your
project follows. **Reducers** fold those records into **state tables**. Your app reads
the tables.

Four things follow, and each one saves you a surprise later:

**State is derived, not written.** There is no `POST /rows`. Every row exists because a
reducer put it there. If a number looks wrong, the reducer is wrong — nothing else can
have touched it.

**Reducers are deterministic.** No clock, no randomness, no network calls. The same
records always produce the same tables. That is what makes the next two possible.

**Your records are kept.** Change a reducer and Nineveh replays your own stored records
— seconds — instead of re-reading the chain, which would be hours. Editing is cheap.

**Aptos finalises.** Committed transactions never reorg, so there is no rewind logic and
no "wait for N confirmations". A version cursor is the whole safety story.

## 4. What to read next

In order, if you're new:

1. **[Your first backend](first-backend.md)** — a working project against a live
   contract, end to end. Start here; everything else makes more sense after it.
2. **[Reducers](reducers.md)** — how to say what your tables hold. This is the part
   you'll spend your time in.
3. **[Reading your data](reading.md)** — the REST API, the change feed, webhooks.
4. **[Running a project](running.md)** — changing it, what a rebuild costs, limits, and
   what to do when something looks wrong.

Two references, for when you need a specific answer:

- **[Configuration](config.md)** — every key in `nineveh.yaml`.
- **[Expressions](expressions.md)** — the small language reducer values are written in.

## 5. What Nineveh is not

Worth knowing now, so you don't design around something that isn't there.

**It is not a general backend.** Your tables are a read-only projection of the chain.
There are no writable app tables, no user accounts, no row-level security. If you need
those, you need them somewhere else.

**It does not do live aggregate queries.** You can subscribe to a table or a row and be
told when it changes. You cannot subscribe to "the top ten by volume" and have Nineveh
keep the answer correct as trades arrive. Compute aggregates in your reducers instead,
into a table, and subscribe to that.

**It does not replace Aptos' own indexer** for generic data. Token balances, NFTs and
name lookups are already served by the hosted Indexer API. Nineveh is for state that is
specific to your contract.

**GraphQL isn't built.** The config accepts `graphql: true` and does nothing with it.
REST and the change feed are real.
