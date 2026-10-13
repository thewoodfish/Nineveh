# @nineveh/react — SDK spec (draft to tear apart)

A thin, typed, ergonomic wrapper over Nineveh's existing REST + SSE API.
**No new backend capability** — it just makes the common things trivial.
This is a draft of the *shape*, not a finished design. Push back on anything.

Goal: a Nineveh-backed frontend in minutes. `useTable` for "how are things",
`useLiveTable` for "tell me when they change". That's the whole product for most devs.

Everything below has been checked against the running API. Where the API doesn't do
something yet, it says so rather than wishing.

- API reference: https://nineveh.dev/docs/reading (the SDK's contract — read this first)
- A working implementation of most of this already exists: `demos/echelon-fe`
  (React 19 + Vite, ~300 lines). See "Prior art" at the bottom. Start there, not here.

---

## Provider

```tsx
import { NinevehProvider } from '@nineveh/react'

<NinevehProvider
  project="https://api.nineveh.dev/projects/myapp"  // a project's base URL
  apiKey={import.meta.env.VITE_NINEVEH_KEY}
>
  <App />
</NinevehProvider>
```

Holds one shared client (base URL, key, a single SSE connection reused across hooks).

**The key is required.** Every read needs one and a key opens exactly one project; a
request without one is refused, and a key for another project is refused too. The one
exception is a locally-run plane in local mode, which serves loopback without a key —
so `apiKey` is optional in the *type*, but in practice it's always set against hosted.

The key reaches the browser. That's intended: reads only ever touch Postgres state
tables, never the chain, so an exposed key costs stream credit nothing. Give a frontend
its own key and revoke it when it's done.

Three places the key can go, tried in this order: `Authorization: Bearer`, an `apikey`
header, or `?apikey=` on the URL. Use the header for REST. The query string exists for
`EventSource`, which cannot set headers — so the change feed has to use it.

---

## useTable — query (the 80% case)

```tsx
const { data, loading, error, count, refetch } = useTable('sellers', {
  where:  { seller: '0xabc…' },      // equality only — see below
  order:  'revenue.desc',
  limit:  10,
  offset: 0,
  count:  true,                      // opt in; costs a second query
})
```

Returns:
```ts
{
  data:    Row[] | undefined
  loading: boolean
  error:   Error | null
  count:   number | undefined   // exact, only when `count: true`
  refetch: () => void
}
```

Thin wrapper over `GET /v1/tables/:name?…`. Re-queries when args change. `data` is
undefined while first-loading, then the rows.

### What the API actually supports

- **`where` is equality only.** `?<column>=<value>`, AND-ed. There are no `gt`/`lt`/`in`
  operators — the server builds `WHERE col = val AND …` and nothing else. So v1 `where`
  is `Record<string, string | number>`. Anything richer is a **core change**: tell me
  what you need and I'll add it server-side, don't fake it client-side by over-fetching.
- **`order` is one column**, `col`, `col.asc` or `col.desc`. `_version` is orderable
  too. No multi-column sort.
- **Default order is `_version` descending** — most recently changed rows first — always
  tie-broken by the row key, so pagination is stable. Worth surfacing in docs: "newest
  change first" is a sensible default feed order and devs won't expect it.
- **`limit` defaults to 50, maxes at 1000**, and a larger value is silently clamped
  rather than rejected. The hook should probably clamp too, so `limit: 5000` doesn't
  look like it worked.
- **`count: true`** sends `?count=exact` and returns a real unbounded `count(*)`. It's
  exact, but it is a second query over the filtered set — opt-in, never automatic.
- JSON columns can't be filtered on; the server returns 400 if you try.

**Open question for you:** `where` as a typed object vs. passing the raw filter string.
I lean object, with a `raw?: string` escape hatch appended to the query so the SDK can
never be the reason something in the API is unreachable. Your call.

---

## useLiveTable — subscribe (the differentiator)

```tsx
const { data, loading, error, connected } = useLiveTable('sellers', {
  order: 'revenue.desc',
  limit: 10,
})
```

- Lists the rows, then opens (or joins) the SSE change feed
  `GET /v1/changes?tables=sellers&apikey=…`.
- Keeps the row set current as changes arrive.
- `connected` reflects live-feed status so the UI can show a "live" dot.

This is the hook devs won't want to hand-write. It's the magic moment and the thing
event-indexers don't give cleanly. **Lead the SDK's story here.**

### The four things the feed makes you handle

1. **Position and resume.** Each event's id is `version.seq`. `?after=version.seq`
   resumes just after it, `?after=beginning` replays everything retained. `EventSource`
   sends `Last-Event-ID` on reconnect by itself, so a dropped connection costs nothing
   — the hook mostly has to not get in the way of that.
