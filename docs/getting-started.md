# Getting started

Rows from an Aptos contract on a screen, in a React app, in about five minutes. One
package, three hooks. Everything after this page is the same three hooks with different
questions asked of them.

## What you need first

A Nineveh **project**: a backend that is already following a contract and has tables in
it. It gives you the two inputs on this page, and they are the only two.

**1. The project's API URL** — the address its tables are served from. It is everything
before `/v1`:

```text
https://api.nineveh.dev/projects/myproject
```

Everything hangs off it: `/v1/tables` lists your tables — which is exactly what the
hooks read — `/v1/tables/<name>` reads one, and `/v1/changes` is the live feed. Studio's
Overview prints it under *Your API*, without any of those paths on the end, because this
is the part you paste.

**2. An API key for that project** — created in Studio under Settings, and shown once:

```text
nvk_a1b2c3d4e5f6…
```

**The key can only read.** Not because it is a restricted kind of key — there is one
kind — but because the project's API is three `GET` routes and nothing else: the status,
the list of tables, and the rows of one table. There is no write to authorise. A Nineveh
table is a read-only projection of chain state, and the only thing that ever writes one
is a reducer, inside the backend.

Don't have a project? Make one in [Studio](https://studio.nineveh.dev): paste a contract
address, tick what to follow, and it builds while you watch.
[Your first backend](first-backend.md) is that walk-through. If someone has handed you
a URL and a key, you already have everything.

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

`project` is the API URL from above — everything before `/v1`, because the hooks append
the rest themselves. A project served by `nineveh up` sits at `/projects/<name>`; one
served by `nineveh run --serve` sits at the root, and in local mode over loopback it
needs no key at all.

**The key reaches the browser, and that is intended.** It can only read, and reads only
ever touch your state tables in Postgres — never the chain — so an exposed key spends no
stream credit and can change nothing. Give each frontend its own and revoke it when it
is done; revoking one leaves the others working, which is the reason to do it that way.

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
