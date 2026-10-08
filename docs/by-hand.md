# What you'd build instead

The honest version of "why not just write it yourself" — what an indexer for your own
contract actually involves, step by step, with the parts that bite.

Everything here is specific to Aptos and to the market contract the
[tutorial](first-backend.md) walks through: a log of sales, a mirror of what's for sale
right now, and a revenue leaderboard the contract never stores. It is the smallest
realistic backend, and it is still all of this.

## 1. The version everyone writes first

A script that polls the fullnode REST API and writes rows into Postgres or Supabase on
a timer. A day's work. For "show the last hundred sales" it is genuinely the right
answer, and you should write it.

It stops being the right answer at three specific points.

**The REST API is pruned.** A fullnode serves recent history and drops the rest —
testnet's window starts around version 11 billion. Anything older is gone, so you can
neither backfill nor recover from a gap. The only complete source of history is the
Transaction Stream.

**It answers point reads only.** One resource at one address, one view function, one
table item by key. There is no "every open listing": to read them you must already know
every id, which is the thing you were trying to build.

**Polling drops and duplicates.** Two sales between polls and you record one. A retry
after a timeout and you record one twice. Nothing in the response tells you which
happened, and a leaderboard that is quietly 3% wrong looks exactly like one that isn't.

Aptos' hosted Indexer doesn't close the gap either: it serves the generic things —
fungible assets, digital assets, ANS — not your contract's own state. Its `events`
table has since been deprecated.

## 2. Reading the chain properly

