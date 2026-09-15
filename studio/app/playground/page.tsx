"use client";

import { useEffect, useState } from "react";

import { Card, PageHeader } from "@/components/ui";
import { API_URL, getPath, rowsPath } from "@/lib/api";
import { useTables } from "@/lib/hooks";

/** Build a request against your own state, see it as a URL and curl, and run it. */
export default function Playground() {
  const { tables } = useTables();
  const [table, setTable] = useState("");
  const [filters, setFilters] = useState<{ column: string; value: string }[]>([]);
  const [order, setOrder] = useState("");
  const [desc, setDesc] = useState(true);
  const [limit, setLimit] = useState(10);
  const [result, setResult] = useState<{ status: "ok" | "error"; body: string; ms: number } | null>(null);
  const [running, setRunning] = useState(false);

  useEffect(() => {
    if (!table && tables?.[0]) setTable(tables[0].name);
  }, [tables, table]);

  const current = tables?.find((t) => t.name === table);
  const path = current
    ? rowsPath(current.name, {
        limit,
        offset: 0,
        order: order ? { column: order, desc } : undefined,
        filters: Object.fromEntries(filters.filter((f) => f.column).map((f) => [f.column, f.value])),
        count: true,
      })
    : "/v1/tables";
  const url = `${API_URL}${path}`;

  const run = async () => {
    setRunning(true);
    const started = performance.now();
    try {
      const body = await getPath(path);
      setResult({ status: "ok", body: JSON.stringify(body, null, 2), ms: performance.now() - started });
    } catch (e) {
      setResult({ status: "error", body: e instanceof Error ? e.message : String(e), ms: performance.now() - started });
    } finally {
      setRunning(false);
    }
  };

  const field =
    "rounded-md border border-zinc-200 bg-white px-2.5 py-1.5 text-sm focus:border-lapis-400 focus:outline-none dark:border-zinc-700 dark:bg-zinc-900";

  return (
    <div>
      <PageHeader title="API playground" />
      <div className="mx-auto grid max-w-6xl gap-6 px-8 py-6 lg:grid-cols-[22rem_1fr]">
        <Card className="flex flex-col gap-4 p-4">
          <label className="flex flex-col gap-1.5 text-xs font-medium text-zinc-500">
            Table
            <select value={table} onChange={(e) => { setTable(e.target.value); setFilters([]); setOrder(""); }} className={`${field} font-mono`}>
              {tables?.map((t) => (
                <option key={t.name} value={t.name}>
                  {t.name}
                </option>
              ))}
            </select>
          </label>

          <div className="flex flex-col gap-1.5 text-xs font-medium text-zinc-500">
            Filters
            {filters.map((f, i) => (
              <div key={i} className="flex gap-1.5">
                <select
                  value={f.column}
                  onChange={(e) => setFilters(filters.map((g, j) => (j === i ? { ...g, column: e.target.value } : g)))}
                  className={`${field} w-32 font-mono`}
                >
                  {current?.columns
                    .filter((c) => c.type !== "json")
                    .map((c) => (
                      <option key={c.name} value={c.name}>
                        {c.name}
                      </option>
                    ))}
                </select>
                <input
                  value={f.value}
                  onChange={(e) => setFilters(filters.map((g, j) => (j === i ? { ...g, value: e.target.value } : g)))}
                  placeholder="equals"
                  className={`${field} min-w-0 flex-1 font-mono`}
                />
                <button
                  type="button"
                  onClick={() => setFilters(filters.filter((_, j) => j !== i))}
                  className="px-1 text-zinc-400 hover:text-red-500"
                  aria-label="Remove filter"
                >
                  ×
                </button>
              </div>
            ))}
            <button
              type="button"
              onClick={() => setFilters([...filters, { column: current?.columns[0]?.name ?? "", value: "" }])}
              className="self-start text-xs font-medium text-lapis-600 hover:underline dark:text-lapis-400"
            >
              + Add filter
            </button>
          </div>

          <div className="flex gap-3">
            <label className="flex flex-1 flex-col gap-1.5 text-xs font-medium text-zinc-500">
              Order by
              <select value={order} onChange={(e) => setOrder(e.target.value)} className={`${field} font-mono`}>
                <option value="">newest change</option>
                <option value="_version">_version</option>
                {current?.columns.map((c) => (
                  <option key={c.name} value={c.name}>
                    {c.name}
                  </option>
                ))}
              </select>
            </label>
            <label className="flex flex-col gap-1.5 text-xs font-medium text-zinc-500">
              Limit
              <input
                type="number"
                min={1}
                max={1000}
                value={limit}
                onChange={(e) => setLimit(Math.max(1, Math.min(1000, Number(e.target.value) || 1)))}
                className={`${field} w-20`}
              />
            </label>
          </div>
          {order && (
            <label className="flex items-center gap-2 text-xs text-zinc-500">
              <input type="checkbox" checked={desc} onChange={(e) => setDesc(e.target.checked)} />
              Descending
            </label>
          )}

          <button
            type="button"
            onClick={() => void run()}
            disabled={running}
            className="mt-1 rounded-md bg-zinc-900 px-3 py-2 text-sm font-medium text-white hover:bg-zinc-700 disabled:opacity-60 dark:bg-white dark:text-zinc-900 dark:hover:bg-zinc-200"
          >
            {running ? "Running…" : "Run"}
          </button>
        </Card>

        <div className="flex min-w-0 flex-col gap-4">
          <Card className="p-4">
            <div className="text-xs font-medium text-zinc-500">GET</div>
            <div className="mt-1 font-mono text-sm break-all">{url}</div>
            <div className="mt-3 text-xs font-medium text-zinc-500">curl</div>
            <pre className="mt-1 overflow-x-auto rounded-lg bg-zinc-900 px-3 py-2 font-mono text-xs text-zinc-100">
              {`curl '${url}'`}
            </pre>
          </Card>
          <Card className="min-h-64 p-4">
            <div className="flex items-center justify-between text-xs text-zinc-500">
              <span className="font-medium">Response</span>
              {result && (
                <span className={result.status === "ok" ? "text-emerald-600" : "text-red-600"}>
                  {result.status === "ok" ? "200" : "error"} · {result.ms.toFixed(0)} ms
                </span>
              )}
            </div>
            <pre className="mt-2 max-h-[60vh] overflow-auto font-mono text-xs leading-relaxed">
              {result?.body ?? "Run the request to see your state."}
            </pre>
          </Card>
        </div>
      </div>
    </div>
  );
}
