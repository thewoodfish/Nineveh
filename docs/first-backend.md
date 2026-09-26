# Your first backend

Build a working backend against a live contract, from nothing to querying real data.
Follow it top to bottom; every step produces something you can see.

You need a GitHub account. Nothing to install.

## 1. A contract that is already busy

The hardest part of a first project is not knowing whether an empty table means *you
got it wrong* or *the contract had nothing to say*. Most contracts on testnet are quiet:
someone published one and poked it three times by hand. So start with one that never is.

`market` is a small marketplace published on testnet, with two accounts trading in it
around the clock. Sellers list an item for a price, buyers buy it, sellers take down
what doesn't sell, and the market keeps 2.5% of every sale. It is
[two hundred lines of Move](https://github.com/thewoodfish/Nineveh/blob/main/examples/03-market/sources/market.move),
and you don't have to read any of them. You are going to point Nineveh at its address,
which is what you will do with your own contract in §7.

It is published at:

```
0xMARKET
```

Open Studio at <https://studio.nineveh.dev> and sign in with GitHub.

## 2. Create the project

**New project** → network **testnet** → the address above → **Inspect**.

Nineveh reads the contract and lists everything it could follow: the events it emits,
the resources it stores, and the tables inside them. The market is small enough to see
all of it at once.

Under **Events**, tick three:

- `Listed`: someone put an item up for sale.
- `Sold`: someone bought one.
- `Cancelled`: a seller took theirs down.

Leave the rest alone. Name the project `market`, and choose **From now on** rather than
the contract's whole history: starting at the chain's current version means data in
seconds instead of a backfill.

**Create backend with 3 tables.**

## 3. Watch it fill

The Overview shows a cursor climbing, the chain head it's chasing, and the gap between
them. Within a few seconds it reads *following the chain*, and rows start arriving:
a `listed` row each time something goes up for sale, a `sold` row each time one is
bought, a few of each a minute.

You now have a backend. It has a URL:

```
https://api.nineveh.dev/projects/market
```

## 4. Query it

```sh
BASE=https://api.nineveh.dev/projects/market

curl $BASE/v1/tables                       # what tables exist, and their columns
curl "$BASE/v1/tables/sold?limit=3"
```

Then something more specific: the ten priciest sales, dearest first.

```sh
curl "$BASE/v1/tables/sold?limit=10&order=price.desc"
```

Look closely at `price` in the response:

```json
{ "id": "412", "seller": "0x9f3a…", "item": "kettle", "price": "640", "fee": "16" }
```

It's a **string**, not a number. `price` is a Move `u64`, which goes up to 18
quintillion, and JavaScript numbers stop being exact at 2⁵³. Nineveh returns wide
integers as strings so nothing is silently rounded, which means in your app:

```js
const price = BigInt(row.price)   // right
const price = Number(row.price)   // wrong above 9 quadrillion, silently
```

The market's prices are small enough to get away with it. A contract that moves real
amounts is not, and the type is the same either way.

## 5. Watch changes arrive

In a second terminal:

```sh
curl -N "$BASE/v1/changes"
```

Every row change is pushed as it commits:

```
event: change
id: 11292175483.0
data: {"version":"11292175483","table":"sold","op":"insert","key":{…},"row":{…}}
```

Kill it, wait a moment, then resume from where you stopped:

```sh
curl -N "$BASE/v1/changes?after=11292175483.0"
```

Nothing is missed. That `id` is how a browser resumes too: `EventSource` sends it
automatically as `Last-Event-ID`.

## 6. Build the table the contract doesn't have

So far every table is a log: one row per record, exactly as it arrived. That is useful,
but it isn't what a marketplace's front page needs. That needs *who sells the most*,
and no amount of querying a log of sales gives you a row per seller.

The contract can't tell you either. It knows a sale happened and then forgets: keeping a
running total per seller would mean a write on every trade, and writes cost gas, so
almost no contract keeps one. The information is all in the events. Nobody has added it
up.

That is what a reducer is for. In Studio, **New state table**, then:

- **Fold records from** `sold`
- **One row per** `seller`
- **Adding up** `price`

Pick **Total per row**. Studio writes it as a reducer you can read:

```ts
export const price_per_seller = table({
  key:     { seller: address },
  columns: {
    total_price: u128.default(0),
    count:       u64.default(0),
  },
})

on(sold, (r) => {
  const b = price_per_seller.row(r.seller)
  b.total_price += u128(r.price)
  b.count += 1
})
```

Read it once before saving it. `sold` is the source you ticked in §2. The handler says
what one sale does: find that seller's row, add the price to a running total, count the
sale. `u128` because a `u64` column adding up `u64` prices overflows eventually, and
overflow halts a project rather than wrapping quietly.

It is also not quite right. `total_price` is what buyers paid, and the market keeps 2.5%
of that, so it isn't what the seller got. Studio had no way to know that; the fee is in
the event, sitting next to the price, and only you know what it means. Change three
lines:

```ts
export const sellers = table({
  key:     { seller: address },
  columns: {
    sold:    u64.default(0),
    revenue: u128.default(0),
  },
})

on(sold, (s) => {
  const b = sellers.row(s.seller)
  b.sold    += 1
  b.revenue += u128(s.price - s.fee)
})
```

That gap is the whole reason reducers exist. Anything can hand you the records. The
number your product actually shows is usually one piece of arithmetic away from them,
and that piece is yours.

Save it. Nineveh rebuilds the new table from the records it already has, without
re-reading the chain, and the old table answers reads throughout, frozen where it had
reached until the new one swaps in. This project holds minutes of history, so that takes
seconds:

```sh
curl "$BASE/v1/tables/sellers?order=revenue.desc&limit=5"
```

That block is the whole of [Reducers](reducers.md), and it is where the rest of your
time goes. The market's finished version, with a `buyers` table beside this one, is
[`market.nineveh.ts`](https://github.com/thewoodfish/Nineveh/blob/main/examples/03-market/market.nineveh.ts)
in the repo.

## 7. Now point it at your own contract

Same three steps: paste the address, tick what to follow, fold it into the tables you
want. Two things are different about your contract, and both are worth knowing before
you spend an afternoon on them.

**A source that matches nothing is not an error.** If you tick a type that never
arrives, you get a project that runs perfectly and stays empty. Nothing fails and
nothing complains, because that is also what a quiet contract looks like. Studio tells
you once it has read far enough to be sure: *"this source hasn't matched anything
yet"*. Until then, check the type name twice. Near-identical names in one module are
the commonest way to lose a morning.

**Events are usually not the whole story.** On-chain storage costs gas, so contracts
emit events for what happened and keep current state in resources and tables. If the
state your app needs is a resource, or the items of a `Table` or `SmartTable`, tick
those: they are sources exactly like events are, and following events alone will
quietly under-cover your data.

One catch if you follow a table. Nineveh works out which table is yours from a write to
the resource that holds it, so it has to start early enough to see one. Choose **All of
its history** rather than **From now on** when a table source is in the list. For a
contract you published recently, that costs very little anyway.

And if your contract is quiet, make it busy before you judge what you built. The market
is kept moving by a
[shell script](https://github.com/thewoodfish/Nineveh/blob/main/examples/play.sh)
doing nothing cleverer than sending transactions in a loop.

## 8. What you just learned

- A **source** is chain data you follow. A **state table** is what you build from it.
- A **log** table keeps every record; a **reduce** table folds them into something
  smaller and more useful. The useful one is almost always a number the contract itself
  never stores.
- Changing a reducer replays your stored records rather than the chain: minutes for a
  project with real history, not another backfill. Adding a *source* is the expensive
  one, because no history exists for something you never followed.
- Wide integers are strings. Parse them as `BigInt`.

Next: **[Reducers](reducers.md)**, to write the fold yourself rather than starting from
a template.
