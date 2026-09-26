"use client";

/**
 * A project's state tables, as cards or as rows.
 *
 * Cards, because a table is what a developer comes here for and a row of name-and-kind
 * is not worth the trip. A card earns its space by carrying the number nothing else on
 * this page says — how many rows are in there — and by going bright when one changes.
 *
 * Rows, because a project gets one table per event, resource and table source before its
 * owner builds a single one of their own, and `/new` warns at forty. Forty cards is a
 * scroll wall where the thing you are scanning for is the one thing a list shows best.
 * So the default follows the count, and whatever you pick after that is remembered.
 */

import Link from "next/link";
import { useCallback, useEffect, useState } from "react";

import type { Table } from "@/lib/api";
import { formatInteger } from "@/lib/format";
import { useFeed } from "@/lib/hooks";
import { useHref } from "@/lib/project";

import { Icon } from "./ui";

type View = "grid" | "list";

/** Above this many tables, cards stop helping and start scrolling. */
const TOO_MANY_FOR_CARDS = 12;
const REMEMBERED = "nineveh.tables.view";
/** How long a table stays lit after one of its rows changes. */
const PULSE = 2000;

function remembered(): View | null {
  try {
    const held = localStorage.getItem(REMEMBERED);
    return held === "grid" || held === "list" ? held : null;
  } catch {
    return null;
  }
}

export function TableList({ tables }: { tables: Table[] }) {
  const href = useHref();
  const [chosen, setChosen] = useState<View | null>(null);
  const [lit, setLit] = useState<Record<string, number>>({});

  // Read on mount, not during render: storage is unreadable in a private window and
  // absent on the server, and either would make the first paint disagree with the HTML.
  useEffect(() => setChosen(remembered()), []);

  const view: View = chosen ?? (tables.length > TOO_MANY_FOR_CARDS ? "list" : "grid");
  const choose = (to: View) => {
    setChosen(to);
    try {
      localStorage.setItem(REMEMBERED, to);
    } catch {
      // A browser that won't remember it still gets to change it.
    }
  };

  const onChanges = useCallback((changes: { table: string }[]) => {
    const at = Date.now();
    setLit((held) => {
      const next = { ...held };
      for (const change of changes) next[change.table] = at;
      return next;
    });
  }, []);
  useFeed({ onChanges });

  // One timer while anything is lit, rather than one per table.
  useEffect(() => {
    if (Object.keys(lit).length === 0) return;
    const timer = setTimeout(() => {
      const cutoff = Date.now() - PULSE;
      setLit((held) => Object.fromEntries(Object.entries(held).filter(([, at]) => at > cutoff)));
    }, PULSE);
    return () => clearTimeout(timer);
  }, [lit]);

  return (
    <section>
      {/* No heading: this list is the Tables page's content, and the page is already
          titled. The count and the view switch are the only chrome it needs. */}
      <div className="flex items-center justify-end gap-3 pb-3">
        <div className="flex items-center gap-3">
          <span className="text-xs text-on-surface-variant">
            {tables.length} table{tables.length === 1 ? "" : "s"}
          </span>
          <div className="flex rounded-sm border border-outline-variant p-0.5">
            {(["grid", "list"] as const).map((v) => (
              <button
                key={v}
                type="button"
                onClick={() => choose(v)}
                aria-pressed={view === v}
                aria-label={v === "grid" ? "Cards" : "List"}
                className={`rounded-[3px] px-1.5 py-0.5 transition-colors ${
                  view === v
                    ? "bg-secondary-container text-on-secondary-container"
                    : "text-on-surface-variant hover:text-on-surface"
                }`}
              >
                <Icon name={v === "grid" ? "grid_view" : "list"} className="block text-[18px]" />
              </button>
            ))}
          </div>
        </div>
      </div>

      {view === "grid" ? (
        <div className="grid gap-3 sm:grid-cols-2 lg:grid-cols-3">
          {tables.map((table) => (
            <TableCard
              key={table.name}
              table={table}
              href={href(`/tables/${encodeURIComponent(table.name)}`)}
              lit={lit[table.name] !== undefined}
            />
          ))}
        </div>
      ) : (
        <ul className="divide-y divide-outline-variant overflow-hidden rounded-md bg-surface-container-low">
          {tables.map((table) => (
            <li key={table.name}>
              <Link
                href={href(`/tables/${encodeURIComponent(table.name)}`)}
                className="group flex items-center gap-4 px-4 py-2.5 text-sm transition-colors hover:bg-on-surface/[0.06]"
              >
                <Pulse lit={lit[table.name] !== undefined} />
                <span className="w-52 truncate font-mono font-medium group-hover:text-primary">
                  {table.name}
                </span>
                <Kind kind={table.kind} />
                <span className="truncate text-xs text-on-surface-variant">
                  key {table.key.join(", ")} · {table.columns.length} columns
                </span>
                <span className="ml-auto shrink-0 font-mono text-xs tabular-nums text-on-surface-variant">
                  {rowsLabel(table)}
                </span>
              </Link>
            </li>
          ))}
        </ul>
      )}
    </section>
  );
}

function TableCard({ table, href, lit }: { table: Table; href: string; lit: boolean }) {
  return (
    <Link
      href={href}
      className="group flex flex-col rounded-md bg-surface-container-low px-4 py-3.5 shadow-e1 transition-all hover:-translate-y-px hover:shadow-e3"
    >
      <div className="flex items-center gap-2">
        <Pulse lit={lit} />
        <span className="min-w-0 flex-1 truncate font-mono text-sm font-medium group-hover:text-primary">
          {table.name}
        </span>
        <Kind kind={table.kind} />
      </div>
      <div className="mt-3 font-mono text-2xl tabular-nums text-on-surface">
        {rowsLabel(table)}
      </div>
      <div className="mt-1.5 truncate text-xs text-on-surface-variant">
        key {table.key.join(", ")} · {table.columns.length} columns
      </div>
    </Link>
  );
}

/** The row count as it should be read: a count plainly, an estimate hedged. */
function rowsLabel(table: Table): string {
  if (!table.rows) return "—";
  const count = formatInteger(table.rows.count);
  return table.rows.exact ? count : `~${count}`;
}

/** Lit while this table is changing, so a busy backend looks like one. */
function Pulse({ lit }: { lit: boolean }) {
  return (
    <span
      aria-hidden
      title={lit ? "changing now" : undefined}
      className={`size-1.5 shrink-0 rounded-full transition-colors ${
        lit ? "bg-primary" : "bg-outline-variant"
      }`}
    />
  );
}

function Kind({ kind }: { kind: string }) {
  const tones: Record<string, string> = {
    reduce: "bg-secondary-container text-primary",
    mirror: "bg-tertiary-container text-on-tertiary-container",
    log: "bg-surface-container-high text-on-surface-variant",
  };
  return (
    <span
      className={`w-16 shrink-0 rounded px-1.5 py-0.5 text-center text-[11px] font-medium ${tones[kind] ?? ""}`}
    >
      {kind}
    </span>
  );
}
