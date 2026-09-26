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

The addresses they're published at are in `deployed.<network>.env`. For each one:

1. **New project**, the network you published to (**devnet** for the addresses in
   `deployed.devnet.env`), paste the address, **Inspect**.
2. Tick what to follow:
   - **counter**: `Incremented`, `Reset`, and the `Counter` resource.
   - **guestbook**: the events, and the `Guestbook.entries` table.
   - **market**: the events, and the `Market.listings` and `Market.credits` tables.
   - **arena**: `Played`, and the `Record` resource.
3. Choose **All of its history**. These contracts are new, so there's little to backfill, and
   a table source needs to see the write that created its table.
4. **Create**. Then run `./play.sh` (below) and watch rows arrive.

For the market, try the reducers too. Its project is written across two files, which is
the shape the docs teach: [`nineveh.yaml`](03-market/nineveh.yaml) says what to follow,
and [`market.nineveh.ts`](03-market/market.nineveh.ts) says what to do about it. Replace
`0xMARKET` in the YAML with the address, paste the two into the project's **Config** and
**Reducers**, and save. You get `sellers` and `buyers` tables with counts and totals that
nothing on chain keeps.

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
