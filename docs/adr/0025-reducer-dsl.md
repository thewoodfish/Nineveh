# 0025. Write reducers in a small JS-shaped DSL that compiles to rules

- Status: Proposed
- Date: 2026-09-24

## Context

A project's behaviour — what happens to state when a record arrives — is written today
as YAML under each state table:

```yaml
state:
  balances:
    key: [user]
    columns: { user: address, balance: { type: u128, default: 0 } }
    reduce:
      - { on: deposits,    set: { balance: "balance + u128(amount)" } }
      - { on: withdrawals, set: { balance: "balance - u128(amount)" } }
  market_stats:
    key: [market]
    columns: { market: address, volume: { type: u128, default: 0 } }
    reduce:
      - { on: deposits, set: { volume: "volume + u128(amount)" } }
```

The expressions inside are fine: `nineveh-expr` is typed, total and exact (ADR 0007),
with located diagnostics. The problem is the layer above them.

- **Behaviour is scattered by table, not by event.** "What happens on a deposit" is
  spread across every table a deposit touches. You find it by grepping for
  `on: deposits`, and you add a new effect by editing a different part of the file.
- **Control flow is encoded in map keys.** `when:` is an if; rule order is sequencing;
  a two-branch fold is two rules that must be read together to see they are exclusive.
- **The editor gives nothing.** No completion for a source's fields or a table's
  columns, no go-to-definition, no type feedback until `nineveh validate` runs.

Nineveh targets teams who will not write Rust. A backend whose behaviour layer cannot
comfortably be written by hand undercuts the product. This is the judgement that forces
the decision: terse YAML is still YAML, and terseness was never the complaint.

What makes a frontend possible is that the target is small and regular. A `Rule` is:

```rust
Rule { on: Trigger, when: Option<Expr>, key: Vec<(Named, Expr)>, action: Set(Vec<(Named, Expr)>) | Delete }
```

Read as English: *when a record from this source arrives, and `when` holds, upsert this
row, setting these columns.* Every rule is one trigger, one condition, one row, one set
of column values. That maps exactly onto **a row binding plus the assignments made to it
under one condition** — which is an ordinary-looking block of JavaScript.

## Decision

Reducers are written in a small DSL with JavaScript syntax, in a `.nineveh.ts` file. It
is a compiler frontend, not a runtime: it produces `nineveh-config` model values and
nothing downstream changes. No JavaScript engine is embedded, ever.

### The surface

The file declares its reduce tables and its handlers:

```ts
export const balances = table({
  key:     { user: address },
  columns: { balance: u128.default(0), deposits: u64.default(0) },
})

on(deposits, (d) => {
  const b = balances.row(d.user)
  b.balance  += u128(d.amount)
  b.deposits += 1
})

on(withdrawals, (w) => {
  if (w.amount == 0) return
  balances.row(w.user).balance -= u128(w.amount)
})

on(vaults.deleted, (v) => {
  balances.row(v.address).delete()
})
```

`nineveh.yaml` keeps everything that is configuration rather than behaviour: `name`,
`network`, `start_version`, `sources`, `mirror` and `log` tables, `api` and `webhooks`.
Everything it names is in scope in the DSL file: sources as handler subjects, and
`mirror` tables as lookups like any reduce table. `log` tables are not in scope, because
a rule cannot read one (ADR 0019). A handler may not write a `mirror` or `log` table —
their source writes them.

The handler's parameter is a local alias for the record, so `on(deposits, (d) => …)`
makes `d.amount` the `amount` field of a `deposits` record. The name is the developer's
choice; `deposits.amount` and a bare `amount` are what the same expression is called in
YAML.

That is the entire language. Statements:

| Statement | Meaning |
| --- | --- |
| `const x = <expr>` | a named value; substituted at its uses |
| `const r = <table>.row(<expr>, …)` | names the row a rule writes |
| `r.<col> = <expr>`, `+=`, `-=` | set a column of that row |
| `r.delete()` | delete that row |
| `if (<expr>) { … } else { … }` | condition on the writes inside |
| `return` | stop; later writes in this handler don't apply |

Expressions are `nineveh-expr` (ADR 0007), spelled the way JavaScript spells them.
`&& || ! == != < <= > >= + - * / %`, `1_000_000`, `'text'` and field access are already
identical. Three forms change, each toward what a JS developer expects:

| `nineveh-expr` | DSL |
| --- | --- |
| `if c then a else b` | `c ? a : b` |
| `unwrap_or(markets[m].fee_bps, 0)` | `markets.get(m)?.fee_bps ?? 0` |
| `42u128`, `@0x1` | `u128(42)`, `address("0x1")` |

`?.` and `??` are not decoration: a cross-table lookup is an option (ADR 0019), which is
exactly what those operators mean. Type names double as conversions, as they already do
for integers: `address` is a column type in `table({…})` and a conversion in an
expression.

### The compile

1. Walk each `on(…)` handler. Group every write by **(row binding, path condition)**.
   Each group emits one `Rule`.
