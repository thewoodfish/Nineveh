# Getting started

Rows from an Aptos contract on a screen, in a React app. One command scaffolds a
working one against your own tables; the rest of this page is the same thing by hand,
in an app you already have.

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
kind — but because reading is all the API does: it lists your tables and returns their
rows, and there is nothing else to ask it for. Your tables are written inside the
backend, by [reducers](reducers.md), and nothing out here can touch them.

Don't have a project? Make one in [Studio](https://studio.nineveh.dev): paste a contract
address, tick what to follow, and it builds while you watch.
[Your first backend](first-backend.md) is that walk-through. If someone has handed you
a URL and a key, you already have everything.

## 1. Scaffold it

```sh
npm create nineveh-app@latest
```

It asks for a folder, the URL and the key, then asks the project what tables it holds
and writes an app around one of them:

```text
leaderboard/
├─ src/
│  ├─ App.tsx                 a live table, built from your real columns
│  ├─ main.tsx                the provider, reading .env
│  ├─ nineveh-tables.d.ts     generated: every table, every column, typed
│  └─ styles.css              yours to replace
├─ .env                       your URL and key, already gitignored
└─ package.json               vite, react, @nineveh/react
```

It installs and generates the types for you, so:

```sh
cd leaderboard && npm run dev
```

## 2. What you just made

A working Aptos frontend. Not a placeholder with sample data in it — the table on screen
is one of yours, the columns are the ones your contract actually writes, and it is live:
when the chain changes a row, the row changes and flashes.

From here it is an ordinary React app, and the two things worth doing first are both in
`src/App.tsx`:

- **Show a different table.** Change the name in `useLiveTable` — the generated types
  will tell you which ones exist, and which columns each has.
- **Show something the contract never stored.** A total, a count, a leaderboard. That is
  a [reducer](reducers.md), written in Studio; the table it produces reads from here
  exactly like any other.

The rest of this page is the same thing done by hand, for an app you already have.

## 3. Point it at your project

In an app of your own, install the package:

```sh
npm install @nineveh/react
```

and wrap it once:

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
the rest themselves.

**The key reaches the browser, and that is intended** — it can only read, so there is
nothing for it to break. Make each frontend its own in Studio and revoke it when that
frontend is done; revoking one leaves the rest working.

## 4. Read a table

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

## 5. Keep it current

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

## 6. Let the compiler check your column names

The scaffolder did this for you; in your own app it is one command:

```sh
npx nineveh-types https://api.nineveh.dev/projects/myproject --key nvk_…
# wrote src/nineveh-tables.d.ts: 5 tables, 25 columns
```

From here `useTable("sellres")` stops compiling, and so does multiplying a `u128` as if
it were a number. Nothing requires it — [Types](types.md) is what it does, and why the
rule about wide integers is the part that matters.

## Where to go next

- **[Hooks](react.md)** — all five, with the query language in full.
- **[Types](types.md)** — generated declarations, and the one rule about wide integers.
- **[Why Nineveh](guide.md)** — the model underneath: sources, reducers, state tables.
- **[Your first backend](first-backend.md)** — the other half, if the project is yours
  to build.
