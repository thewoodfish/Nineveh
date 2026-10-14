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
| `where` | filters, AND-ed. A bare value is equality; an object compares: `{ revenue: { gte: 100, lt: 500 } }`. |
| `order` | `'revenue.desc'`, or `['revenue.desc', 'seller']` to break the ties in the first. Four columns at most. |
| `limit` | 50 by default, 1000 at most. A larger number is clamped, not refused. |
| `offset` | for paging. |
| `count` | `true` adds an exact `count(*)` over the filtered set. A second query — ask when you need it. |
| `raw` | appended to the query string verbatim, so this package can never be the reason something the API supports is out of reach. |

```tsx
// a leaderboard: the top ten by revenue, ties broken so page two is stable
useTable('sellers', { order: ['revenue.desc', 'seller'], limit: 10 })
// a range is two comparisons on the same column
useTable('balances', { where: { balance: { gte: 100, lt: 500 } } })
```

The operators are `eq`, `neq`, `gt`, `gte`, `lt`, `lte`, and that is the whole set.
Each is one comparison against one column, which is what a database can answer from an
index on it. **No `like`, no `in`, no `or`, nothing spanning two columns** — if you want
one of those, it belongs in a reducer, computed once as the row is written rather than
searched for on every request. A boolean column takes `eq` and `neq` only; a JSON column
cannot be filtered at all.

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

### `useChanges(options?)`

The feed itself, as a list: `{ changes, connected }`. For the view that shows changes
as the content rather than using them as a signal.

```tsx
const { changes } = useChanges({ tables: ['sellers'], keep: 60 })
…
{changes.map((c) => <li key={`${c.version}.${c.seq}`}>{c.op} {c.table} @{c.version}</li>)}
```

Newest first, `keep` of them (50 by default), deduplicated by position — the same
change can arrive twice when a connection resumes.

Changes are **stored as they commit**, not only broadcast, so this can open in the past:
`after` takes a `version.seq` position, or `'beginning'` for the whole stored feed,
which is unbounded and usually not what you want. Pass a position and the list arrives
full instead of filling up as you watch. Every hook still shares one connection — it
opens at the earliest point any of them asked for.

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

## Types for your own tables

`GET /v1/tables` already reports every table's kind, key and typed columns, so the
package can turn your project into TypeScript:

```sh
npx nineveh-types https://api.nineveh.dev/projects/myproject --key nvk_…
# wrote src/nineveh-tables.d.ts: 5 tables, 25 columns
```

That file augments the package with your tables. It needs no importing — what matters is
that it sits inside your tsconfig's `include`, which is why it defaults to `src/`. A
declaration file the compiler never reads changes nothing and says nothing about it.

With it in place:

```tsx
useTable('sellres')                       // error: did you mean 'sellers'?
useTable('sellers', { order: 'revenu' })  // error: did you mean 'revenue'?
row.revenue * 2                           // error: revenue is a u128, so it is a string
row.event_index * 2                       // fine: event_index is a u32, so it is a number
```

That last pair is the point. The string-versus-number call per column is the one thing a
hand-written type gets wrong, and getting it wrong is silent until a balance goes past
2^53.

Nothing requires it. Without the file every hook still works and rows are `Row`, whose
columns are `unknown` — generated types sharpen, they are not a dependency.

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
