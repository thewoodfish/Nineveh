# Nineveh Studio

The dashboard for Nineveh. Create a backend from a contract address, then watch it:
processor health, state tables that update live, the change feed, and an API
playground. Next.js, React, Tailwind and TypeScript.

Studio talks to the control plane, `nineveh up`, on `http://127.0.0.1:4000`. It has no
backend of its own.

```sh
# Anywhere: the control plane, with a Geomi key and a Postgres.
APTOS_API_KEY=… NINEVEH_DATABASE_URL=postgres:///nineveh nineveh up

# Here: the dashboard on http://localhost:3000. New project → paste an address.
npm install
npm run dev
```

Studio also works against one project served by `nineveh run --serve` or `nineveh serve`
(no control API): it shows that project, and can't create others.

Point it elsewhere with `NEXT_PUBLIC_NINEVEH_API=http://host:port npm run dev`.

The change feed is shared per project and delivered in batches a few times a second
(`lib/feed.ts`): a busy contract commits hundreds of changes a second.

Wide integers (u64 and up, and versions) arrive from the API as decimal strings and stay
exact: Studio formats them with `BigInt`, never `Number`.

`npm run typecheck` and `npm run build` are what CI runs.
