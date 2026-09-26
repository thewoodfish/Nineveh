"use client";

/**
 * One table's world.
 *
 * Studio's navigation is arranged by verb — browse, watch, query, edit — while the
 * questions a developer actually has are all about one table: what's in it, is it still
 * moving, what wrote this row, how do I read it from my app. Answering them used to mean
 * four pages, three of which made you pick the table again. They're tabs here instead.
 *
 * Every panel is built from endpoints that already existed; nothing was added to the API
 * for this page. What a tab shows depends on the table's kind, because the kinds aren't
 * the same thing: only a `reduce` table has rules, and a `mirror` or a `log` is better
 * explained by the source it follows than by a rules panel that would always be empty.
 */

import Link from "next/link";
import { useParams, useRouter, useSearchParams } from "next/navigation";
import { Suspense, useCallback, useEffect, useMemo, useState } from "react";

import { ApiConsole } from "@/components/api-console";
import { DataGrid } from "@/components/data-grid";
import { PageHeader } from "@/components/page-header";
import { SourceSchema } from "@/components/source-schema";
import { Button, Card, Live, Notice, Offline } from "@/components/ui";
import { ApiError, type Change, type Table, control, getRows } from "@/lib/api";
import { definitionOf } from "@/lib/definition";
import { formatInteger } from "@/lib/format";
import { useFeed, useSources, useTables } from "@/lib/hooks";
import { useHref, useProject } from "@/lib/project";
import { dslTableBlock, foreignWrites, reducersFile } from "@/lib/state-table";

/** How long the header keeps saying a change just landed. */
const PULSE = 4000;

const TABS = ["data", "definition", "schema", "api", "changes"] as const;
type Tab = (typeof TABS)[number];

const LABEL: Record<Tab, string> = {
  data: "Data",
  definition: "Definition",
  schema: "Schema",
  api: "API",
  changes: "Changes",
};

export default function TablePage() {
  return (
    <Suspense>
      <TableWorld />
    </Suspense>
  );
}

function TableWorld() {
  const raw = useParams<{ name: string }>().name;
  const name = decodeURIComponent(Array.isArray(raw) ? (raw[0] ?? "") : raw);
  const { tables, error } = useTables();
  const params = useSearchParams();
  const router = useRouter();
  const href = useHref();

  const tab = (TABS as readonly string[]).includes(params.get("tab") ?? "")
    ? (params.get("tab") as Tab)
    : "data";
  // The tab is in the URL so a table's rules can be linked to, and so the back button
  // walks the tabs the way it walks anything else.
  const go = (to: Tab) => {
    const next = new URLSearchParams(params.toString());
    if (to === "data") next.delete("tab");
    else next.set("tab", to);
    router.replace(`?${next.toString()}`, { scroll: false });
  };

  const table = tables?.find((t) => t.name === name);

  if (error && !tables) return <Offline error={error} />;
  if (!tables) return null;
  if (!table) {
    return (
      <div className="px-8 py-10">
        <Notice tone="error" title={`No table called ${name}`}>
          This project serves {tables.length} table{tables.length === 1 ? "" : "s"}.{" "}
          <Link href={href("/")} className="text-primary hover:underline">
            Back to the overview
          </Link>
          .
        </Notice>
      </div>
    );
  }

  return <Inner key={table.name} table={table} tab={tab} go={go} />;
}

