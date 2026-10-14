# Plan — the client path

The next milestone, proposed for review. One sentence: **a developer who has never
spoken to me gets their own contract's state onto a live React page, and I am not in
the loop.**

`react-spec.md` holds the API shape this assumes, and is a sibling to this document
rather than an input to it: the package is ours to build.

## Why this, now

Two adoption bets are in flight — the outreach post, which ends "point it at your own
contract", and a design-partner demo. Both dead-end in the same place.

Today the path is: create a project in Studio, pick sources, get rows, mint a key — and
then nothing. The only way to put that state on a screen is to read `docs/reading.md`
and rewrite `demos/echelon-fe/src/api.ts` by hand. That file is 33 lines and it is the
entire client story we ship. Every reader of the post who gets as far as liking the idea
hits a blank editor, and the thing they are trying to build is a frontend.

The backend half is done and hosted. The client half is a weekend of TypeScript that
nobody has done. That asymmetry is the whole argument for this being next.

## Exit criterion

Testable on a stranger, which is the point:

```
npm create nineveh-app@latest my-app     # clone
<paste project URL + key>                # configure
npm run dev                              # a live-updating table of their own rows
```

Under ten minutes, no Rust, no questions asked of me. Anything that does not move that
sentence forward is not in this milestone.

## Deliverables, in dependency order

### 1. `@nineveh/react`

The three primitives from `react-spec.md`, unchanged in spirit: `NinevehProvider`,
`useTable`, `useLiveTable`, plus `useStatus()` for cursor/lag.

Already written, in prototype form, in `demos/echelon-fe/src`:

- `api.ts` — REST + `EventSource` transport, complete.
- `useLive.ts` — list, open the feed just before the oldest row on screen so it replays
  what you are looking at, re-read on change, poll on a 15s floor.
- `useFlash.ts` — per-row change highlight. This belongs in the package; it is what
  makes "live" read as live, and every consumer would otherwise rewrite it.

Generalising those three from "two hard-coded tables" to "any table" *is* the SDK.

Done when: the echelon demo is rewritten on top of the package and loses code.

### 2. Generated types

`GET /v1/tables` already returns every table's `name`, `kind`, `key` and typed columns
(`crates/nineveh-api/src/tables.rs`). A small CLI — `nineveh types` or a `npx` shim —
turns that into a `.d.ts`: table names as a union, row shapes per table, and the right
`string`-vs-`number` call per column, which is the one thing a hand-written type gets
wrong (ADR 0008: ≤32-bit integers are JSON numbers, wider ones decimal strings).

No Rust needed beyond wiring the subcommand. It is also what turns the package from
"a fetch wrapper" into a reason to install it. The `nineveh.d.ts` the DSL already
generates is the precedent to copy, including where the file lands.

Done when: a wrong column name or a `u64` treated as a number is a type error.

### 3. `create-nineveh-app`

`react-spec.md` filed this as a stretch. It should be in scope and it is arguably the
most valuable single item: it is the only deliverable that compresses ten minutes to
two, and the only one that fits in a chat message as a one-liner.

Vite + React 19 + the SDK + one live table + the flash, prompting for base URL and key
on first run. Deliberately not a framework — a starting file people delete half of.

Done when: `npm create` on a clean machine renders rows from a pasted key.

### 4. Four examples, treated as the spec

From the spec, and keeping its rule: **if an example is clunky to write, the API is
wrong and the API gets fixed.**

| Example | Hook | Backend |
| --- | --- | --- |
| Leaderboard | `useTable('sellers', { order: 'revenue.desc' })` | `examples/03-market` — `sellers`/`buyers` already exist in `market.nineveh.ts` |
| Holders | sortable balances | `examples/01-counter` / market `credits` |
| Live feed | `useLiveTable('posts')`, visibly ticking | `examples/02-guestbook` |
| Dashboard | two tables + `useStatus()` | any of them |

Each 30–50 lines. They run against contracts in `examples/` with `deploy.sh` and
`play.sh` behind them, so no example depends on a project I personally keep alive.

Done when: all four run from a clean clone against a freshly deployed example contract.

### 5. Docs that end in a page

`docs/first-backend.md` currently ends at a working backend; the quickstart should end
at rows moving on a screen. Plus one new page — "Reading from React" — that is the
SDK reference, sitting next to `docs/reading.md` (which stays the raw HTTP contract).

