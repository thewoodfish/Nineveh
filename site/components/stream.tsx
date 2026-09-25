"use client";

// The product, in one picture, in the order it actually happens: events and state changes
// arrive from the chain on the left, the reducer you wrote is in the middle, and the
// application state on the right is what it folded them into.
//
// The middle pane is the whole reason this drawing exists. With two panes it read as
// chain in, table out — which is a claim about magic, and invites the reader to assume
// the interesting part is indexing. The interesting part is the rule, it is four lines
// long, and it is yours. So it sits in the centre, tinted blue like Nineveh's band in the
// diagrams further down the page, and it lights up as each record passes through it.
//
// Nothing here is fetched — it's a drawing, driven by a seeded sequence so the server and
// the browser agree on the first frame. The handler is the same text `nineveh-dsl`'s
// tests/landing.rs pins, so the hero can't drift from the language either.

import { useEffect, useState, type ReactNode } from "react";

const SELLERS = ["0x7a3f…c41d", "0x1e87…8d2a", "0x9b02…4f77", "0x6146…e554"];
const ITEMS = ["brass lamp", "oak chair", "wool rug", "clay mug", "iron kettle"];

type Sale = {
  n: number;
  seller: number;
  item: string;
  price: number;
  fee: number;
  version: number;
};

/** A small deterministic sequence: the same numbers every time, on both sides. */
function roll(n: number): number {
  return (Math.imul(n + 1, 1_664_525) + 1_013_904_223) >>> 0;
}

function sale(n: number): Sale {
  const r = roll(n);
  const price = 120 + ((r >>> 4) % 880);
  return {
    n,
    seller: r % SELLERS.length,
    item: ITEMS[(r >>> 8) % ITEMS.length] ?? "brass lamp",
    price,
    fee: Math.round(price * 0.02),
    version: 20_670_379 + n * 37,
  };
}

const FIRST = 4;
const SHOWN = 5;

/** The totals after the first `count` sales: what the table holds. */
function totals(count: number): { sold: number; revenue: number }[] {
  const rows = SELLERS.map(() => ({ sold: 0, revenue: 0 }));
  for (let n = 0; n < count; n++) {
    const one = sale(n);
    const row = rows[one.seller];
    if (row) {
      row.sold += 1;
      row.revenue += one.price - one.fee;
    }
  }
  return rows;
}

/** One change event: the row as it stood after sale `n` was folded. */
function change(n: number) {
  const one = sale(n);
  const row = totals(n + 1)[one.seller];
  return {
    n,
    version: one.version,
    seller: SELLERS[one.seller] ?? SELLERS[0]!,
    sold: row?.sold ?? 0,
    revenue: row?.revenue ?? 0,
  };
}

