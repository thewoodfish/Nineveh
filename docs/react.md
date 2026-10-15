# Reading from React

`@nineveh/react` does the fetching, holds the change feed open, and keeps the types
honest. Five hooks, and the query language they share.

Nothing here is a new capability. Underneath, every one of these is a request to
[the REST API and the change feed](reading.md), made short — and
[Getting started](getting-started.md) is the one that gets the first rows onto a screen.

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
// a range
useTable("balances", { where: { balance: { gte: 100, lt: 500 } } });

// a leaderboard
useTable("sellers", { order: ["revenue.desc", "seller"], limit: 10 });
```

That list is the whole set, deliberately: every one of them is a single comparison
against a single column, which is the kind of question that stays fast as a table grows.
No `like`, no `in`, no `or`, nothing spanning two columns — those belong in a
[reducer](reducers.md), worked out once as the row is written rather than searched for
on every request.

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

Changes are batched a few times a second. Two things skip the wait and re-read at once,
because after either one the rows on screen are wrong rather than merely late: a
**rebuild**, which is what changing your project's config looks like from here, and a
**reconnection**, because a gap in the feed can be longer than the feed remembers.

### `useChanges` — what has been happening

For the view that shows changes as the content rather than using them as a signal:

```tsx
const { changes } = useChanges({ tables: ["sales"], keep: 60 });
```

Changes are kept, not only broadcast, so this can start in the past rather than from
the moment the page loaded. `after` takes a position in the feed, or `'beginning'` for
all of it. `useFeedStart(rows)` works a position out from the rows you are already
showing, so the list arrives full instead of filling up while you watch.

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

## Typing your own tables

Without generated types every hook still works, and rows are `Row`, whose columns are
`unknown`. With them, a mistyped table or column name stops compiling and every column
arrives as the right type. One command does it: [Types](types.md).

## Without React

`NinevehClient` is the whole transport and has no React in it:

```ts
const client = new NinevehClient({ project, apiKey });

const page = await client.rows("sellers", {
  order: "revenue.desc",
  limit: 10,
});

const stop = client.subscribe({
  tables: ["sellers"],
  onChange: (change) => console.log(change),
});
```

`fetch` and `eventSource` can both be passed in, for a server render, a test, or a
polyfill that puts the key in a header. Node's own `EventSource` is behind
`--experimental-eventsource`, so on a server you generally hand one in.

## What this doesn't do

No writes — your tables are built inside the backend, and nothing out here can change
them. No cache beyond each hook's own rows: at these sizes two components reading one
table is two reads, and that is fine. No GraphQL yet.

There are [four worked examples](https://github.com/thewoodfish/Nineveh/blob/main/clients/react/examples/README.md),
each one file.