2. **Retention is a window, not forever.** Free tier keeps **7 days** of feed history.
   "Nothing missed, nothing twice" holds inside it; a client that was away longer must
   re-list rather than resume. Don't promise more than that in the README.
3. **`reset` means the tables were rebuilt** and a new build was swapped in. The feed
   continues from the new build's newest change, and everything you listed is from the
   old one. A client that ignores `reset` shows stale rows forever, with no error. The
   hook **must** re-list on it. This is not optional and it's the one thing I'd check
   first in review.
4. **A row's identity comes from the schema, not the row.** A listed row carries
   `_version` and its declared columns — *not* its key. A feed change carries a separate
   `key` object (`{"user": "0xabc…"}`). To match a change to a listed row you need the
   table's key columns, which come from `GET /v1/tables` (`key: string[]`, in key order).
   So the client should fetch and cache the table list once; `useLiveTable` needs it
   anyway, and it's what makes generated types free (below).

### Re-query or merge locally?

**Already answered in `demos/echelon-fe/src/useLive.ts`, and I'd start there:** on each
change it debounces 400ms and re-reads the tables, plus polls every 15s as a floor. The
feed is used as a *signal that something changed*, not as the data. It's simpler, always
exactly right, and it keeps `order`/`limit` correct for free — which local merging does
not, since a change can reorder or push a row out of a `limit: 10` window.

That demo also does something I want kept: it opens the feed just *before* the oldest
`_version` on screen, so it replays how the visible rows got there instead of starting
blank and waiting for the chain to move. `after=beginning` would also work and would
grow without bound; that window is exactly what's on the page.

Local merge is the optimization, not the baseline. If you want it, I'd want it behind an
option with the re-query path as the default. Your lane though — if you think merge-first
is right, make the case.

**Still genuinely open:** one shared SSE connection multiplexed across hooks, or one per
hook? Shared scales better and the server already supports `?tables=a,b`. Lean shared.

---

## Types

```ts
// generated or hand-declared per project
type Sellers = {
  seller:   string   // address
  sold:     number   // u32  → JSON number
  revenue:  string   // u128 → decimal string
  _version: string   // always present
}

const { data } = useTable<Sellers>('sellers', { order: 'revenue.desc' })
// data?.[0].revenue  ← autocompletes, typed
```

How values arrive, exactly (the SDK must not "helpfully" convert any of it):

| Move / column type | JSON |
| --- | --- |
| `bool`, `u8`–`u32`, `i8`–`i32` | number / boolean |
| `u64`, `u128`, `u256`, `i64`–`i256` | **decimal string** — parse with `BigInt`, never `Number` |
| `address` | string, full width |
| `bytes` | `0x…` hex string |
| `string` | string |
| `json` | nested JSON |
| `_version` | decimal string, on every row |

Only ints ≥64 bits are strings; the narrow ones really are numbers. Don't apply
`BigInt()` across the board. The loss above 2^53 is silent, which is why the wide ones
are strings and why the SDK must never coerce them to `number`.

`_version` is the transaction that last wrote the row. It's how you tell a stale copy
from a current one, and how you dedupe a change you've already applied. Put it in the
generated types.

### Generated types are unblocked and yours

`GET /v1/tables` already returns, per table: `name`, `kind` (`reduce`/`mirror`/`log`),
`key` (column names in key order), and per column `name`, `type` (`u64`, `address`,
`json`, …) and `nullable`. That's everything a `.d.ts` needs.

So a `nineveh-gen-types <project-url>` that emits types from a live project needs **no
Rust and nothing from me** — it's a TS script against one endpoint. I'd love it to exist
and it's entirely in your lane. Biggest trust/ergonomics win available here.

---

## Errors the hooks will meet

Errors come back as `{"error": "…"}` with a status:

- **404** — no such table. A bug in the caller; surface it loudly.
- **400** — bad parameter (unknown column to order by, filtering a JSON column, a value
  that doesn't cast to its column's type). Also a caller bug.
- **503** — `"the served build doesn't match the config yet: it's being rebuilt"`. This
  is **transient and expected** when a config change swaps in a new build. The SDK should
  retry with backoff, not surface it as a fatal error. Pairs with `reset` on the feed.
- **401** — missing or wrong key.

A `useStatus()` bonus hook is easy if you want it: `GET /v1/status` returns the project,
network, current build cursor, any in-progress rebuild, and pipeline health. That's the
"synced / N versions behind" badge, which is the other thing every one of these UIs wants.