function Inner({ table, tab, go }: { table: Table; tab: Tab; go: (to: Tab) => void }) {
  const { name: project, mode, base } = useProject();
  const [count, setCount] = useState<number | null>(null);
  const [changed, setChanged] = useState(false);

  // The header says the same thing on every tab, so the count can't come only from the
  // grid — land on Definition and there would be no number at all. One cheap ask for it
  // here; the grid overwrites it with the live figure while the Data tab is open.
  useEffect(() => {
    if (!base) return;
    let alive = true;
    getRows(base, table.name, { limit: 1, offset: 0, filters: {}, count: true })
      .then((page) => alive && page.count !== null && setCount(page.count))
      .catch(() => {});
    return () => {
      alive = false;
    };
  }, [base, table.name]);

  // One filtered subscription for the header's pulse. The feed is shared per project,
  // so this costs a callback, not a connection. It has to fade: a table that changed
  // once an hour ago should not still be claiming it just did.
  const onChanges = useCallback(() => setChanged(true), []);
  const connected = useFeed({ tables: [table.name], onChanges });
  useEffect(() => {
    if (!changed) return;
    const timer = setTimeout(() => setChanged(false), PULSE);
    return () => clearTimeout(timer);
  }, [changed]);

  const onCount = useCallback((n: number | null) => setCount(n), []);

  return (
    <div className="flex h-full min-h-0 flex-col">
      <PageHeader
        title={<span className="font-mono">{table.name}</span>}
        hint={
          <span className="text-xs">
            {table.kind} · key{" "}
            <span className="font-mono text-on-surface-variant">{table.key.join(", ")}</span>
            {count !== null && ` · ${formatInteger(count)} rows`}
            {changed && " · just changed"}
          </span>
        }
      >
        {table.kind === "reduce" && mode === "control" && project && (
          <Link
            href={`/state?project=${encodeURIComponent(project)}&table=${encodeURIComponent(table.name)}`}
            className="text-xs font-medium text-primary hover:underline"
          >
            Edit rules
          </Link>
        )}
        <Live connected={connected} />
      </PageHeader>

      {/* The tabs belong to the header above them, so they carry its surface: the band
          along the top is one thing, and the canvas starts under it. */}
      <div className="flex gap-1 border-b border-outline-variant bg-surface-container-low px-8">
        {TABS.map((t) => (
          <button
            key={t}
            type="button"
            onClick={() => go(t)}
            aria-current={tab === t ? "page" : undefined}
            className={`-mb-px border-b-2 px-3 py-2 text-sm transition-colors ${
              tab === t
                ? "border-primary font-medium text-on-surface"
                : "border-transparent text-on-surface-variant hover:text-on-surface"
            }`}
          >
            {LABEL[t]}
          </button>
        ))}
      </div>

      {/* Keyed so switching tables resets each panel rather than showing the last one's
          rows under the new name for a frame. */}
      <div className="min-h-0 flex-1 overflow-auto">
        {tab === "data" && <DataGrid bare table={table} onCount={onCount} />}
        {tab === "definition" && <DefinitionPanel table={table} />}
        {tab === "schema" && <SchemaPanel table={table} />}
        {tab === "api" && (
          <div className="flex min-h-0 px-8 py-6">
            <ApiConsole table={table} />
          </div>
        )}
        {tab === "changes" && <ChangesPanel table={table} />}
      </div>
    </div>
  );
}

/**
 * What builds this table. A `reduce` table's rules, rendered as the reducers file that
 * holds them; a `mirror` or `log` table's one line of config, and the source it follows
 * with its fields — which is the honest answer for a table that has no rules at all.
 */
