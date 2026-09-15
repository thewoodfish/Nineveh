# Nineveh Studio

The dashboard for a Nineveh project: processor health, state tables that update live,
the change feed, and an API playground. Next.js, React, Tailwind and TypeScript.

Studio talks to the API that `nineveh run --serve` (or `nineveh serve`) serves on
`http://127.0.0.1:4000`. It has no backend of its own.

```sh
# In your project directory: build the state and serve it.
nineveh run --serve

# Here: the dashboard on http://localhost:3000.
npm install
npm run dev
```

Point it elsewhere with `NEXT_PUBLIC_NINEVEH_API=http://host:port npm run dev`.

Wide integers (u64 and up, and versions) arrive from the API as decimal strings and stay
exact: Studio formats them with `BigInt`, never `Number`.

`npm run typecheck` and `npm run build` are what CI runs.
