"use client";

// The product, in one picture: records arrive from the chain on the left, and the
// table on the right is what they fold into. Nothing here is real — it's a drawing of
// what Nineveh does, driven by a seeded sequence so the server and the browser agree
// on the first frame.

import { useEffect, useState } from "react";

const SELLERS = ["0x7a3f…c41d", "0x1e87…8d2a", "0x9b02…4f77", "0x6146…e554"];
const ITEMS = ["brass lamp", "oak chair", "wool rug", "clay mug", "iron kettle"];

type Sale = { n: number; seller: number; item: string; price: number; fee: number; version: number };

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

  return (
    <div className="relative grid gap-px overflow-hidden rounded-2xl bg-white/10 sm:grid-cols-2">
      <Pane title="From the chain" hint="events, as they commit" side="left">
        <ul className="flex flex-col gap-1.5">
          {sales.map((one) => (
            <li
              key={one.n}
              className="arrive flex items-baseline gap-2.5 rounded-lg bg-white/[0.04] px-3 py-2 font-mono text-[11px] sm:text-xs"
            >
              <span className="text-white/35 tabular-nums">v{one.version.toLocaleString("en-US")}</span>
              <span className="rounded bg-blue-500/20 px-1.5 py-0.5 text-[10px] font-medium text-blue-200">
                Sold
              </span>
              <span className="min-w-0 flex-1 truncate text-white/60">{one.item}</span>
              <span className="text-white/80 tabular-nums">{one.price}</span>
            </li>
          ))}
        </ul>
      </Pane>

      <Pane title="Your table" hint="sellers · key seller" side="right">
        <table className="w-full font-mono text-[11px] sm:text-xs">
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
                className={`${i === newest.seller ? "settle" : ""} border-t border-white/[0.06]`}
              >
                <td className="py-1.5 text-white/70">{SELLERS[i]}</td>
                <td className="py-1.5 text-right text-white/80 tabular-nums">{row.sold}</td>
                <td className="py-1.5 text-right font-medium text-white tabular-nums">
                  {row.revenue.toLocaleString("en-US")}
                </td>
              </tr>
            ))}
          </tbody>
        </table>
        <p className="mt-4 text-[11px] text-white/35">
          revenue = price − fee, folded from every sale
        </p>
      </Pane>
    </div>
  );
}

function Pane({
  title,
  hint,
  side,
  children,
}: {
  title: string;
  hint: string;
  side: "left" | "right";
  children: React.ReactNode;
}) {
  return (
    <div className="relative bg-ink-900/90 p-4 backdrop-blur sm:p-5">
      {/* A thread of light along the top edge, so the card reads as glass. */}
      <div className="absolute inset-x-0 top-0 h-px bg-gradient-to-r from-transparent via-white/15 to-transparent" />
      <div className="mb-3 flex items-baseline justify-between gap-3">
        <span className="flex items-center gap-2 text-xs font-medium text-white/70">
          {side === "right" && (
            <span className="text-blue-400/70" aria-hidden>
              {/* The panes sit side by side on a wide screen and stack on a narrow one,
                  so the arrow has to point the way the eye actually travels. */}
              <span className="sm:hidden">↓</span>
              <span className="hidden sm:inline">→</span>
            </span>
          )}
          {title}
        </span>
        <span className="truncate font-mono text-[11px] text-white/30">{hint}</span>
      </div>
      {children}
    </div>
  );
}