function DefinitionPanel({ table }: { table: Table }) {
  const { name: project, mode } = useProject();
  const { data: sources } = useSources();
  const [config, setConfig] = useState<string | null>(null);
  const [reducers, setReducers] = useState<string | null>(null);
  const [draft, setDraft] = useState<string | null>(null);
  const [checked, setChecked] = useState<{ ok: boolean; details?: string } | null>(null);
  const [saving, setSaving] = useState(false);
  const [saved, setSaved] = useState(false);
  const [error, setError] = useState<string | null>(null);
  const [failed, setFailed] = useState(false);

  const load = useCallback(() => {
    if (mode !== "control" || !project) return;
    control
      .project(project)
      .then((p) => {
        setConfig(p.config);
        setReducers(p.reducers ?? null);
      })
      .catch(() => setFailed(true));
  }, [project, mode]);
  useEffect(load, [load]);

  const block = reducers ? dslTableBlock(reducers, table.name) : null;
  const shared = block ? foreignWrites(block, table.name) : [];
  const editable = block !== null && shared.length === 0;
  const text = draft ?? block ?? "";
  const changed = draft !== null && draft !== block;

  // The same check the state table editor runs, on the file this save would write.
  const whole = useMemo(
    () => (reducers && block && draft !== null ? reducers.replace(block, draft) : null),
    [reducers, block, draft],
  );
  useEffect(() => {
    if (!project || !config || !whole || !changed) {
      setChecked(null);
      return;
    }
    const timer = setTimeout(() => {
      control
        .check(project, config, whole)
        .then(() => setChecked({ ok: true }))
        .catch((e: unknown) =>
          setChecked({
            ok: false,
            details: e instanceof ApiError ? (e.details ?? e.message) : String(e),
          }),
        );
    }, 400);
    return () => clearTimeout(timer);
  }, [project, config, whole, changed]);

  const save = async () => {
    if (!project || !config || !whole) return;
    setSaving(true);
    setError(null);
    try {
      const updated = await control.update(project, config, whole);
      setReducers(updated.reducers ?? null);
      setDraft(null);
      setChecked(null);
      setSaved(true);
      setTimeout(() => setSaved(false), 2000);
    } catch (e) {
      setError(e instanceof ApiError ? (e.details ?? e.message) : String(e));
    } finally {
      setSaving(false);
    }
  };

  const definition = useMemo(
    () => (config ? definitionOf(config, table.name) : null),
    [config, table.name],
  );
  const follows = definition?.source
    ? (sources?.find((s) => s.name === definition.source) ?? null)
    : null;

  if (mode !== "control") {
    return (
      <Empty>
        The config lives in the control plane. This Studio is talking straight to one
        project&apos;s API, so there is nothing here to read it from.
      </Empty>
    );
  }

  return (
    <div className="flex flex-col gap-5 px-8 py-6">
      {block !== null && (
        <Card className="overflow-hidden">
          <div className="flex flex-wrap items-center justify-between gap-3 border-b border-outline-variant bg-surface-container-high px-4 py-2">
            <span className="text-[11px] font-semibold tracking-[0.08em] text-on-surface-variant uppercase">
              Rules
            </span>
            <span className="min-w-0 flex-1 truncate text-xs text-on-surface-variant">
              <span className="font-mono">{reducersFile(project ?? "")}</span>, the part that
              builds this table
            </span>
            {editable && changed && (
              <button
                type="button"
                onClick={() => setDraft(null)}
                className="text-xs text-on-surface-variant hover:text-on-surface"
              >
                Revert
              </button>
            )}
            {editable && (
              <Button
                tone="primary"
                disabled={!changed || saving || checked?.ok !== true}
                onClick={() => void save()}
              >
                {saving ? "Saving…" : saved ? "Saved" : "Save"}
              </Button>
            )}
          </div>
          {editable ? (
            <textarea
              value={text}
              onChange={(e) => setDraft(e.target.value)}
              spellCheck={false}
              wrap="off"
              aria-label={`Rules for ${table.name}`}
              rows={Math.min(28, Math.max(8, text.split("\n").length + 1))}
              className="block w-full resize-y overflow-auto bg-surface-container-low px-4 py-3 font-mono text-[13px] leading-relaxed text-on-surface outline-none focus:ring-1 focus:ring-inset focus:ring-primary"
            />
          ) : (
            <pre className="overflow-x-auto px-4 py-3 font-mono text-xs leading-relaxed">
              {block}
            </pre>
          )}
        </Card>
      )}

      {/* The one shape a table can't own. A handler is written event-first, so one `on()`
          may write several tables (ADR 0025) — saving it under this table's name would
          carry the others' rules along with it. */}
      {block !== null && !editable && (
        <Notice tone="neutral" title="These rules aren't this table's alone">
          A handler here also writes{" "}
          {shared.map((t) => (
            <span key={t} className="font-mono">
              {t}{" "}
            </span>
          ))}
          — so the block belongs to more than one table and can&apos;t be saved under this
          one. Edit it in the{" "}
          <Link
            href={`/state?project=${encodeURIComponent(project ?? "")}&table=${encodeURIComponent(table.name)}`}
            className="text-primary hover:underline"
          >
            state table editor
          </Link>
          , which writes the whole file.
        </Notice>
      )}

      {checked && !checked.ok && (
        <Notice tone="error" title="Nineveh can't build that">
          <pre className="overflow-x-auto font-mono text-xs whitespace-pre">{checked.details}</pre>
        </Notice>
      )}
      {checked?.ok && changed && (
        <p className="text-xs text-on-tertiary-container">
          Checks out. Saving rebuilds this table beside the served one and swaps it in once
          it has caught up.
        </p>
      )}
      {error && (
        <Notice tone="error" title="That didn't save">
          <pre className="overflow-x-auto font-mono text-xs whitespace-pre-wrap">{error}</pre>
        </Notice>
      )}

      {definition && (
        <Card className="overflow-hidden">
          <PanelHead title="nineveh.yaml" hint="what `state:` says about this table" />
          <pre className="overflow-x-auto px-4 py-3 font-mono text-xs leading-relaxed">
            {definition.text}
          </pre>
        </Card>
      )}

      {follows && <SourceSchema source={follows} />}

      {!definition && block === null && !failed && (
        <p className="text-sm text-on-surface-variant">Reading the config…</p>
      )}
      {failed && !definition && (
        <Notice tone="neutral" title="Couldn't read this table's definition">
          The rows and schema are served from the build that is running, which is the
          thing that matters — this panel only quotes the config back.
        </Notice>
      )}
    </div>
  );
}

