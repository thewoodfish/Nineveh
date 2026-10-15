# Types for your own tables

One generated file and the compiler knows your project: every table name, every column
name, and — the part that matters — which columns are numbers and which are strings.
None of it is required; all of it is one command.

## Generating them

```sh
npx nineveh-types https://api.nineveh.dev/projects/myproject --key nvk_…
# wrote src/nineveh-tables.d.ts: 5 tables, 25 columns
```

It asks the project what it holds and writes one declaration file. Nothing imports it —
it only has to sit somewhere your `tsconfig.json` already includes, which is why it
defaults to `src/`. Put it outside and nothing breaks and nothing works: a declaration
file the compiler never reads changes nothing and doesn't say so.

Commit it. It is generated, but it is also the contract your app is written against, and
a reviewer should see it change when the backend does.

## What stops compiling

```tsx
useTable("sellres")                       // did you mean 'sellers'?
useTable("sellers", { order: "revenu" })  // did you mean 'revenue'?

row.revenue * 2       // error: a u128 arrives as a string
row.event_index * 2   // fine: a u32 arrives as a number
```

That last pair is the point.

## The one rule about integers

**Integers up to 32 bits arrive as JSON numbers and anything wider as decimal strings**,
because `Number()` over a `u64` loses digits above 2⁵³ and does it silently. Move a
balance of 9,007,199,254,740,993 octas through a JavaScript number and it comes out as
…992, with nothing raised anywhere.

Which call to make per column is the one thing a hand-written type gets wrong, and
getting it wrong is invisible until an amount gets large — which is to say, until it
matters. The generated file makes the compiler carry that rule for you.

Format wide values with `BigInt`, and keep them as strings everywhere else:

```tsx
const revenue = BigInt(row.revenue);
```

## Without the file

Every hook still works and rows are `Row`, whose columns are `unknown`. You reach a
column by asserting it, and the compiler helps you with none of it — which is a fine
place to start and a poor place to stay.

Re-run `nineveh-types` whenever the backend's tables change. A rebuild that adds a
column does not tell your editor about it.