export function Stream() {
  const [count, setCount] = useState(FIRST);

  useEffect(() => {
    if (window.matchMedia("(prefers-reduced-motion: reduce)").matches) return;
    const timer = setInterval(() => setCount((c) => c + 1), 1900);
    return () => clearInterval(timer);
  }, []);

  const sales = Array.from({ length: Math.min(count, SHOWN) }, (_, i) => sale(count - 1 - i));
  const rows = totals(count);
  const newest = sale(count - 1);
  const revenue = rows[newest.seller]?.revenue ?? 0;

  return (
    <div className="grid overflow-hidden rounded-2xl border border-white/10 bg-white/[0.02] lg:grid-cols-[0.95fr_1.05fr_1fr]">
      {/* What the chain hands over. */}
      <div className="border-b border-white/10 p-5 lg:border-r lg:border-b-0">
        <Label title="Events and state" hint="as the chain commits them" />
        <ul className="mt-4 flex flex-col gap-1.5">
          {sales.map((one) => (
            <li
              key={one.n}
              className="arrive flex items-baseline gap-2.5 rounded-lg bg-white/[0.05] px-3 py-2 font-mono text-[11px] sm:text-xs"
            >
              <span className="text-white/35 tabular-nums">
                v{one.version.toLocaleString("en-US")}
              </span>
              <span className="rounded bg-white/10 px-1.5 py-0.5 text-[10px] font-medium text-white/60">
                Sold
              </span>
              <span className="min-w-0 flex-1 truncate text-white/50">{one.item}</span>
              <span className="text-white/75 tabular-nums">{one.price}</span>
            </li>
          ))}
        </ul>
      </div>

      {/* The rule you wrote: the layer Nineveh is, and the only part of this you own. */}
      <div className="border-b border-white/10 bg-blue-500/[0.07] p-5 lg:border-r lg:border-white/10 lg:border-b-0">
        <Label title="Your reducer" hint="market.nineveh.ts" arrow />
        <pre className="mt-4 overflow-x-auto font-mono text-[11px] leading-[1.95] sm:text-xs">
          <div className="text-white/70">
            {"on("}
            <span className="text-blue-300">sold</span>
            {", (s) => {"}
          </div>
          <div className="text-white/45">{"  const row = sellers.row(s.seller)"}</div>
          {/* Keyed on the tick, so the assignment flashes each time a record lands. */}
          <div key={`sold-${count}`} className="settle rounded text-white/70">
            {"  row.sold    += 1"}
          </div>
          <div key={`revenue-${count}`} className="settle rounded text-white/70">
            {"  row.revenue += s.price - s.fee"}
          </div>
          <div className="text-white/70">{"})"}</div>
        </pre>
        <p className="mt-4 truncate font-mono text-[11px] text-white/40">
          <span className="text-clay-400 tabular-nums">
            {newest.price} - {newest.fee}
          </span>{" "}
          → revenue{" "}
          <span className="text-white/75 tabular-nums">{revenue.toLocaleString("en-US")}</span>
        </p>
      </div>

      {/* What your app queries. */}
      <div className="bg-white/[0.04] p-5">
        <Label title="Application state" hint="sellers · key seller" arrow />
        <table className="mt-4 w-full font-mono text-[11px] sm:text-xs">
          <thead>
            <tr className="text-left text-white/35">
              <th className="pb-2 font-medium">seller</th>
              <th className="pb-2 text-right font-medium">sold</th>
              <th className="pb-2 text-right font-medium">revenue</th>
            </tr>
          </thead>
          <tbody>
            {rows.map((row, i) => (
              <tr
                key={SELLERS[i]}
                className={`${i === newest.seller ? "settle" : ""} border-t border-white/[0.07]`}
              >
                <td className="py-1.5 text-white/55">{SELLERS[i]}</td>
                <td className="py-1.5 text-right text-white/55 tabular-nums">{row.sold}</td>
                <td className="py-1.5 text-right font-medium text-white tabular-nums">
                  {row.revenue.toLocaleString("en-US")}
                </td>
              </tr>
            ))}
          </tbody>
        </table>
        <p className="mt-4 text-[11px] text-white/35">One row per seller, queryable over REST.</p>
      </div>
    </div>
  );
}

function Label({ title, hint, arrow = false }: { title: string; hint: string; arrow?: boolean }) {
  return (
    <div className="flex items-baseline justify-between gap-3">
      <span className="flex items-center gap-2 text-xs font-semibold text-white/85">
        {arrow && (
          <span className="text-blue-400" aria-hidden>
            {/* The panes sit in a row on a wide screen and stack on a narrow one, so the
                arrow points the way the eye actually travels. */}
            <span className="lg:hidden">↓</span>
            <span className="hidden lg:inline">→</span>
          </span>
        )}
        {title}
      </span>
      <span className="truncate font-mono text-[11px] text-white/35">{hint}</span>
    </div>
  );
}

