# Your first backend

Publish a contract, point Nineveh at it, and watch your own transactions turn into
tables you can query. Follow it top to bottom; every step produces something you can
see.

You need the [Aptos CLI](https://aptos.dev/tools/aptos-cli/) and a GitHub account.

## 1. Publish a contract to play with

You could point Nineveh at a contract that is already out there, but then the first
thing you see is somebody else's data, and you can't tell a mistake of yours from a
quiet afternoon on chain. It's better to own both ends. So publish a small contract,
drive it yourself, and watch what happens in between.

`market` is a marketplace, the third of four example contracts in the repo. Sellers list
an item for a price in the market's own credits, buyers buy it, sellers take down what
doesn't sell, and the market keeps 2.5% of every sale. Open listings live in a
`SmartTable` and balances in a `Table`, which matters later. It is
[two hundred lines of Move](https://github.com/thewoodfish/Nineveh/blob/main/examples/03-market/sources/market.move)
and you don't have to read any of them yet.

```sh
git clone https://github.com/thewoodfish/Nineveh.git
cd Nineveh/examples

export NETWORK=devnet
./setup.sh                 # two accounts, funded from the faucet
./deploy.sh 03-market      # publishes it at an object address of its own
```

Devnet because its faucet funds accounts over its API, so `setup.sh` finishes without
stopping to ask you for anything. Testnet works identically if you'd rather, but its
faucet is a web page, so `setup.sh` prints a link and waits for you.

`deploy.sh` prints the address you need:

```text
==> 03-market
    market is at 0x35c1f01bf1f6187df158d342b4e41f66d7747fb393b9841ded0457d0fb44adfa
```

Yours will be a different one: the contract goes to an address of its own, derived from
the account that published it. Copy what your terminal prints, not what is printed here.
Leave it open; you'll want that window again in §3.

Devnet is wiped about once a week, and everything published there goes with it. Run
`./deploy.sh 03-market` again and it notices the address has nothing at it any more and
republishes; you then point a new project at the new address.

## 2. Create the project

Open Studio at <https://studio.nineveh.dev> and sign in with GitHub.

**New project** → network **devnet** → paste the address → **Inspect**.

Nineveh reads the contract off the chain and lists everything it could follow: the
events it emits, the resources it stores, and the tables inside those resources. The
market is small enough to see all of it at once. Tick four things.

Under **Events**:

- `Listed`: someone put an item up for sale.
- `Sold`: someone bought one.
- `Cancelled`: a seller took theirs down.

Under **Tables**:

- `Market.listings`: what is for sale right now.

That last one is not an event, and it is the interesting one. Keep reading in §3 to see
why it behaves differently from the other three.

Name the project `market` and choose **All of its history** rather than **From now on**.
You published this contract two minutes ago, so its whole history is almost nothing, and
a table source needs to start early enough to see the write that created its table.

**Create backend with 4 tables.**

## 3. Make something happen

The Overview shows a cursor climbing, the chain head it's chasing, and the gap between
them. Within a few seconds it reads *following the chain*.

And every table is empty, which is correct. You published a contract and nobody has used
it. Nothing has happened yet.

So make something happen. Back in the terminal:

```sh
./play.sh
```

Two accounts start trading: listing items, buying each other's, cancelling some. It
prints a line per transaction and runs until you stop it with Ctrl-C.

Leave Studio open on the project while it runs. Rows arrive in the order the chain
commits them, a few seconds behind the transaction you just watched go out. Watch two
tables in particular:

- **`sold`** only grows. It is a log: one row per sale, kept forever, in order.
- **`market_listings`** grows *and shrinks*. A row appears when something is listed and
  disappears when it sells or is cancelled.

You could have worked that out from the events alone: fold `Listed`, then take away
everything `Sold` and `Cancelled` mention. The mirror doesn't have to. The contract
removed the entry from its own `SmartTable`, and that removal is itself a record Nineveh
followed, so the table knows what is open without anyone deriving it.

That is why resources and tables are sources in their own right and not a second-class
thing. Events tell you what happened; resources and tables tell you what is true now.
Which of the two carries the state you need is the contract author's decision, not yours,
and because on-chain storage costs gas, plenty of contracts keep their real state in
resources and barely emit events at all. Following only events would leave you guessing
at those.

You now have a backend. It has a URL:

```text
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

In a third terminal:

```sh
curl -N "$BASE/v1/changes"
```

Every row change is pushed as it commits, and with `play.sh` still running you are
watching your own transactions come back to you:

```text
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

Every table so far is a copy of something the chain already had: a log of records as
they arrived, or a mirror of what a table holds right now. That is useful, but it isn't
what a marketplace's front page needs. That needs *who sells the most*, and no amount of
querying a log of sales gives you a row per seller.

The contract can't tell you either. It knows a sale happened and then forgets. Keeping a
running total per seller would mean an extra write on every trade, and writes cost gas,
so almost no contract keeps one. The information is all there in the events you are
already following. Nobody has added it up.

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

Read it before saving it. `sold` is the source you ticked in §2. The handler says what
one sale does: find that seller's row, add the price to a running total, count the sale.
`u128` because a `u64` column adding up `u64` prices overflows eventually, and overflow
halts a project rather than wrapping quietly.

It is also not quite right. `total_price` is what buyers paid, and the market keeps 2.5%
of that, so it isn't what the seller got. Studio had no way to know: the fee is sitting
right there in the event next to the price, and only you know what it means. Change
three lines.

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

You have already done every step: paste an address, tick what to follow, fold it into
the table you want. Two things are different about a contract you didn't publish four
minutes ago.

**A source that matches nothing is not an error.** If you tick a type that never
arrives, you get a project that runs perfectly and stays empty. Nothing fails and
nothing complains, because that is also what a contract nobody is using looks like.
Studio tells you once it has read far enough to be sure: *"this source hasn't matched
anything yet"*. Until then, check the type name twice. Near-identical names in one
module are the commonest way to lose a morning.

**History is the expensive axis, not tables.** You chose **All of its history** in §2
and it cost nothing, because there wasn't any. A contract that has been live for months
is a real backfill, and Nineveh reads every version of the chain in the range, not just
yours. If you only need data from today, start **From now on**, and accept that a
`table:` source will not know which table is yours until the resource holding it is
written again. Following the whole history is what makes that certain.

And if your contract is quiet, make it busy before you judge what you built.
[`play.sh`](https://github.com/thewoodfish/Nineveh/blob/main/examples/play.sh) does
nothing cleverer than sending transactions in a loop; it is under a hundred lines and
most of them are picking what to send.

## 8. What you just learned

- A **source** is chain data you follow: events, resources, and the tables inside them.
  A **state table** is what you build from it.
- Events say what happened; resources and tables say what is true now. `market_listings`
  shrinking is the difference, and most contracts need you to follow both.
- A **log** table keeps every record; a **reduce** table folds them into something
  smaller and more useful. The useful one is almost always a number the contract itself
  never stores, because storing it would have cost gas.
- Changing a reducer replays your stored records rather than the chain: minutes for a
  project with real history, not another backfill. Adding a *source* is the expensive
  one, because no history exists for something you never followed.
- Wide integers are strings. Parse them as `BigInt`.

Next: **[Reducers](reducers.md)**, to write the fold yourself rather than starting from
a template.
