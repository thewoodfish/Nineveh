"use client";

import { useCallback, useEffect, useRef, useState } from "react";

import { type Change, type Row, type RowsQuery, type Table, getRows, rowKey } from "@/lib/api";
import { formatInteger } from "@/lib/format";
import { useFeed } from "@/lib/hooks";
import { useProject } from "@/lib/project";

import { Button, Cell, Notice, isNumeric } from "./ui";

const PAGE = 50;

type Order = { column: string; desc: boolean } | undefined;

/**
 * A state table as a grid that updates live. In the default view (newest changes
 * first, first page, no filters) changes from the feed are merged in as they commit;
 * in any other view, rows on screen update in place and a count of new changes
 * offers a refresh.
 */
export function DataGrid({
  table,
  onCount,
  action,
}: {
  table: Table;
  /** The table's row count, which the grid learns by asking and the header wants to show. */
  onCount?: (count: number | null) => void;
  /** What sits above the table on the right, such as the button that edits it. */
  action?: React.ReactNode;
}) {
  const { base } = useProject();
  const [rows, setRows] = useState<Row[]>([]);
  const [count, setCount] = useState<number | null>(null);
  const [offset, setOffset] = useState(0);
  const [order, setOrder] = useState<Order>(undefined);
  const [filters, setFilters] = useState<Record<string, string>>({});
  const [draft, setDraft] = useState<Record<string, string>>({});
  const [error, setError] = useState<string | null>(null);
  const [loading, setLoading] = useState(true);
  const [flashes, setFlashes] = useState<Map<string, number>>(new Map());
  const [missed, setMissed] = useState(0);

  const columns = table.columns;
  const defaultView =
    offset === 0 && order === undefined && Object.values(filters).every((v) => v === "");

  const load = useCallback(async () => {
    if (!base) return;
    const query: RowsQuery = {
      limit: PAGE,
      offset,
      order,
      filters,
      count: true,
    };
    try {
      const page = await getRows(base, table.name, query);
      setRows(page.rows);
      setCount(page.count);
      setError(null);
      setMissed(0);
    } catch (e) {
      setError(e instanceof Error ? e.message : String(e));
    } finally {
      setLoading(false);
    }
  }, [base, table.name, offset, order, filters]);

  useEffect(() => {
    void load();
  }, [load]);

  // The feed's handler reads the current view and rows without re-subscribing.
  const view = useRef({ defaultView, rows });
  view.current = { defaultView, rows };

  const reload = useRef(load);
  reload.current = load;

  const onChanges = useCallback(
    (batch: Change[], dropped: number) => {
      // More changed than the feed held for us: the page is out of date, so read it again.
      if (dropped > 0) {
        void reload.current();
        return;
      }
      const latest = new Map<string, Change>();
      for (const change of batch) latest.set(rowKey(table, change.key), change);
      const rowOf = (change: Change): Row | null =>
        change.row ? { _version: change.version, ...change.row } : null;
      if (view.current.defaultView) {
        setRows((current) => {
          const rest = current.filter((r) => !latest.has(rowKey(table, r)));
          const fresh = [...latest.values()].reverse().flatMap((c): Row[] => {
            const row = rowOf(c);
            return row ? [row] : [];
          });
          return [...fresh, ...rest].slice(0, PAGE);
        });
        const delta = batch.reduce(
          (n, c) => n + (c.op === "insert" ? 1 : c.op === "delete" ? -1 : 0),
          0,
        );
        setCount((c) => (c === null ? c : c + delta));
      } else {
        const shown = new Set(view.current.rows.map((r) => rowKey(table, r)));
        const here = [...latest.keys()].filter((k) => shown.has(k));
        if (here.length > 0) {
          setRows((current) =>
            current.flatMap((r) => {
              const change = latest.get(rowKey(table, r));
              if (!change) return [r];
              const row = rowOf(change);
              return row ? [row] : [];
            }),
          );
        }
        if (here.length < latest.size) setMissed((m) => m + latest.size - here.length);
      }
      const now = Date.now();
      setFlashes((current) => {
        const next = new Map(current);
        for (const [key, change] of latest) if (change.row) next.set(key, now);
        return next;
      });
    },
    [table],
  );
  const connected = useFeed({
    tables: [table.name],
    onChanges,
    onReset: () => void reload.current(),
  });

  // Forget flashes once they've played.
  useEffect(() => {
    if (flashes.size === 0) return;
    const timer = setTimeout(() => {
      const now = Date.now();
      setFlashes((current) => new Map([...current].filter(([, at]) => now - at < 1600)));
    }, 1700);
    return () => clearTimeout(timer);
  }, [flashes]);

  const sortBy = (column: string) => {
    setOffset(0);
    setOrder((current) => {
      if (current?.column !== column) return { column, desc: false };
      if (!current.desc) return { column, desc: true };
      return undefined;
    });
  };

  const applyFilters = () => {
    const same = columns.every((c) => (draft[c.name] ?? "") === (filters[c.name] ?? ""));
    if (same) return;
    setOffset(0);
    setFilters({ ...draft });
  };

  const filtered = Object.values(filters).some((v) => v !== "");

  // Unfiltered is the table's size; filtered is the size of a question about it, which
  // is not what a header saying "1,204 rows" means.
  const report = useRef(onCount);
  report.current = onCount;
  useEffect(() => {
    report.current?.(filtered ? null : count);
  }, [count, filtered]);
  const last = count === null ? offset + rows.length : Math.min(offset + PAGE, count);

  const stale = missed > 0 && (
    <button
      type="button"
      onClick={() => void load()}
      className="rounded-full bg-secondary-container px-2.5 py-0.5 text-xs font-medium text-primary hover:bg-secondary-container"
    >
      {missed} new {missed === 1 ? "change" : "changes"} · refresh
    </button>
  );

  return (
    /* A table on a page, not a grid stretched to the window. It is bounded, so on a wide
       screen the rows stop rather than running to the edge, and it is as tall as it is
       rather than pinning a footer to the bottom of the viewport. */
    <div className="max-w-6xl px-8 py-6">
      {/* Toolbar and table share a shrink-to-fit column, so the button lands flush with
          the table's right edge instead of out at the bound the table never reaches. */}
      <div className="w-fit max-w-full">
      <div className="mb-3 flex min-h-9 items-center justify-end gap-3">
        {stale}
        {action}
      </div>

      {error && (
        <div className="mb-3">
          {/* A config change rebuilds the tables beside the served ones (ADR 0016):
              that's work in progress, not a failure. */}
          {error.includes("being rebuilt") ? (
            <Notice tone="neutral" title="Building this table">
              Nineveh is folding the project&apos;s history into its new tables. The rows appear
              when it catches up; the Overview shows how far along it is.
            </Notice>
          ) : (
            <Notice tone="error" title="Couldn't load rows">
              {error}
            </Notice>
          )}
        </div>
      )}

      <div className="overflow-hidden rounded-md border border-outline-variant bg-surface-container-low">
        {/* Only this scrolls sideways, so the footer under it stays put. The head isn't
            sticky any more: there is no tall scroller for it to stick inside. */}
        <div className="overflow-x-auto">
        <table className="w-auto border-separate border-spacing-0 text-sm">
          <thead className="bg-surface-container-low">
            <tr>
              {columns.map((column) => (
                <th
                  key={column.name}
                  className={`border-b border-outline-variant px-3 py-2 font-medium whitespace-nowrap ${
                    isNumeric(column.type) ? "text-right" : "text-left"
                  }`}
                >
                  <button
                    type="button"
                    onClick={() => sortBy(column.name)}
                    className="inline-flex items-center gap-1 hover:text-primary"
                  >
                    <span className="font-mono text-[13px]">{column.name}</span>
                    {table.key.includes(column.name) && (
                      <span
                        className="rounded bg-primary/20 px-1 py-px text-[9px] font-semibold text-primary"
                        title="key column"
                      >
                        KEY
                      </span>
                    )}
                    <span className="font-mono text-[11px] font-normal text-on-surface-variant">
                      {column.type}
                    </span>
                    {order?.column === column.name && (
                      <span className="text-xs">{order.desc ? "↓" : "↑"}</span>
                    )}
                  </button>
                </th>
              ))}
              <th className="border-b border-outline-variant px-3 py-2 text-right font-medium whitespace-nowrap">
                <button
                  type="button"
                  onClick={() => sortBy("_version")}
                  className="inline-flex items-center gap-1 text-[13px] text-on-surface-variant hover:text-primary"
                  title="Version of the row's last change"
                >
                  version{" "}
                  {order?.column === "_version" ? (order.desc ? "↓" : "↑") : order ? "" : "↓"}
                </button>
              </th>
              <th className="border-b border-outline-variant" />
            </tr>
            <tr>
              {columns.map((column) => (
                <th
                  key={column.name}
                  className="border-b border-outline-variant bg-surface-container px-2 py-1"
                >
                  {column.type !== "json" && (
                    <input
                      value={draft[column.name] ?? ""}
                      onChange={(e) => setDraft({ ...draft, [column.name]: e.target.value })}
                      onKeyDown={(e) => e.key === "Enter" && applyFilters()}
                      onBlur={applyFilters}
                      placeholder="filter ="
                      className="w-full min-w-28 rounded-sm border border-outline-variant bg-surface-container-high px-2 py-1 font-mono text-xs font-normal text-on-surface transition-colors placeholder:text-on-surface-variant/50 hover:border-outline focus:border-primary focus:ring-1 focus:ring-primary focus:outline-none"
                    />
                  )}
                </th>
              ))}
              <th className="border-b border-outline-variant bg-surface-container" />
              <th className="border-b border-outline-variant bg-surface-container" />
            </tr>
          </thead>
          <tbody>
            {rows.map((row) => {
              const key = rowKey(table, row);
              return (
                <tr
                  key={`${key}:${flashes.get(key) ?? 0}`}
                  className={`transition-colors hover:bg-on-surface/[0.06] ${flashes.has(key) ? "flash" : ""}`}
                >
                  {columns.map((column) => (
                    <td
                      key={column.name}
                      className={`max-w-xs truncate border-b border-outline-variant px-3 py-1.5 whitespace-nowrap ${
                        isNumeric(column.type) ? "text-right" : ""
                      }`}
                    >
                      <Cell type={column.type} value={row[column.name]} />
                    </td>
                  ))}
                  <td className="border-b border-outline-variant px-3 py-1.5 text-right text-xs whitespace-nowrap">
                    <Cell type="version" value={row._version} />
                  </td>
                  <td className="border-b border-outline-variant" />
                </tr>
              );
            })}
          </tbody>
        </table>
        {!loading && rows.length === 0 && !error && (
          <div className="mx-auto max-w-sm px-8 py-20 text-center">
            <p className="text-sm font-medium">
              {filtered ? "Nothing matches these filters" : "No rows yet"}
            </p>
            <p className="mt-1 text-sm text-on-surface-variant text-pretty">
              {filtered
                ? "Filters match a column exactly."
                : table.kind === "reduce"
                  ? "Rows appear as records reach this table's rules."
                  : "Rows appear as the chain is folded into this table."}
            </p>
            {filtered && (
              <Button
                className="mt-4"
                onClick={() => {
                  setDraft({});
                  setFilters({});
                  setOffset(0);
                }}
              >
                Clear filters
              </Button>
            )}
          </div>
        )}
        </div>

      <footer className="flex items-center justify-between border-t border-outline-variant px-4 py-2.5 text-xs text-on-surface-variant">
        <span className="tabular-nums">
          {rows.length === 0 ? "0 rows" : `${formatInteger(offset + 1)}–${formatInteger(last)}`}
          {count !== null && ` of ${formatInteger(count)}`}
        </span>
        <div className="flex gap-2">
          <PageButton disabled={offset === 0} onClick={() => setOffset(Math.max(0, offset - PAGE))}>
            Previous
          </PageButton>
          <PageButton
            disabled={count !== null ? offset + PAGE >= count : rows.length < PAGE}
            onClick={() => setOffset(offset + PAGE)}
          >
            Next
          </PageButton>
        </div>
      </footer>
      </div>
      </div>
    </div>
  );
}

function PageButton({
  disabled,
  onClick,
  children,
}: {
  disabled: boolean;
  onClick: () => void;
  children: React.ReactNode;
}) {
  return (
    <button
      type="button"
      disabled={disabled}
      onClick={onClick}
      className="rounded-sm border border-outline-variant px-2.5 py-1 font-medium text-on-surface enabled:hover:bg-surface-container-high disabled:opacity-40"
    >
      {children}
    </button>
  );
}
