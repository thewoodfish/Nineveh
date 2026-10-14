# @nineveh/react

React hooks for a [Nineveh](https://nineveh.dev) project: query your state tables, and
subscribe to them.

A Nineveh project turns a contract's events and resource changes into Postgres tables
that stay in sync with the chain, and serves them over REST and a change feed. This
package is a thin, typed wrapper over those two things — no new capability, just the
common cases made short. The HTTP contract underneath is
[`docs/reading.md`](https://nineveh.dev/docs/reading), and nothing here hides it.

```sh
npm install @nineveh/react
```

## Start

Wrap your app once with the project's URL and a key:

```tsx
import { NinevehProvider } from '@nineveh/react'

<NinevehProvider
  project="https://api.nineveh.dev/projects/myproject"
  apiKey={import.meta.env.VITE_NINEVEH_KEY}
>
  <App />
</NinevehProvider>
```

Then read a table:

```tsx
import { useLiveTable } from '@nineveh/react'

function Leaderboard() {
  const { data, loading, connected } = useLiveTable('sellers', {
    order: 'revenue.desc',
    limit: 10,
  })

  if (loading) return <p>Loading…</p>
  return (
    <table>
      <tbody>
        {data?.map((row) => (
          <tr key={String(row.seller)}>
            <td>{String(row.seller)}</td>
            <td>{String(row.revenue)}</td>
          </tr>
        ))}
      </tbody>
    </table>
  )
}
```

That table now updates itself. Nothing else to wire: `useLiveTable` holds the change
feed open and re-reads when a row it is showing moves.

**The key reaches the browser, and that is intended.** Reads only ever touch the state
tables in Postgres, never the chain, so an exposed read key costs you no stream credit.
Give a frontend its own key and revoke it when it is done.

## The hooks

### `useTable(table, query?)`

One page of rows, read once. `{ data, loading, error, count, refetch }`.

```tsx
const { data } = useTable('balances', { where: { owner: address }, limit: 50 })
```

`query` is the REST query, and it is exactly as wide as the API is:

| | |
| --- | --- |
| `where` | equality filters, AND-ed. **There is no `gt`/`lt`/`in`** — the API builds `WHERE col = val AND …` and nothing else. |
| `order` | one column: `'revenue'`, `'revenue.asc'`, `'revenue.desc'`. `_version` works too. One, not two. |
| `limit` | 50 by default, 1000 at most. A larger number is clamped, not refused. |
| `offset` | for paging. |
| `count` | `true` adds an exact `count(*)` over the filtered set. A second query — ask when you need it. |
| `raw` | appended to the query string verbatim, so this package can never be the reason something the API supports is out of reach. |

It re-reads when the table or the query changes, and a slower earlier read never
overwrites a later one. The query is compared by what it says, not by object identity,
so passing a fresh `{ where: { … } }` on every render is fine.

### `useLiveTable(table, query?, options?)`

The same, kept current: `{ …useTable, connected, lastChange }`.

The feed is used as the signal that something moved, not as the data — on a change the
rows are read again. That is simpler and always right, where merging a change into the
rows locally is not: a change can reorder rows, or push one out of a `limit: 10` window,
and only the server knows which. Changes are debounced (400ms by default, `debounceMs`)
so a burst costs one read.

`connected` is whether the feed is actually open, so a "live" dot can be honest. Every
live hook on a page shares **one** connection.

Two things are read again immediately rather than on the debounce, because in both the
rows on screen are not merely late but wrong:

- **A rebuild**, which is what a config change looks like. It arrives as a `reset`, and
  every row you listed came from the build that just went.
- **A reconnection.** The feed keeps seven days on the free tier, so a connection that
  comes back after a longer gap — a tab that was asleep — resumes at a position whose
  changes have been pruned. The replay is short by however much, and nothing in the
  feed says so, so the hook re-reads on every reopen.

That is why there is no polling fallback. If you want one anyway, `options.poll` takes
an interval in milliseconds; it is off by default.

### `useStatus(options?)`

How the project is doing: `{ status, behind, synced, progress, loading, error, refetch }`.

```tsx
const { behind, synced, progress } = useStatus()
// synced ? 'Live' : `${behind} versions behind` — and progress for a backfill bar
```

Polled, 5s by default (`interval`; `0` reads once). The change feed carries row
changes, not cursor movement, so there is nothing to subscribe to here.

### `useFlash(rows, key, ms?)`

The set of row keys whose `_version` just moved. A page that claims to be live should
look it:

```tsx
const flashing = useFlash(data, ['seller'])
…
<tr className={flashing.has(rowKey(row, ['seller'])) ? 'flash' : ''}>
```

`key` is the table's key columns, which `GET /v1/tables` reports in key order.

## Numbers are sometimes strings

Move integers do not fit JSON numbers. The API renders integers up to 32 bits as JSON
numbers and anything wider — `u64`, `u128`, `u256` — as decimal **strings**, because
`Number()` over a `u64` loses digits silently above 2^53.

So `row.revenue` may well be `'1200000000000'`. Display it as it arrived; use `BigInt`
if you need arithmetic. Nothing in this package converts values, and that is deliberate
— the one bug it will not introduce into your app is a wrong balance.

## Without React

`NinevehClient` is the whole transport and has no React in it:

```ts
import { NinevehClient } from '@nineveh/react'

const client = new NinevehClient({ project, apiKey })
const page = await client.rows('sellers', { order: 'revenue.desc', limit: 10 })
const stop = client.subscribe({ tables: ['sellers'], onChange: (c) => console.log(c) })
```

`fetch` and `eventSource` can both be passed in, for a server render, a test, or a
polyfill that puts the key in a header.

## What this does not do

No writes — a Nineveh table is a read-only projection of chain state. No cache beyond
each hook's own rows: at v1 sizes two components reading the same table is two reads,
and that is fine. No GraphQL yet.

Apache-2.0.