So you consume the **Transaction Stream**: Aptos' gRPC feed, every transaction in
commit order, with its events *and* its write set. That means protobuf, a long-lived
gRPC connection with retries, and a [Geomi](https://geomi.dev) key.

Ask for zstd. Uncompressed is 12–22× slower, and the stream is the only thing standing
between you and the whole chain.

## 3. Decoding Move values

The stream does not give you BCS. It gives you the fullnode's **JSON rendering** of
Move values, and that rendering has conventions you have to learn one surprise at a
time:

- `u64`, `u128` and `u256` arrive as strings, because JSON numbers can't hold them.
- `Option<T>` is `{"vec": []}` or `{"vec": [value]}`, not `null`.
- Addresses appear both with and without leading zeros, and `0x1` and the 64-character
  form are the same address.
- Enums, nested structs, generics and vectors each have their own shape.

To decode any of it you need the module's ABI, which means fetching and pinning it —
and pinning matters, because a contract upgrade that changes a struct changes what your
decoder should expect. Decode the old shape with the new layout and you don't get an
error, you get wrong numbers.

This is the layer that fails silently, which is why every convention needs a test
against a real transaction rather than one you wrote from the documentation.

## 4. The two thirds that aren't events

Events are the easy part, and for most contracts they are not most of the data.

**Resources** live in the write set — the exact storage slots a transaction changed.
Reading them means walking `WriteResource` and `DeleteResource` entries and matching
types yourself. You cannot avoid this by following only events, because a contract
emits events for what it expected you to want and keeps the rest in storage: a running
total, a pool's current reserves, a status flag that changes with no event behind it.
Whether any given number is emitted was decided by whoever wrote the contract, for their
reasons, before you turned up.

**Table items** — `Table`, `SmartTable`, `BigOrderedMap` — are worse. An item arrives
keyed by its table's **handle**, and nothing in that record says which contract or
which field the handle belongs to. The parent resource holds the handle, so you only
learn the association when the parent is written. You have to watch for parents, record
their handles, and route items by handle from then on.

Then the corner: deleting an object emits **one** `DeleteResource` for
`0x1::object::ObjectGroup`, not one per member. A reader that matches on member types
sees nothing and never deletes the row. Your mirror grows monotonically and looks fine.

## 5. Applying it exactly once, in order

`revenue += price - fee` is a read-modify-write. For one seller it must apply in version
order, exactly once, forever.

That gives you two hard requirements:

- **Partition by state key, never across one.** Two workers on the same seller will
  interleave a read and a write and lose an update. You can parallelise across keys; you
  cannot parallelise within one.
- **The cursor commits with the rows.** If you write rows and then save "processed up to
  version N" separately, a crash between them replays those rows and double-counts.
  They have to be in the same database transaction.

Neither of these announces itself when you get it wrong. The numbers are simply off,
and you find out from a user.

## 6. Catching up

A single stream sustains roughly 3.5–11k versions per second. Filters don't change it
much — they save bandwidth, not the server's scan — and **the stream cannot filter write
set changes at all**, only events, senders and entry functions. So any project that
follows a resource or a table reads every transaction on the network.

For a contract published today that's fine. For real history you need several streams
over disjoint version ranges, reassembled into version order before the single-writer
fold — and Geomi caps how many streams an organisation may hold open at once, so you
also need a slot pool and a queue so a backfill waits instead of being refused.

## 7. Schema choices that bite later

Postgres `BIGINT` is signed: 2^63−1. A Move `u64` goes to 2^64−1. A `u128` is not close
to fitting anything Postgres has as an integer.

So wide integers are `NUMERIC`, and they leave your API as **strings**, because
JavaScript numbers stop being exact at 2^53. Get this wrong and everything works until
someone trades a big number, at which point the row either errors or silently rounds.

## 8. Changing your mind

This is the one teams don't plan for.

We changed this leaderboard mid-tutorial: `revenue` was the price buyers paid, and it
should have been the price less the market's fee. One line.

Applying that to rows you already have means recomputing history. If all you kept is
the current state, the only place that history exists is the chain, so a one-line change
means **re-reading the chain from the beginning**. To avoid that you keep every record
you ever matched, which is a second storage system with its own retention policy.

And doing it without downtime means building the new tables *beside* the old ones,
keeping the old ones serving until the new build catches up, then swapping atomically.
That is a feature you must build before you dare change a formula.

## 9. Serving it

REST over your tables with filtering, sorting and paging is ordinary work. The live part
is not: a frontend that polls its own database for changes is the polling problem again,
one layer down. Doing it properly means every state change leaving the same transaction
as the row — a transactional outbox — and a feed that resumes from a position after a
disconnect, so a reconnecting browser misses nothing.

## 10. Operating it

Two things that happened to this project in a single afternoon, both of which you would
own:

**A capped API key stops everything at once.** Geomi bills by bytes streamed and minutes
held open, and caps an organisation's spend monthly. Past the cap every call is refused
with `429` — the same status a transient rate limit uses, so a naive client retries
forever against a wall that will not clear until the month does.

**A stranger's type can halt you.** Because write set changes can't be filtered
server-side, you see every contract's writes. One unrelated contract used a Move 2
function type in a generic argument — `0x…::ft::Holder<|u64|u64>` — and a decoder that
parses a type before checking whether it's yours stops dead on somebody else's data.

Neither is exotic. Both are Tuesday.

## What this adds up to

The naive version is a day and quietly wrong. The correct version is the list above —
and most of those items are not work so much as knowledge you get by being burned.

There is also a part that no amount of work buys you. For "what's for sale right now"
you'd be folding `Listed` minus `Sold` minus `Cancelled` to reconstruct a truth the
contract already wrote down, and you'd be carrying that derivation forever. Reading the
removal from the `SmartTable` is not an optimisation; it's the difference between
reporting what the chain says and maintaining your own opinion about it.

And for state that is never emitted, the event-only version cannot be built at all.

## When to write it yourself anyway

If you need a feed of recent events and nothing else, write the poller. It's a day, you
understand every line, and none of the above applies to you.

The argument for Nineveh gets strong at exactly three points: when you need state the
contract never emitted, when a total has to stay correct rather than approximately
correct, and when you want to change a rule after launch without re-indexing. Most
products reach all three; they just reach them after the poller is in production.

Next: **[Your first backend](first-backend.md)**, which is the whole of the above as a
config file and a page of TypeScript.