---

## Non-goals (keep it tight)

- No write path (Nineveh is read-only state).
- No auth beyond the API key.
- No global state manager, no caching layer beyond simple in-hook. (If it grows into
  needing react-query-style caching, that's a deliberate later decision, not v1.)

Three primitives — provider, `useTable`, `useLiveTable` — done beautifully.

---

## Example apps (built alongside the SDK — they ARE its spec)

Each ~30–50 lines, runnable, copy-paste-to-ship, against a real Nineveh project:

1. **Leaderboard** (game) — `useTable('players', { order: 'wins.desc' })`
2. **Holders** (token) — sortable balances table
3. **Live feed** (social) — `useLiveTable('posts', …)`, visibly ticking
4. (bonus) **Dashboard** — a couple of tables on one screen, plus `useStatus()`

If writing any example against the SDK feels clunky, the SDK API is wrong — fix it there.

You don't need to invent backends for these. `examples/` in the main repo has four
published Move contracts with deploy + traffic-generating scripts — counter, guestbook,
market (with reducers: per-seller revenue, per-buyer spend) and arena. `./deploy.sh` then
`./play.sh` gives you a project with rows moving in it. The market one is the natural
leaderboard.

Stretch: a `create-nineveh-app` / quickstart template — the thing a dev actually clones.
Single biggest "0→1 in 5 minutes" lever. Your call if in scope.

---

## Prior art — read this before writing anything

`demos/echelon-fe` is a React 19 + Vite app already doing this against a live project:

- `src/api.ts` — the whole REST + `EventSource` surface, ~35 lines. This is `useTable`'s
  and `useLiveTable`'s transport, already written.
- `src/useLive.ts` — the live hook: list, open the feed at the right position, re-read on
  change, poll as a floor. This is `useLiveTable`'s prototype.
- `src/useFlash.ts` — row-level change highlighting. The thing that makes "live" *feel*
  live, and the kind of ergonomic the SDK should probably own.

It's two tables hard-coded rather than a general hook. Generalising it *is* the SDK. If
you disagree with a choice in there, that's exactly the feedback I want — it was written
to get a demo on screen, not to be a library.

---

## Working boundary

- You own the design and the code of `@nineveh/react` + the examples — the whole
  frontend surface, and the call on anything I've guessed at in this doc.
- I hold the repo and the release. Changes land as a branch and a PR into `main` that I
  review and merge; npm releases I cut from a tag.
- I own the Rust core — ingest, reducers, correctness. Core changes route through me.
- Rough edges you hit in the REST/SSE API are exactly the feedback I want. Several
  things in this doc (operators in `where`, multi-column sort) are core changes I'd
  rather make properly than have the SDK paper over.

Saying that plainly up front rather than "it's all yours" and then asking for a PR in
week two. The package carries the project's API promise under its name, so versioning and
what ships when are part of a story I have to keep coherent. Everything up to the merge
button is yours, you're named as its author and maintainer, and where a design call of
yours and my taste disagree, the SDK's ergonomics win — that's the point of you being on
it.

### The mechanics

- **Repo**: its own `nineveh-react` repo on my account, you with write access to push
  branches and open PRs; `main` protected so changes arrive reviewed. A directory in the
  main repo would be worse for you specifically: its CI has no path filters, so every PR
  runs the full Rust matrix — fmt, clippy, nextest, MSRV, `cargo deny`, codegen drift —
  and a TypeScript change would sit blocked behind Rust jobs you can't fix. Separate repo
  means your CI is `tsc` and your tests, and it runs in a minute.
- **npm**: I hold the `@nineveh` org and publish. Tell me when a change is release-worthy
  and what it should be called — patch, minor, breaking — and I'll cut it. If the cadence
  ever makes me the bottleneck, say so and we'll automate it off a tag from `main`.
- **License**: Apache-2.0, the same as the core repo — one license across the project,
  and the patent grant is the reason rather than a preference.
- **Review**: light on anything purely frontend — it's your judgement, I'm not going to
  bikeshed hooks. Close on the handful of places the API can bite you: the `reset` path,
  feed resume and position, and the wide-integer handling. Those are where a plausible
  implementation is silently wrong, and where I can tell from the core side.

## What you need from me

- [x] REST + SSE stable and documented: https://nineveh.dev/docs/reading
- [x] A real live project with populated tables: `api.nineveh.dev/projects/echelon` —
      I'll send you a key. `examples/` gives you more you control yourself.
- [x] A table-schema endpoint for generated types: `GET /v1/tables`, has everything.
- [ ] A key for you, and a second project if you want one that isn't echelon.
