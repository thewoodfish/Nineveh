# Demos

Live indexes of real mainnet protocols, built to be sent to the people who wrote them.

| | What it is |
| --- | --- |
| [`echelon/`](echelon/) | The project: `nineveh.yaml`, the reducers, the pinned lock. |
| [`echelon-fe/`](echelon-fe/) | The page it feeds — Vite + React, one static build. |

Each demo is a config and a page, and the page's job is to make the config the
interesting part. A dashboard proves the numbers are right and that they move; neither
of those is a reason to choose Nineveh. The thing on the page nothing else can do is
state the contract never emitted.

## What a demo has to be

1. **Right.** The numbers match the chain, checked by eye against a fullnode read
   before anyone sees it.
2. **Alive.** It moves while you watch, from the change feed.
3. **Cheap.** The config is on the page, because that is the claim.

And one rule learned the hard way: **never show a derived number you cannot verify.**
The Echelon page briefly led with a dollar total that summed APT, USDC and WLFI token
counts as though each were a dollar, and later nearly shipped a debt figure whose
formula was out by one part in 10^8 against the chain's own view functions. Both looked
entirely plausible. The page now shows what the chain stored and says what it is.

## Echelon

Package `0xc6bc659f1649553c1a3fa05d9727433dc03843baac29473c817d06d39e7621ba`, four
modules, mainnet. Two resource sources — `lending::Vault` and `lending::Market` — and
the events alongside them.

It works because **`Vault` is one resource per account**: one record, one row, no
fan-out. And the argument writes itself from their own documentation, which tells
integrators to *"index `SupplyEvent`"* for vault addresses and then read each account's
resource by hand.

Verified: a live vault's `collaterals`, `liabilities` and `efficiency_mode_id` match
the chain field for field, and the APT market's `total_cash` and `total_liability`
match to the digit.

## Aries — scoped, and blocked on one engine feature

Package `0x9770fa9c725cbd97eb50b2be5f7416efdfd1f1554beb0750d4dae4c64e860da3`, 26
modules, mainnet. Confirmed live.

**The half that works today.** `reserve::Reserves.stats` is a
`0x1::table::Table<TypeInfo, …>`, so per-asset reserve statistics are followable with
a `table:` source and no new code. The `controller` module emits 26 event types, which
cover activity.

**The half that doesn't.** Per-account positions live in `profile::Profile`, whose
`deposited_reserves` and `borrowed_reserves` are
`iterable_table::IterableTable` — Aries' own linked-list wrapper whose `inner` is a
`TableWithLength`. A `table:` source names *one field*, and that field must hold the
table itself. Here the field holds a struct that holds the table.

So an Aries demo of the Echelon kind — a row per account, live — needs:

> **A `table:` source that can name a path.** `Profile.deposited_reserves.inner`,
> walking struct fields to reach a container. Three places change: `table_of` in
> `nineveh-control`'s catalog walks the path rather than inspecting one field;
> `TableMatcher::for_field` in `nineveh-decode` resolves a path; and `handle_in` in
> `nineveh-engine` reads a handle from a nested field rather than a direct one. The
> config grammar already allows the spelling — `Struct.field` becomes `Struct.a.b`.

It is a real feature with value beyond Aries: wrapping a table in a struct that tracks
a head, a tail and a length is an ordinary Move idiom, and `fixtures/abi/testnet` already
carries one from an unrelated contract (`linked_list::LinkedList`). It is not an
afternoon, which is what Echelon was.

**Recommended order.** Don't start Aries until Echelon has done its job. One live index
of a protocol that is talking to you beats two of protocols that aren't, and the second
demo costs an engine feature that should be justified by someone asking for it rather
than by symmetry.

## If there is a second demo, don't copy the page

`echelon-fe` is a standalone Vite app, and a second protocol should not be a second
copy of it: the shell, the formatting, the feed and the styling are all identical, and
only the tables, the columns and the sentences differ.

Make the page take a protocol description — the tables to read, the columns to show,
the copy — selected at build time, and deploy the same build twice with different
environments. One codebase, two Vercel projects. Do this *when* the second demo
exists, not in anticipation of it.

## Running one locally

```sh
cd demos/echelon-fe
cp .env.example .env     # the project's read key, from Studio → Settings → API keys
npm install && npm run dev
```

The key reaches the browser, because the page calls the API directly. That is why each
demo gets a key of its own: revoke it in Studio when the demo is done and nothing else
notices. Reads never touch the chain, so an exposed key spends no stream credit.
