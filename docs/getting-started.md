# Getting started

Rows from an Aptos contract on a screen, in a React app, in about five minutes. One
package, three hooks. Everything after this page is the same three hooks with different
questions asked of them.

## What you need first

A Nineveh **project**: a backend that is already following a contract and has tables in
it. It gives you two things — a URL and a read key — and those are the only inputs on
this page.

Don't have one? Make one in [Studio](https://studio.nineveh.dev): paste a contract
address, tick what to follow, and it builds while you watch.
[Your first backend](first-backend.md) is that walk-through. If someone has handed you a
project's URL and key, you already have everything.

## 1. Install

From nothing at all:

```sh
npm create nineveh-app@latest
```

It asks for the URL and the key, asks the *project* what it holds — `GET /v1/tables`
reports every table's kind, key and typed columns — and writes a working page against
one of your real tables, with the types for all of them generated beside it.

Into an app you already have:

```sh
npm install @nineveh/react
```

## 2. Point it at your project

Wrap the app once:

```tsx
import { NinevehProvider } from "@nineveh/react";

export function App() {
  return (
    <NinevehProvider
      project="https://api.nineveh.dev/projects/myproject"
      apiKey={import.meta.env.VITE_NINEVEH_KEY}
    >
      <Leaderboard />
    </NinevehProvider>
  );
}
```

`project` is everything before `/v1`. A project served by `nineveh up` sits at
`/projects/<name>`; one served by `nineveh run --serve` sits at the root, and in local
mode over loopback it needs no key at all.

**The key reaches the browser, and that is intended.** Reads only ever touch your state
tables in Postgres, never the chain, so an exposed read key spends no stream credit. Give
each frontend its own and revoke it when it is done.

## 3. Read a table

```tsx
import { useTable } from "@nineveh/react";

function Leaderboard() {
  const { data, loading, error } = useTable("sellers", {
    order: ["revenue.desc", "seller"],
    limit: 10,
  });

  if (loading) return <p>Reading…</p>;
  if (error) return <p>{error.message}</p>;

  return (
    <ol>
      {data.map((row) => (
        <li key={row.seller}>
          {row.seller} — {row.revenue}
        </li>
      ))}
    </ol>
  );
}
```

That is the whole loop. `useTable` asks the REST API one question and hands back the
answer with the three states a fetch has. The query is the REST query and no wider:
`where`, `order`, `limit`, `offset`, `count`.

Two columns in `order` because one is rarely enough — a leaderboard on revenue has ties,
and which row wins a tie decides what page two shows.

## 4. Keep it current

Change one import and the same rows follow the chain:

```tsx
const { data, connected } = useLiveTable("sellers", {
  order: ["revenue.desc", "seller"],
  limit: 10,
});
```

`connected` is whether the change feed is actually open, so a "live" dot can be honest
rather than decorative. Every live hook on a page shares one connection.

A change tells the hook that something moved; it does not carry the new rows. On a
change they are read again, which is simpler and always right — a change can reorder
rows or push one out of a `limit: 10` window, and only the server knows which.

## 5. Make the compiler check your column names

```sh
npx nineveh-types https://api.nineveh.dev/projects/myproject --key nvk_…
# wrote src/nineveh-tables.d.ts: 5 tables, 25 columns
```

From here `useTable("sellres")` stops compiling, and so does multiplying a `u128` as if
it were a number. Nothing requires it — [Types](types.md) is what it does and why the
integer rule matters.

## Where to go next

- **[Hooks](react.md)** — all five, with the query language in full.
- **[Types](types.md)** — generated declarations, and the one rule about wide integers.
- **[Why Nineveh](guide.md)** — the model underneath: sources, reducers, state tables.
- **[Your first backend](first-backend.md)** — the other half, if the project is yours
  to build.