/**
 * The change feed, arriving. The section beside this argues that an app is told rather
 * than asks, and a still frame of SSE is a poor way to make that argument: the whole
 * claim is that something happens without the reader doing anything.
 *
 * Older events collapse to a line and the newest is shown whole, which is how a log reads
 * when you're watching one. The frames are the real wire format (`nineveh-realtime`): the
 * id is `version.seq`, which is what makes `Last-Event-ID` enough to resume. The sequence
 * is the same one the hero figure folds, so both drawings are the same market.
 */
export function Feed() {
  const [count, setCount] = useState(FIRST);

  useEffect(() => {
    if (window.matchMedia("(prefers-reduced-motion: reduce)").matches) return;
    const timer = setInterval(() => setCount((c) => c + 1), 1900);
    return () => clearInterval(timer);
  }, []);

  const past = Array.from({ length: 6 }, (_, i) => count - 7 + i)
    .filter((n) => n >= 0)
    .map(change);
  const now = change(count - 1);

  return (
    <div className="overflow-hidden rounded-2xl bg-white/[0.045] ring-1 ring-white/10 backdrop-blur">
      <div className="flex items-center gap-2 border-b border-white/10 px-4 py-2.5">
        <span className="size-2 rounded-full bg-blue-400/70" />
        <span className="font-mono text-[11px] font-medium text-white/40">
          GET /v1/changes?tables=sellers
        </span>
      </div>
      <div className="px-4 py-4 font-mono text-[11.5px] leading-[1.8]">
        {past.map((c) => (
          <div key={c.n} className="flex gap-3 text-white/25">
            <span className="tabular-nums">id: {c.version}.0</span>
            <span>sellers</span>
            <span>update</span>
          </div>
        ))}
        {/* Keyed on the event, so a new frame animates in rather than mutating in place. */}
        <div key={now.n} className="arrive mt-2 border-t border-white/[0.07] pt-2">
          <Field name="id">{`${now.version}.0`}</Field>
          <Field name="event">change</Field>
          <div>
            <span className="text-blue-300">data:</span>
            <span className="text-white/60">{" { "}</span>
            <Pair k="version" v={String(now.version)} />
            <span className="text-white/60">, </span>
            <Pair k="seq" v="0" bare />
          </div>
          <Indented>
            <Pair k="table" v="sellers" />
            <span className="text-white/60">, </span>
            <Pair k="op" v="update" />
          </Indented>
          <Indented>
            <Pair k="key" v={`{ "seller": "${now.seller}" }`} bare />
          </Indented>
          <Indented>
            <Pair k="row" v={`{ "sold": "${now.sold}",`} bare />
          </Indented>
          <Indented deep>
            <span className="text-emerald-300/90">&quot;revenue&quot;</span>
            <span className="text-white/60">: </span>
            <span className="text-emerald-300/90">&quot;{now.revenue}&quot;</span>
            <span className="text-white/60">{" } }"}</span>
          </Indented>
        </div>
      </div>
    </div>
  );
}

/** An SSE field line: `id:`, `event:`. */
function Field({ name, children }: { name: string; children: string }) {
  return (
    <div>
      <span className="text-blue-300">{name}:</span>{" "}
      <span className="text-white/60 tabular-nums">{children}</span>
    </div>
  );
}

/** A continuation line inside the JSON, indented as the wire renders it. */
function Indented({ children, deep = false }: { children: ReactNode; deep?: boolean }) {
  return <div className={deep ? "pl-[9.5ch]" : "pl-[7ch]"}>{children}</div>;
}

/** One JSON pair. `bare` is for a value that is already punctuation, not a string. */
function Pair({ k, v, bare = false }: { k: string; v: string; bare?: boolean }) {
  return (
    <>
      <span className="text-emerald-300/90">&quot;{k}&quot;</span>
      <span className="text-white/60">: </span>
      {bare ? (
        <span className="text-white/60">{v}</span>
      ) : (
        <span className="text-emerald-300/90">&quot;{v}&quot;</span>
      )}
    </>
  );
}
