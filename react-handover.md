# Handover note — @nineveh/react

Short version of what Nineveh is, what I'd want you to own, and the three places to
look. The full shape is in `react-spec.md` — this is the orientation for it.

## What Nineveh is, in one paragraph

A reactive backend for Aptos apps — Firebase/Supabase, not The Graph. You point it at a
contract, describe the state you want, and you get a live Postgres-backed database with
a REST API and a real-time change feed, kept in sync with the chain, no infra to run.
Aptos' own APIs are point-reads only: you can't ask for aggregates, feeds, history or
"everything matching X". Nineveh is the backend that sits in that gap.

For you, none of the chain part matters. It's a REST API over tables, plus an SSE feed
that tells you when a row changed. That's the entire surface you'd build on.

## What I'd want you to own

`@nineveh/react` — a thin typed wrapper over that API, three primitives:

- `NinevehProvider` — base URL + key, one shared connection
- `useTable` — query ("how are things")
- `useLiveTable` — subscribe ("tell me when they change")

Plus the example apps, which I think *are* the spec: if an example is clunky to write,
the API is wrong and we fix the API. Leaderboard, token holders, live feed, a dashboard.

**You own the design and the code; I hold the repo and the release.** Concretely: the
package lives in a `nineveh-react` repo on my account, you get write access, and changes
land as a branch and a PR that I review and merge into `main`. Releases to npm I cut
myself from a tag. Apache-2.0, same as the rest of the project.

I'm being upfront about that rather than saying "it's all yours" and then asking for a
PR in week two. It isn't distrust — it's that the package carries the project's API
promise under its name, so versioning and what goes out when are part of a product story
I have to keep coherent. What's genuinely yours is everything up to the merge button:
the API shape, the implementation, the examples, and the call on anything I've guessed at
in the spec. You're the author and maintainer of it, named as such. Where a design call of
yours and my taste disagree, the SDK's ergonomics win — that's the whole reason to have
you on it.

I own the Rust core and anything that touches correctness. API rough edges you hit are
the feedback I most want, and I'd rather fix them properly in the core than have the SDK
paper over them. Stretch, entirely in your lane if you want it: a `create-nineveh-app`
template and a type generator (the schema endpoint already has everything it needs — no
Rust).

## Three places to look, in this order

1. **https://nineveh.dev/docs/reading** — the API reference. REST params, the change
   feed's resume semantics, webhooks. This is the contract you'd be wrapping; it's
   complete enough to build against without talking to me.
2. **`demos/echelon-fe`** — a React 19 + Vite app already doing this against a live
   project. `src/api.ts` is the whole transport in ~35 lines; `src/useLive.ts` is
   `useLiveTable`'s prototype; `src/useFlash.ts` is the row-highlight that makes it feel
   live. It's two tables hard-coded instead of a general hook — generalising it is the
   SDK. Written to get a demo on screen, so disagree with it freely.
3. **`react-spec.md`** — the proposed API shape, with the API's real limits written in
   (equality-only filters, single-column sort, 7-day feed retention, which numbers
   arrive as strings, and the `reset` event you have to handle or data goes stale
   silently). Everything in it has been checked against the running API. It's a draft of
   the shape, not a finished design — push back on any of it.

## State of things

The API is live and hosted (`api.nineveh.dev/projects/echelon`); I'll send you a key.
`examples/` in the repo has four published Move contracts with deploy and
traffic-generating scripts, so you can stand up a project of your own with rows moving
in it and not depend on mine.

Two things to settle in the first conversation: how much review you actually want from me
beyond the merge itself (my instinct is light on anything frontend, close on the few
places the API can bite you — the `reset` path, feed resume, and the wide-integer
handling), and whether you want a second live project that isn't echelon.
