# Your first backend

Build a working backend against a live contract, from nothing to querying real data.
Follow it top to bottom — every step produces something you can see.

You need a GitHub account. Nothing to install.

## 1. Pick something that is definitely busy

The hardest part of a first project is not knowing whether silence means *you got it
wrong* or *the contract is quiet*. So start with something that is never quiet: the
Aptos framework itself, published on every network, emitting a block event every few
hundred milliseconds.

Open Studio at <https://studio.nineveh.dev> and sign in with GitHub.

## 2. Create the project

**New project** → network **testnet** → address `0x1` → **Inspect**.

Nineveh reads the contract and lists everything it could follow — for `0x1` that's
around three hundred things, which is far too many for a first look.

Click **None** in every section, then find **Events** and tick `NewBlockEvent`
(from the `0x1::block` module).

> Tick `NewBlockEvent`, not `NewBlock`. They are different types, and testnet only
> emits the first. A source that matches nothing is not an error — you get a project
> that runs perfectly and stays empty, which is exactly what a silent contract looks
> like, so nothing fails and nothing complains.
>
> Studio will tell you once the project has read far enough to be sure: *"this source
> hasn't matched anything yet"*. But it's the commonest way to lose an afternoon, so
> it's worth checking the name twice now.

Name it `blocks`. Choose **From now on** rather than the contract's whole history —
starting at the tip means data in seconds instead of a long backfill.

**Create backend with 1 table.**

## 3. Watch it fill

The Overview shows a cursor climbing, the chain head it's chasing, and the gap between
them. Within a few seconds it reads *following the chain*, and `new_block_event` gains a row
every time testnet produces a block — about a dozen a second.

You now have a backend. It has a URL:

```
https://api.nineveh.dev/projects/blocks
```

## 4. Query it

```sh
BASE=https://api.nineveh.dev/projects/blocks

curl $BASE/v1/tables                       # what tables exist, and their columns
curl "$BASE/v1/tables/new_block_event?limit=3"
```

Then something more specific — the ten most recent blocks, newest first:

```sh
curl "$BASE/v1/tables/new_block_event?limit=10&order=height.desc"
```

Look closely at `height` in the response:

```json
{ "height": "381947", "epoch": "9821" }
```

It's a **string**, not a number. Move integers go up to 256 bits and JavaScript numbers
stop being exact at 2⁵³. Nineveh returns them as strings so nothing is silently
rounded, which means in your app:

```js
const height = BigInt(row.height)   // right
const height = Number(row.height)   // wrong above 9 quadrillion, silently
```

## 5. Watch changes arrive

In a second terminal:

```sh
curl -N "$BASE/v1/changes"
```

Every row change is pushed as it commits:

```
event: change
id: 11292175483.0
data: {"version":"11292175483","table":"new_block_event","op":"insert","key":{…},"row":{…}}
```

Kill it, wait a moment, then resume from where you stopped:

```sh
curl -N "$BASE/v1/changes?after=11292175483.0"
```

Nothing is missed. That `id` is how a browser resumes too — `EventSource` sends it
automatically as `Last-Event-ID`.

## 6. Make a table of your own

So far the table is a log: one row per event, exactly as it arrived. That is useful,
but it isn't what an app usually wants. Apps want *totals*, *current state*, *per
something*.

That is what a reducer is for. In Studio, **New state table**, and pick **Count per
row** over `proposer`. You get a table with one row per proposer and a count that goes
up — and Studio writes it as a reducer you can read:

```ts
export const blocks_per_proposer = table({
  key:     { proposer: address },
  columns: {
    count:     u64.default(0),
    last_seen: u64.default(0),
  },
})

on(new_block_event, (r) => {
  const b = blocks_per_proposer.row(r.proposer)
  b.count += 1
  b.last_seen = tx.timestamp
})
```

Save it. Nineveh rebuilds the new table from the records it already has — no re-reading
the chain — and your old table keeps serving the whole time. Within seconds:

```sh
curl "$BASE/v1/tables/blocks_per_proposer?order=count.desc&limit=5"
```

That block is the whole of [Reducers](reducers.md), and it is where the rest of your
time goes.

## 7. What you just learned

- A **source** is chain data you follow. A **state table** is what you build from it.
- A **log** table keeps every record; a **reduce** table folds them into something
  smaller and more useful.
- Changing a reducer replays your stored records in seconds. Adding a *source* is the
  expensive one, because no history exists for something you never followed.
- Wide integers are strings. Parse them as `BigInt`.

Next: **[Reducers](reducers.md)**, to write the fold yourself rather than picking a
template.
