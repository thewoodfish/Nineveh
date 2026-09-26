# Start here

Nineveh is a backend for your Aptos app. You describe the tables your product needs and
the rules that fill them; Nineveh folds your contract's activity into those tables and
serves them over REST, a live change feed and signed webhooks. It runs on Aptos'
Transaction Stream, and there is nothing for you to operate.

This is the short version: what Nineveh does, the one idea everything rests on, and
where to go next. It takes about five minutes.

These pages describe the hosted service at [nineveh.dev](https://nineveh.dev), where
the infrastructure is ours. Nineveh is also open source, and
[running it yourself](https://github.com/thewoodfish/Nineveh/blob/main/SELF_HOSTED.md)
is a supported path. Everything here works the same either way; only the operating
differs.

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
don't keep it. The shape of your app's data lives in **events and resource changes**: a
stream of things that happened, not a table you can query.

Nineveh is the thing that turns that stream into tables.

## 3. The one idea

Everything else follows from this, so it's worth a minute:

```
chain  →  records  →  reducers  →  state tables  →  your app
```

The chain is the input. Records are what arrived, the events and resource writes your
project follows. **Reducers** fold those records into **state tables**. Your app reads
the tables.

Four things follow, and each one saves you a surprise later:

**State is derived, not written.** There is no `POST /rows`. Every row exists because a
reducer put it there. If a number looks wrong, the reducer is wrong. Nothing else can
have touched it.

**Reducers are deterministic.** No clock, no randomness, no network calls. The same
records always produce the same tables. That is what makes the next two possible.

**Your records are kept.** Change a reducer and Nineveh rebuilds from its own stored
records rather than re-reading the chain: minutes, instead of another backfill. Editing
a rule is cheap.

**Aptos finalises.** Committed transactions never reorg, so there is no rewind logic and
no "wait for N confirmations". A version cursor is the whole safety story.

## 4. What to read next

In order, if you're new:

1. **[Your first backend](first-backend.md)**: publish an example contract, point
   Nineveh at it, and query your own transactions. Start here; everything else makes
   more sense after it. It needs the Aptos CLI, and nothing else.
2. **[Reducers](reducers.md)**: how to say what your tables hold. This is the part
   you'll spend your time in.
3. **[Reading your data](reading.md)**: the REST API, the change feed, webhooks.
4. **[Running a project](running.md)**: changing it, what a rebuild costs, limits, and
   what to do when something looks wrong.

Two references, for when you need a specific answer:

- **[Configuration](config.md)**: every key in `nineveh.yaml`.
- **[Expressions](expressions.md)**: the small language reducer values are written in.

And if you'd rather run it yourself:
[Running Nineveh yourself](https://github.com/thewoodfish/Nineveh/blob/main/SELF_HOSTED.md)
covers building it, the CLI, the control plane, and what you take on as the operator.

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