Done when: nothing in the happy path requires reading Rust or `api.ts`.

## What this forces in the core

The examples will hit these in the first hour. They are small Rust changes and they
belong inside this milestone rather than discovered during it:

- **`where` is equality-only** (`crates/nineveh-api/src/tables.rs`, "`WHERE column =
  value AND …`, one equality per filter"). A feed wants `version > x`; a leaderboard
  wants `volume >= n`. Needs `gt`/`gte`/`lt`/`lte`/`neq`, as a bounded, typed operator
  set — not a query language.
- **`order` is one column** (`fn order`, splitting on the last `.` for direction). Any
  real leaderboard tie-breaks on a second column.
- **Feed `reset` is unhandled in the prototype.** `nineveh-realtime` sends `reset` when
  a rebuild is swapped in (ADR 0016) and the client must reload everything it listed;
  `useLive.ts` hedges with a 15s poll instead. The SDK must handle it properly, and
  `useLiveTable` should then be able to drop the poll floor.
- **Feed retention is 7 days** on the free tier (`nineveh-control/src/tier.rs`), so a
  resume position older than that cannot be honoured. The SDK needs one honest
  behaviour for "your position is gone" — almost certainly: re-list, re-open at newest,
  tell the caller it happened.

Deliberately *not* papered over in hooks. The point of owning both halves now is that
an API rough edge gets fixed where it lives.

## Decisions to confirm

1. **Where the package lives.** The earlier thinking was a separate `nineveh-react`
   repo, for one reason: `.github/workflows/ci.yml` has no path filters, so a
   TypeScript change sits behind the full Rust matrix — fmt, clippy, nextest, MSRV,
   `cargo deny`, codegen drift — to prove a `tsc` run. That's an argument for fixing
   CI, not for splitting the repo. Recommendation: `clients/react` in this repo, plus path filters on the
   workflow (small), because the real risk now is SDK-vs-API drift and the type
   generator reads an endpoint that lives in this tree. Publish `@nineveh/react` from a
   tag either way. `studio/` already establishes that TypeScript lives here.
2. **Merge or re-query on a change.** The prototype re-reads the table on any change,
   debounced 400ms. The feed carries the full row in the REST shape precisely so a
   client *can* merge locally. Recommendation: re-query in v1 (correct under filters
   and ordering, one code path), with merge as a later opt-in — but the hook's contract
   should be written so merging does not become a breaking change.
3. **How much of the SDK Studio should eat.** Studio's playground could emit the exact
   `useTable` call for a query the user just ran. Cheap, and it closes the loop between
   the two surfaces. In or out?

## Not in this milestone

No write path. No auth beyond the API key. No react-query-scale caching. No GraphQL.
No Vue/Svelte. Three primitives done properly, four examples, a template.

## Sequencing

Roughly a week of focused work, in this order, each step shippable:

1. `clients/react` scaffold + CI path filters; port `api.ts`, `useLive.ts`, `useFlash.ts`
   as the package's internals.
2. `useTable` / `useLiveTable` / `NinevehProvider` / `useStatus` + the `reset` and
   retention paths.
3. Rewrite `demos/echelon-fe` on the package — the first real consumer, and the test of
   whether it is pleasant.
4. Core API: operators and multi-column order, driven by what steps 3 and 5 actually
   want.
5. The four examples; fix the API wherever one reads badly.
6. Type generation.
7. `create-nineveh-app`, then the docs page and the quickstart's new ending.

Against the other track: the design-partner demo is backend correctness work and does
not need the SDK — but it is the honest test of it, so build its page *with* the SDK
once step 3 is done rather than hand-rolling another one-off frontend.

## Risks

- **It stays a demo-shaped library.** `echelon-fe` was written to get one screen
  working; generalising the wrong three functions is the easy failure. Mitigation is
  the examples: they are the spec, and they are written against the package, not
  before it.
- **Scope creep into caching.** The moment two components want the same table, a cache
  looks necessary. It is not, at v1 sizes. Note it and move on.
- **The core changes grow.** Operators on `where` are a slope that ends in a query
  language. Bounded, typed set, enumerated in the config-validated path, no string
  passthrough.