/** The columns, as the API serves them. */
function SchemaPanel({ table }: { table: Table }) {
  return (
    <div className="px-8 py-6">
      <Card className="overflow-hidden">
        <PanelHead
          title="Columns"
          hint={`${table.columns.length} column${table.columns.length === 1 ? "" : "s"}, key ${table.key.join(", ")}`}
        />
        <table className="w-full text-left text-sm">
          {/* The key badge rides with the name, the way the grid's own header shows it —
              stranded in a column of its own it sits half a screen from what it marks. */}
          <thead className="border-b border-outline-variant text-[10px] font-medium tracking-[0.08em] text-on-surface-variant uppercase">
            <tr>
              <th className="px-4 py-2 pr-16 font-medium">Column</th>
              <th className="w-full px-4 py-2 font-medium">Type</th>
            </tr>
          </thead>
          <tbody className="font-mono text-xs">
            {table.columns.map((column) => (
              <tr key={column.name} className="border-b border-outline-variant last:border-0">
                <td className="px-4 py-1.5 whitespace-nowrap text-on-surface">
                  {column.name}
                  {table.key.includes(column.name) && (
                    <span
                      className="ml-2 rounded bg-primary/20 px-1 py-px text-[9px] font-semibold text-primary"
                      title="key column"
                    >
                      KEY
                    </span>
                  )}
                </td>
                <td className="px-4 py-1.5 text-on-surface-variant">{column.type}</td>
              </tr>
            ))}
          </tbody>
        </table>
      </Card>
      <p className="mt-3 text-xs leading-relaxed text-on-surface-variant">
        Wide integers — <span className="font-mono">u64</span> and up — are JSON strings in
        the API, because they don&apos;t fit a double (ADR 0008).
      </p>
    </div>
  );
}

/** This table's changes, live, newest first. */
function ChangesPanel({ table }: { table: Table }) {
  const [changes, setChanges] = useState<Change[]>([]);
  const onChanges = useCallback((batch: Change[]) => {
    setChanges((held) => [...batch].reverse().concat(held).slice(0, 200));
  }, []);
  const connected = useFeed({ tables: [table.name], onChanges });

  return (
    <div className="px-8 py-6">
      <Card className="overflow-hidden">
        <PanelHead
          title="Changes"
          hint={
            connected
              ? "live, newest first — the last 200 since you opened this tab"
              : "connecting to the change feed…"
          }
        />
        {changes.length === 0 ? (
          <p className="px-4 py-6 text-sm text-on-surface-variant">
            Nothing has changed here since you opened the tab. This is a live feed, not
            history — it starts from now.
          </p>
        ) : (
          <ul className="divide-y divide-outline-variant">
            {changes.map((change) => (
              <li
                key={`${change.version}.${change.seq}`}
                className="flex items-baseline gap-3 px-4 py-1.5 font-mono text-xs"
              >
                <span
                  className={`w-14 shrink-0 font-sans text-[10px] font-semibold tracking-wide uppercase ${
                    change.op === "delete"
                      ? "text-error"
                      : change.op === "insert"
                        ? "text-on-tertiary-container"
                        : "text-on-surface-variant"
                  }`}
                >
                  {change.op}
                </span>
                <span className="shrink-0 text-on-surface-variant">{change.version}</span>
                <span className="truncate text-on-surface">
                  {Object.entries(change.key)
                    .map(([k, v]) => `${k}=${String(v)}`)
                    .join(" ")}
                </span>
              </li>
            ))}
          </ul>
        )}
      </Card>
    </div>
  );
}

function PanelHead({ title, hint }: { title: string; hint: string }) {
  return (
    <div className="flex flex-wrap items-baseline gap-x-3 gap-y-1 border-b border-outline-variant bg-surface-container-high px-4 py-2">
      <span className="text-[11px] font-semibold tracking-[0.08em] text-on-surface-variant uppercase">
        {title}
      </span>
      <span className="text-xs text-on-surface-variant">{hint}</span>
    </div>
  );
}

function Empty({ children }: { children: React.ReactNode }) {
  return <p className="px-8 py-10 text-sm text-on-surface-variant">{children}</p>;
}