2. `on: Trigger { source, deleted }` comes from the handler's subject.
3. `when` is the conjunction of the conditions in scope at the write: each enclosing
   `if` (negated in an `else`), and the negation of every earlier `return`'s condition.
   No conditions means `when: None`.
4. `key` is the `.row(…)` arguments, positionally against the table's declared key
   columns.
5. `Set` holds the group's assignments in statement order. `r.c += e` emits
   `(c, "c + e")`; `r.c = e` emits `(c, e)`.
6. `const` bindings of plain values are substituted into the expressions that use them.
   They generate no IR.
7. Rules are emitted in the statement order of each group's first write.

A binding may therefore produce several rules — one per path condition — which is how
`if`/`else` over the same row compiles.

### Rules fire in the order they are written

`Plan` currently collects `write_rules` by iterating tables in config order and then
rules within each table (`nineveh-engine/src/fold.rs`). Under a handler-centric surface
the order a developer sees is statement order, and the two differ when one handler
writes table A, then B, then A again. It is observable, because a rule can read a table
it also writes (ADR 0019).

Each rule therefore carries the sequence number assigned in step 7, and `Plan` sorts
`write_rules` and `delete_rules` by it. Configs written as YAML number their rules in
the existing table-major order, so their behaviour is unchanged. The rule a developer
has to hold in their head becomes: **rules apply in the order you wrote them.**

### What the checker rejects

Beyond the existing config and expression validation, which is unchanged:

- a binding both assigned and deleted under the same path condition;
- two assignments to the same column of the same binding under the same path condition;
- writes after an unconditional `return`;
- any identifier that is not a source, a declared table, a handler parameter, a `const`
  in scope, or a built-in.

There is no allow-list of forbidden globals to maintain. `Date.now()` is not rejected
because it is on a list; it fails because `Date` resolves to nothing. Determinism is a
property of what the grammar can express, not a filter over what it cannot.

### Where it lives

A new crate, `nineveh-dsl`, depending on `nineveh-config`, `nineveh-expr` and
`nineveh-core`. It parses the DSL file, merges its tables and rules with the YAML, and
returns the same `Config` the YAML path returns today. `nineveh-cli` and
`nineveh-control` load projects through it. `nineveh-config` does not depend on it, so
the direction in ADR 0005 holds; `scripts/check-deps.sh` gains
`nineveh-dsl tokio sqlx tonic`.

The parser is hand-written, for the reason ADR 0007 gives: error quality matters more
than generator convenience, and `nineveh-config`'s diagnostics are the renderer.

### Both paths converge

`reduce:` in YAML keeps working, and both frontends produce the same `Config`. A project
uses one or the other for a given table, not both. The equivalence is a test, not a
promise: the vault workload in `nineveh-testkit` is expressed both ways, and the
resulting `Config::canonical()` must match.

## Alternatives considered

**Improve the YAML.** Shorter keys, an inline rule form. Rejected: the complaint is
locality, editor support and control flow living in map keys. None of those improve by
respelling YAML.

**Generate YAML text from the DSL, then re-parse it.** Rejected: two formats to keep
honest, and error spans would point into generated text rather than the file the
developer wrote. Compiling straight to the model is strictly less work.

**A general JS-shaped language with `get`/`create`/`update`/`increment`, locals, loops
and collection helpers.** Rejected. The engine has upsert and delete; `create` would
require inventing a conflict error that does not exist, `increment` is strictly less
expressive than `balance + amount`, and collection helpers need array columns that
`ColumnType` does not have. Each would be a runtime change wearing a frontend's clothes.
Every construct in the decision above is a spelling of something the fold already does.

**Move the whole project into TypeScript, dropping `nineveh.yaml`.** Rejected for now:
it rewrites config loading, `init`, the lock flow and Studio's config generation at
once, for a surface that is genuinely configuration. Reconsider once the DSL has users.

**Embed QuickJS or similar and run handlers per record.** Rejected outright: it breaks
determinism and replay (ADR 0005), and puts an interpreter on the hot path.

## Consequences

Behaviour gains a home: one event is one place, and adding an effect means adding a
line to the handler that already exists. The editor can help, because the file is valid
TypeScript syntax — a generated `nineveh.d.ts` gives completion on a record's fields and
a table's columns, and `tsc` catches typos before `nineveh validate` runs. The
substantive checks stay in Rust; the `.d.ts` is convenience, never the source of truth.

Nineveh now maintains a second frontend. Every change to `Rule`, `Column` or the
expression language has to be spelled in both, and the convergence test is what keeps
them honest.

Diagnostics inside an expression are coarser at first. `model::Expr` carries source
text and a span, so a translated expression reports against the whole expression's span
in the `.ts` file rather than the exact sub-token. Good enough to ship; worth a span map
later.

Studio's visual reduce editor should generate DSL rather than YAML once this lands,
which makes generated and hand-written reducers the same artifact — a developer can read
what Studio produced, and edit it.

`nineveh init` should scaffold a `.nineveh.ts` with a handler per discovered source.
That scaffolding is a large part of the product experience: the first thing a developer
sees is working, readable code for their own contract.
