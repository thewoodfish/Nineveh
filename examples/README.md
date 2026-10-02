# Example contracts

Four Move contracts to try Nineveh on, from the smallest thing it follows to the
patterns production contracts use. Each is published at an address of its own, so
pasting one into Studio's **New project** shows just that contract.

| | Contract | What it shows in Nineveh |
|---|---|---|
| 1 | [`counter`](01-counter/sources/counter.move) | The basics: an event becomes a log table, a resource at each account becomes a mirror table. |
| 2 | [`guestbook`](02-guestbook/sources/guestbook.move) | Strings and optional fields, and a `Table` whose items come and go: erased entries disappear from the mirror. |
| 3 | [`market`](03-market/sources/market.move) | A `SmartTable` of listings and a `Table` of balances, plus [`market.nineveh.ts`](03-market/market.nineveh.ts): reducers for the per-seller and per-buyer totals the contract never stores. It is the contract the [tutorial](../docs/first-backend.md) walks through. |
| 4 | [`arena`](04-arena/sources/arena.move) | Move 2 enums: a versioned event (`V1`, then `V2`), a resource that upgrades from `V1` to `V2` in place, and label enums inside them. |

## Try them in Studio

Publish them first (below). `./deploy.sh` writes the address of each one to
`deployed.<network>.env`, which is yours and isn't in git: these contracts live wherever
*you* put them. Then, for each one:

1. **New project**, the network you published to, paste the address, **Inspect**.
2. Tick what to follow:
   - **counter**: `Incremented`, `Reset`, and the `Counter` resource.
   - **guestbook**: the events, and the `Guestbook.entries` table.
   - **market**: the events, and the `Market.listings` and `Market.credits` tables.
   - **arena**: `Played`, and the `Record` resource.
3. Choose **All of its history**. A table source has to start early enough to see the
   write that created its table, and a contract you published minutes ago has almost no
   history to read, so this costs nothing here.

   **Publish them the same day you follow them.** The free tier starts a project within
   six hours of the chain's tip — deep backfills tie up shared catch-up capacity, so
   they're a paid thing ([limits](../docs/running.md#3-limits)). Past that window
   Studio refuses the project outright: *"The Free tier starts a project within 6 hours
   of the chain's tip."* Publish again and use the new address — on devnet that's the
   normal state of affairs anyway, since it's reset about weekly.
4. **Create**. Then run `./play.sh` (below) and watch rows arrive.

For the market, try the reducers too. Everything so far is a copy of something the
contract already holds; `sellers` and `buyers` are the numbers it never keeps, because a
running total per account would cost gas on every trade.

Open **New state table** and, instead of answering the questions, paste
[`market.nineveh.ts`](03-market/market.nineveh.ts) over what's in the editor — that pane
is the project's whole reducers file, not one table, so a file declaring two of them
makes two. Save, and you have per-seller revenue and per-buyer spend folded out of events
the contract was emitting anyway.

Nothing to replace in it: the file names sources, not addresses. Those you ticked in
step 2, and Studio wrote them into the project's config for you.

## Publish them yourself

You need the [Aptos CLI](https://aptos.dev/tools/aptos-cli/) (`brew install aptos`).

```sh
./setup.sh              # two accounts; fund any it can't at the faucet link it prints
./deploy.sh             # publishes all four, each at its own object address
./deploy.sh 03-market   # or just one of them
./play.sh               # keeps whatever you published busy, until Ctrl-C
```

They default to testnet, whose faucet only works through its web page. Devnet funds
accounts over its API, so everything runs unattended there, and devnet is reset about
once a week:

```sh
NETWORK=devnet ./setup.sh && NETWORK=devnet ./deploy.sh && NETWORK=devnet ./play.sh
```

Set `NODE_API_KEY` to a [Geomi](https://geomi.dev) key for the network you're using:
without one these calls share the anonymous per-IP rate limit, and start failing.

Devnet is reset about once a week, and everything published there goes with it: run
`deploy.sh` again (after deleting `deployed.devnet.env`) to publish afresh.

Keys live in `.aptos/config.yaml` here, which git ignores. Each contract has unit
tests: `aptos move test --package-dir 01-counter --dev`.
