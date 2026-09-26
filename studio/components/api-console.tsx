"use client";

/**
 * Build a request against a project's own state, see it as a URL, a curl and the two
 * lines an app would actually use, and run it.
 *
 * Shared by the API playground and a table's API tab. The two differ by one thing —
 * whether the table is yours to choose or already decided — so they differ by one prop
 * rather than by two copies of a request builder that would drift.
 */

import { useState } from "react";

import { type Table, getPath, rowsPath } from "@/lib/api";
import { useProject } from "@/lib/project";

import { Button, Card, Select, field } from "./ui";

type Result = { status: "ok" | "error"; body: string; ms: number };

export function ApiConsole({
  table,
  pick,
}: {
  table: Table;
  /** Render a table picker. Omitted where the table is already the subject of the page. */
  pick?: { tables: Table[]; onPick: (name: string) => void };
}) {
  const { base, hosted } = useProject();
  const [filters, setFilters] = useState<{ column: string; value: string }[]>([]);
  const [order, setOrder] = useState("");
  const [desc, setDesc] = useState(true);
  const [limit, setLimit] = useState(10);
  const [result, setResult] = useState<Result | null>(null);
  const [running, setRunning] = useState(false);
  const [copied, setCopied] = useState("");

  const reset = () => {
    setFilters([]);
    setOrder("");
    setResult(null);
  };

  const path = rowsPath(table.name, {
    limit,
    offset: 0,
    order: order ? { column: order, desc } : undefined,
    filters: Object.fromEntries(filters.filter((f) => f.column).map((f) => [f.column, f.value])),
    count: true,
  });
  const url = `${base ?? ""}${path}`;
  const feed = `${base ?? ""}/v1/changes?tables=${encodeURIComponent(table.name)}`;

  const copy = (what: string, text: string) => {
    void navigator.clipboard?.writeText(text);
    setCopied(what);
    setTimeout(() => setCopied(""), 1500);
  };

  const run = async () => {
    setRunning(true);
    const started = performance.now();
    try {
      if (!base) throw new Error("Open a project first");
      const body = await getPath(base, path);
      setResult({ status: "ok", body: JSON.stringify(body, null, 2), ms: performance.now() - started });
    } catch (e) {
      setResult({
        status: "error",
        body: e instanceof Error ? e.message : String(e),
        ms: performance.now() - started,
      });
    } finally {
      setRunning(false);
    }
  };

  return (
    <div className="grid min-h-0 max-w-7xl flex-1 gap-6 lg:grid-cols-[22rem_1fr]">
      <Card className="flex h-fit flex-col gap-4 p-4">
        {pick && (
          <label className="flex flex-col gap-1.5 text-xs font-medium text-on-surface-variant">
            Table
            <Select
              value={table.name}
              onChange={(e) => {
                reset();
                pick.onPick(e.target.value);
              }}
              className="font-mono"
            >
              {pick.tables.map((t) => (
                <option key={t.name} value={t.name}>
                  {t.name}
                </option>
              ))}
            </Select>
          </label>
        )}

        <div className="flex flex-col gap-1.5 text-xs font-medium text-on-surface-variant">
          Filters
          {filters.map((f, i) => (
            <div key={i} className="flex gap-1.5">
              <Select
                value={f.column}
                onChange={(e) =>
                  setFilters(filters.map((g, j) => (j === i ? { ...g, column: e.target.value } : g)))
                }
                className={`${field} w-32 font-mono`}
              >
                {table.columns
                  .filter((c) => c.type !== "json")
                  .map((c) => (
                    <option key={c.name} value={c.name}>
                      {c.name}
                    </option>
                  ))}
              </Select>
              <input
                value={f.value}
                onChange={(e) =>
                  setFilters(filters.map((g, j) => (j === i ? { ...g, value: e.target.value } : g)))
                }
                placeholder="equals"
                className={`${field} min-w-0 flex-1 font-mono`}
              />
              <button
                type="button"
                onClick={() => setFilters(filters.filter((_, j) => j !== i))}
                className="px-1 text-on-surface-variant hover:text-on-error-container"
                aria-label="Remove filter"
              >
                ×
              </button>
            </div>
          ))}
          <button
            type="button"
            onClick={() => setFilters([...filters, { column: table.columns[0]?.name ?? "", value: "" }])}
            className="self-start text-xs font-medium text-primary hover:underline"
          >
            + Add filter
          </button>
        </div>

        <div className="flex gap-3">
          <label className="flex flex-1 flex-col gap-1.5 text-xs font-medium text-on-surface-variant">
            Order by
            <Select value={order} onChange={(e) => setOrder(e.target.value)} className="font-mono">
              <option value="">newest change</option>
              <option value="_version">_version</option>
              {table.columns.map((c) => (
                <option key={c.name} value={c.name}>
                  {c.name}
                </option>
              ))}
            </Select>
          </label>
          <label className="flex flex-col gap-1.5 text-xs font-medium text-on-surface-variant">
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
          <label className="flex items-center gap-2 text-xs text-on-surface-variant">
            <input type="checkbox" checked={desc} onChange={(e) => setDesc(e.target.checked)} />
            Descending
          </label>
        )}

        <Button tone="primary" size="lg" className="mt-1" onClick={() => void run()} disabled={running}>
          {running ? "Running…" : "Run request"}
        </Button>
      </Card>

      <div className="flex min-h-0 min-w-0 flex-col gap-4">
        <Card className="shrink-0 p-4">
          <div className="flex items-center gap-2">
            <span className="rounded bg-tertiary-container px-1.5 py-0.5 font-mono text-[11px] font-medium text-on-tertiary-container">
              GET
            </span>
            <span className="min-w-0 flex-1 truncate font-mono text-sm" title={url}>
              {url}
            </span>
            <Button onClick={() => copy("url", url)}>{copied === "url" ? "Copied" : "Copy"}</Button>
          </div>
          <Snippet
            label="curl"
            copied={copied === "curl"}
            onCopy={(text) => copy("curl", text)}
            code={hosted ? `curl -H 'Authorization: Bearer nvk_…' \\\n  '${url}'` : `curl '${url}'`}
          />
        </Card>

        <Card className="shrink-0 p-4">
          <div className="text-[11px] font-medium tracking-[0.08em] text-on-surface-variant uppercase">
            From your app
          </div>
          <Snippet
            label="fetch"
            copied={copied === "fetch"}
            onCopy={(text) => copy("fetch", text)}
            code={
              hosted
                ? `const res = await fetch('${url}', {\n  headers: { Authorization: 'Bearer nvk_…' },\n})\nconst { rows } = await res.json()`
                : `const res = await fetch('${url}')\nconst { rows } = await res.json()`
            }
          />
          <Snippet
            label={`live: every change to ${table.name}`}
            copied={copied === "feed"}
            onCopy={(text) => copy("feed", text)}
            code={
              hosted
                ? `const feed = new EventSource('${feed}&apikey=nvk_…')\nfeed.addEventListener('change', (e) => {\n  const { op, key, row } = JSON.parse(e.data)\n})`
                : `const feed = new EventSource('${feed}')\nfeed.addEventListener('change', (e) => {\n  const { op, key, row } = JSON.parse(e.data)\n})`
            }
          />
          <p className="mt-2 text-[11px] leading-relaxed text-on-surface-variant">
            {hosted ? (
              <>
                The key is in the feed&apos;s URL because <span className="font-mono">EventSource</span>{" "}
                can&apos;t send headers. Everything else takes it as a header — prefer that, and keep
                the URL form out of anything that logs URLs.
              </>
            ) : (
              <>
                Local mode is loopback-only and takes no key. A hosted plane wants one of the
                project&apos;s API keys on every request.
              </>
            )}
          </p>
        </Card>

        <Card className="flex min-h-96 flex-1 flex-col overflow-hidden">
          <div className="flex items-center justify-between border-b border-outline-variant bg-surface-container-high px-4 py-2 text-xs">
            <span className="font-semibold tracking-wide text-on-surface-variant uppercase">
              Response
            </span>
            {result && (
              <span
                className={`font-mono ${result.status === "ok" ? "text-on-tertiary-container" : "text-error"}`}
              >
                {result.status === "ok" ? "200 OK" : "error"} · {result.ms.toFixed(0)} ms
              </span>
            )}
          </div>
          {result ? (
            <pre className="min-h-0 flex-1 overflow-auto px-4 py-3 font-mono text-xs leading-relaxed">
              {result.body}
            </pre>
          ) : (
            <p className="flex flex-1 items-center justify-center px-4 py-10 text-center text-sm text-on-surface-variant">
              Run the request to see your state.
            </p>
          )}
        </Card>
      </div>
    </div>
  );
}

function Snippet({
  label,
  code,
  copied,
  onCopy,
}: {
  label: string;
  code: string;
  copied: boolean;
  onCopy: (code: string) => void;
}) {
  return (
    <>
      <div className="mt-3 flex items-baseline justify-between gap-3">
        <span className="text-[11px] font-medium tracking-[0.08em] text-on-surface-variant uppercase">
          {label}
        </span>
        <button
          type="button"
          onClick={() => onCopy(code)}
          className="text-[11px] font-medium text-primary hover:underline"
        >
          {copied ? "Copied" : "Copy"}
        </button>
      </div>
      <pre className="mt-1.5 overflow-x-auto rounded-sm bg-surface-container-high px-3 py-2.5 font-mono text-xs leading-relaxed text-on-surface">
        {code}
      </pre>
    </>
  );
}
