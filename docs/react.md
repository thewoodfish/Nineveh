# Reading from React

[`docs/reading.md`](reading.md) is the HTTP contract — the REST API, the change feed,
webhooks. This page is the same thing from a React app, where `@nineveh/react` does the
fetching, holds the change feed open, and keeps the types honest.

Nothing here is a new capability. It is the common cases made short.

## From nothing to rows on a screen

```sh
npm create nineveh-app@latest
```

It asks for your project's URL and a read key, then asks the *project* what it holds —
`GET /v1/tables` reports every table's kind, key and typed columns — and writes a page
showing one of your tables, with the types for all of them generated beside it.

```sh
cd my-app && npm run dev
```

That's the whole quickstart. The rest of this page is what to do once you want a second
page.

## In an app you already have

```sh
npm install @nineveh/react
```

Wrap it once:

```tsx
import { NinevehProvider } from "@nineveh/react";

<NinevehProvider
  project="https://api.nineveh.dev/projects/myproject"
  apiKey={import.meta.env.VITE_NINEVEH_KEY}
>
  <App />
</NinevehProvider>
```

The `project` is everything before `/v1`. A project served by `nineveh up` is at
`/projects/<name>`; one served by `nineveh run --serve` is at the root, and in local mode
over loopback it needs no key at all.

**The key reaches the browser, and that is intended.** Reads only ever touch your state
tables in Postgres, never the chain, so an exposed read key spends no stream credit. Give
each frontend its own and revoke it when it's done.

## The hooks

### `useTable` — how are things

```tsx
const { data, loading, error, count, refetch } = useTable("balances", {
  where: { owner: address },
  limit: 50,
});
```

The query is the REST query and no wider: `where`, `order`, `limit`, `offset`, `count`.
A bare value in `where` is equality; an object compares, with `eq`, `neq`, `gt`, `gte`,
`lt`, `lte`:

```tsx
useTable("balances", { where: { balance: { gte: 100, lt: 500 } } });  // a range
useTable("sellers", { order: ["revenue.desc", "seller"], limit: 10 }); // a leaderboard
```

That operator list is the whole set, deliberately: each one is a single comparison
against a single column, which is what a database answers from an index on it. No
`like`, no `in`, no `or`, nothing spanning two columns — those belong in a
[reducer](reducers.md), computed once as the row is written rather than searched for on
every request.

Two columns in `order` because one is rarely enough: a leaderboard on revenue has ties,
and which row comes first inside a tie decides what page two shows.

### `useLiveTable` — tell me when they change

The same, kept current:

```tsx
const { data, connected, lastChange } = useLiveTable("sellers", {
  order: ["revenue.desc", "seller"],
  limit: 10,
});
```

The feed is the signal that something moved, not the data — on a change the rows are
read again. That is simpler and always right, where merging a change in locally is not:
a change can reorder rows, or push one out of a `limit: 10` window, and only the server
knows which. A burst of changes costs one read, and `connected` is whether the feed is
actually open, so a "live" dot can be honest.

Every live hook on a page shares **one** connection.

Two things make it re-read immediately rather than on the debounce, because in both the
rows on screen are wrong rather than merely late: a **rebuild**, which is what a config
change looks like and arrives as a `reset`, and a **reconnection**, because the feed
keeps seven days and a gap longer than that cannot be replayed. That is why there is no
polling fallback.

### `useChanges` — what has been happening

For the view that shows changes as the content rather than using them as a signal:

```tsx
const { changes } = useChanges({ tables: ["sales"], keep: 60 });
```

Changes are stored as they commit, not only broadcast, so this can open in the past:
`after` takes a `version.seq` position, or `'beginning'` for the whole stored feed.
`useFeedStart(rows)` works a position out from the rows you are showing, so the list
arrives full instead of filling up as you watch.

### `useStatus` — is it keeping up

```tsx
const { synced, behind, progress } = useStatus();
// synced ? "Live" : `${behind} versions behind`
```

Polled, not streamed: the change feed carries row changes, not cursor movement.

### `useFlash` — make live look live

```tsx
const flashing = useFlash(data, ["seller"]);
…
<tr className={flashing.has(row.seller) ? "flash" : undefined}>
```

The set of row keys whose `_version` just moved. `_version` is on every row and says
which transaction last wrote it, so there is no value-diffing to get wrong.

## Types for your own tables

```sh
npx nineveh-types https://api.nineveh.dev/projects/myproject --key nvk_…
# wrote src/nineveh-tables.d.ts: 5 tables, 25 columns
```

That file augments the package with your project's tables. It needs no importing — what
matters is that it sits inside your tsconfig's `include`, which is why it defaults to
`src/`. A declaration file the compiler never reads changes nothing and doesn't say so.

With it in place, the mistakes stop compiling:

```tsx
useTable("sellres")                       // error: did you mean 'sellers'?
useTable("sellers", { order: "revenu" })  // error: did you mean 'revenue'?
row.revenue * 2                           // error: revenue is a u128, so it's a string
row.event_index * 2                       // fine: event_index is a u32, so it's a number
```

That last pair is the point. **Integers up to 32 bits arrive as JSON numbers and
anything wider as decimal strings**, because `Number()` over a `u64` loses digits above
2⁵³ and does it silently. Which call to make per column is the one thing a hand-written
type gets wrong, and getting it wrong is invisible until an amount gets large.

Nothing requires the file. Without it every hook still works and rows are `Row`, whose
columns are `unknown` — generated types sharpen, they are not a dependency.

## Without React

`NinevehClient` is the whole transport and has no React in it:

```ts
const client = new NinevehClient({ project, apiKey });
const page = await client.rows("sellers", { order: "revenue.desc", limit: 10 });
const stop = client.subscribe({ tables: ["sellers"], onChange: (c) => console.log(c) });
```

`fetch` and `eventSource` can both be passed in, for a server render, a test, or a
polyfill that puts the key in a header. Node's own `EventSource` is behind
`--experimental-eventsource`, so on a server you generally hand one in.

## What this doesn't do

No writes: a Nineveh table is a read-only projection of chain state. No cache beyond
each hook's own rows — at these sizes two components reading one table is two reads, and
that is fine. No GraphQL yet.

Four worked examples, each one file:
[`clients/react/examples`](https://github.com/thewoodfish/Nineveh/blob/main/clients/react/examples/README.md).
