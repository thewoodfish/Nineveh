# Types for your own tables

Every hook works without this page. Generated types sharpen them; they are not a
dependency. What you get is the compiler knowing your project's table names, column
names, and — the part that matters — which columns are numbers and which are strings.

## Generating them

```sh
npx nineveh-types https://api.nineveh.dev/projects/myproject --key nvk_…
# wrote src/nineveh-tables.d.ts: 5 tables, 25 columns
```

It asks the project what it holds and writes one declaration file. That file augments
the package with your project's tables. It needs no importing — what matters is that it
sits inside your tsconfig's `include`, which is why it defaults to `src/`. A declaration
file the compiler never reads changes nothing and doesn't say so.

Commit it. It is generated, but it is also the contract your app is written against, and
a reviewer should see it change when the backend does.

## What stops compiling

```tsx
useTable("sellres")                       // error: did you mean 'sellers'?
useTable("sellers", { order: "revenu" })  // error: did you mean 'revenue'?
row.revenue * 2                           // error: revenue is a u128, so it's a string
row.event_index * 2                       // fine: event_index is a u32, so it's a number
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
