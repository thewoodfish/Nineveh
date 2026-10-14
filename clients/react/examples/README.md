# @nineveh/react examples

Four pages, one project, one file each. They are the package's spec as much as its
demonstration: if one of these reads badly, the API is wrong and the API gets fixed.

| File | What it shows |
| --- | --- |
| [`leaderboard.tsx`](src/leaderboard.tsx) | `useLiveTable` with a **comparison filter** and a **two-column order** — a leaderboard whose ties don't reshuffle between pages. |
| [`holders.tsx`](src/holders.tsx) | Sorting from React state, and `useFlash` so a balance that changes is something you see. |
| [`feed.tsx`](src/feed.tsx) | A log table ordered by `_version` — rows only ever arrive, so the page ticks. |
| [`dashboard.tsx`](src/dashboard.tsx) | Two live tables sharing one connection, plus `useStatus` for the cursor and how far behind the chain it is. |

Everything else is scaffolding: [`shared.tsx`](src/shared.tsx) holds the panel, the
address shortener and a `BigInt` formatter, so each example is only the thing it is
showing. Nothing in `shared.tsx` is specific to Nineveh.

Two more files earn their place. [`nineveh-tables.d.ts`](src/nineveh-tables.d.ts) is
generated — `npx nineveh-types http://127.0.0.1:4000` against the market project below —
and it is why `row.revenue` is a `string` here rather than `unknown`, and why a mistyped
table name does not compile. [`types-check.ts`](src/types-check.ts) is that claim written
as a test the compiler runs: every `@ts-expect-error` in it fails the build if the line it
marks stops being an error.

## Run them

```sh
cd ../ && npm install && npm run build   # the SDK: these import its built dist/
cd examples && npm install
npm run dev                              # http://localhost:5173
```

That first line is not optional. `@nineveh/react` is a `file:..` dependency, so npm
symlinks the package and resolves it to the `dist/` that `npm run build` writes — and
`dist/` is generated and untracked, so a fresh clone has none. Rebuild it after changing
the SDK too: these import the built files, not the sources.

The base URL is everything before `/v1`, and which it is depends on how the project is
served:

| | Base URL |
| --- | --- |
| `nineveh run --serve` | `http://127.0.0.1:4000` — one project, at the root |
| `nineveh up` | `http://127.0.0.1:4000/projects/market` — many, each under its name |
| hosted | `https://api.nineveh.dev/projects/market` |

The default is the first, and loopback in local mode needs no key, so for that leave
both variables unset. Otherwise copy `.env.example` to `.env` and paste the URL and a
read key.

## The project they expect

A project over [`examples/03-market`](../../../examples/03-market), which is the
contract the [tutorial](../../../docs/first-backend.md) walks through. These four read
four of its tables:

| Table | Where it comes from |
| --- | --- |
| `sellers`, `buyers` | reducers in [`market.nineveh.ts`](../../../examples/03-market/market.nineveh.ts) — totals the contract never stores |
| `balances` | a mirror of the contract's `Table<address, u64>` of credits |
| `sales` | a log of its `Sold` event |

`balances` has columns `handle`, `key` and `value` rather than anything friendlier: it
mirrors a Move table whose value is a `u64`, and a value that isn't a struct has nowhere
to put a name. That is why `holders.tsx` labels them in the markup instead.

To get that project: publish the contract with
[`examples/deploy.sh`](../../../examples/deploy.sh), point a project at it, and keep it
busy with [`examples/play.sh`](../../../examples/play.sh). Both need the
[Aptos CLI](https://aptos.dev/tools/aptos-cli/), and running the project needs a
[Geomi](https://geomi.dev) key **for that network** — keys are per-network, so a key
that works on mainnet is refused on devnet.

Any other project works too, with the table and column names changed. There is nothing
market-specific in the package.
