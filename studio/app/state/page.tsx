"use client";

import { useRouter, useSearchParams } from "next/navigation";
import {
  Suspense,
  useCallback,
  useEffect,
  useMemo,
  useRef,
  useState,
} from "react";

import {
  ExpressionInput,
  type Insert,
  type Name,
} from "@/components/expression";
import { Button, Card, Notice, PageHeader } from "@/components/ui";
import {
  ApiError,
  type ColumnType,
  type FieldInfo,
  type Preview,
  type SourceInfo,
  type Table,
  control,
  getTables,
} from "@/lib/api";
import { useProject } from "@/lib/project";
import {
  COLUMN_TYPES,
  type Column,
  type Rule,
  type StateTable,
  amountFields,
  blank,
  countPer,
  dailyPer,
  keyFields,
  latestPer,
  liveSet,
  problems,
  sumPer,
  toYaml,
  withLookup,
  withTable,
} from "@/lib/state-table";

/** The functions an expression can call (`docs/expressions.md`). */
const FUNCTIONS = [
  "min",
  "max",
  "abs",
  "is_some",
  "is_none",
  "unwrap_or",
  "u8",
  "u16",
  "u32",
  "u64",
  "u128",
  "u256",
  "i8",
  "i16",
  "i32",
  "i64",
  "i128",
  "i256",
];

/**
 * Everything a rule's expressions can refer to: the record's fields, the row's own
 * columns, the transaction, the functions, and a row of another table (ADR 0019). A
 * name that is both a field and a column is offered qualified, since a bare one would
 * be ambiguous.
 */
function namesInScope(
  source: string,
  fields: FieldInfo[],
  columns: Column[],
  tables: Table[],
): Name[] {
  const clash = (name: string) =>
    fields.some((f) => f.name === name) && columns.some((c) => c.name === name);
  return [
    ...fields.map((f) => ({
      label: clash(f.name) ? `${source}.${f.name}` : f.name,
      detail: f.type,
      kind: "field" as const,
    })),
    ...columns.map((c) => ({
      label: clash(c.name) ? `row.${c.name}` : c.name,
      detail: c.type,
      kind: "column" as const,
    })),
    { label: "tx.version", detail: "u64", kind: "builtin" as const },
    { label: "tx.timestamp", detail: "u64", kind: "builtin" as const },
    ...tables.map((t) => ({
      label: `${t.name}[${t.key.join(", ")}]`,
      insert: `${t.name}[`,
      detail: "table",
      kind: "table" as const,
    })),
    ...FUNCTIONS.map((f) => ({
      label: f,
      insert: `${f}(`,
      detail: "function",
      kind: "function" as const,
    })),
  ];
}

/** Narrows a list to one with a first element, so pickers always have a selection. */
function isFilled(
  sources: SourceInfo[],
): sources is [SourceInfo, ...SourceInfo[]] {
  return sources.length > 0;
}

const field =
  "rounded-md border border-zinc-200 bg-white px-2.5 py-1.5 text-sm focus:border-lapis-400 focus:outline-none dark:border-zinc-700 dark:bg-zinc-900";

/**
 * Design a state table: a key, typed columns, and rules that fold records into them.
 * The control plane checks the config as it's written, and saving it rebuilds the
 * project's tables (ADR 0016).
 */
export default function StateTablePage() {
  return (
    <Suspense>
      <StateTableEditor />
    </Suspense>
  );
}

function StateTableEditor() {
  const { name: project, mode, base } = useProject();
  const router = useRouter();
  // `?table=` opens a saved table to change, instead of designing a new one.
  const editing = useSearchParams().get("table");
  const [sources, setSources] = useState<SourceInfo[] | null>(null);
  const [existing, setExisting] = useState<Table[]>([]);
  const [config, setConfig] = useState<string | null>(null);
  const [table, setTable] = useState<StateTable | null>(null);
  const [checked, setChecked] = useState<{
    ok: boolean;
    details?: string;
  } | null>(null);
  const [saving, setSaving] = useState(false);
  const [loadError, setLoadError] = useState<string | null>(null);
  const [error, setError] = useState<string | null>(null);

  useEffect(() => {
    if (!project) return;
    const failed = (e: unknown) =>
      setLoadError(e instanceof Error ? e.message : String(e));
    control.sources(project).then(setSources).catch(failed);
    control
      .project(project)
      .then((p) => setConfig(p.config))
      .catch(failed);
    if (editing) {
      control
        .stateTable(project, editing)
        .then((saved) =>
          setTable({
            name: saved.name,
            columns: saved.columns,
            rules: saved.rules,
          }),
        )
        .catch(failed);
    }
  }, [project, editing]);

  // The tables a rule can read from (ADR 0019). A project still being built has none,
  // and that's not an error worth showing.
  useEffect(() => {
    if (!base) return;
    getTables(base)
      .then((tables) => setExisting(tables.filter((t) => t.kind !== "log")))
      .catch(() => setExisting([]));
  }, [base]);

  const yaml = useMemo(
    () => (table && config ? withTable(config, table) : null),
    [table, config],
  );
  const listed = table ? problems(table) : [];

  // Check with the server as it's written, once it's worth checking.
  useEffect(() => {
    if (!project || !yaml || listed.length > 0) {
      setChecked(null);
      return;
    }
    const timer = setTimeout(() => {
      control
        .check(project, yaml)
        .then(() => setChecked({ ok: true }))
        .catch((e: unknown) =>
          setChecked({
            ok: false,
            details:
              e instanceof ApiError ? (e.details ?? e.message) : String(e),
          }),
        );
    }, 400);
    return () => clearTimeout(timer);
  }, [project, yaml, listed.length]);

  const save = useCallback(async () => {
    if (!project || !yaml || !table) return;
    setSaving(true);
    setError(null);
    try {
      await control.update(project, yaml);
      router.push(
        `/tables?project=${encodeURIComponent(project)}&name=${encodeURIComponent(table.name)}`,
      );
    } catch (e) {
      setError(e instanceof ApiError ? (e.details ?? e.message) : String(e));
      setSaving(false);
    }
  }, [project, yaml, table, router]);

  if (mode === "single" || !base) {
    return (
      <div className="mx-auto mt-24 max-w-md px-6 text-center text-sm text-zinc-500">
        Open a project first: state tables belong to one.
      </div>
    );
  }

  return (
    <div>
      <PageHeader title={editing ? `Edit ${editing}` : "New state table"}>
        {checked?.ok && (
          <span className="text-xs text-emerald-600">checks out</span>
        )}
        <Button
          tone="primary"
          disabled={saving || !checked?.ok}
          onClick={() => void save()}
        >
          {saving ? "Saving…" : editing ? "Save changes" : "Create table"}
        </Button>
      </PageHeader>

      <div className="mx-auto flex max-w-5xl flex-col gap-6 px-8 py-6">
        {!table && !editing && (
          <div>
            <h2 className="text-xl font-semibold tracking-tight">
              What should this table hold?
            </h2>
            <p className="mt-1.5 max-w-2xl text-sm text-zinc-500 text-pretty">
              You say what a row is and how each record changes it. Nineveh
              folds every record into it in order, and serves it over REST with
              a change feed, like any other table.
            </p>
          </div>
        )}

        {loadError && (
          <Notice tone="error" title="Couldn't read this project">
            {loadError}
          </Notice>
        )}
        {error && (
          <Notice tone="error" title="That didn't save">
            <pre className="overflow-x-auto font-mono text-xs whitespace-pre-wrap">
              {error}
            </pre>
          </Notice>
        )}
        {sources?.length === 0 && (
          <Notice tone="warning" title="This project follows nothing yet">
            Add a source to its config first.
          </Notice>
        )}

        {sources && isFilled(sources) && !table && !editing && (
          <Templates sources={sources} existing={existing} onPick={setTable} />
        )}

        {sources && table && (
          <>
            <Editor
              sources={sources}
              existing={existing}
              table={table}
              onChange={setTable}
            />
            <Card className="overflow-hidden">
              <div className="flex items-center justify-between border-b border-zinc-200 px-4 py-2 dark:border-zinc-800">
                <span className="text-xs text-zinc-500">
                  <span className="font-mono">nineveh.yaml</span>, as this will
                  be saved
                </span>
                <button
                  type="button"
                  onClick={() => setTable(null)}
                  className="text-xs text-zinc-500 hover:text-zinc-900 dark:hover:text-zinc-100"
                >
                  {editing ? "Discard changes" : "Start over"}
                </button>
              </div>
              <pre className="max-h-64 overflow-auto px-4 py-3 font-mono text-xs leading-relaxed">
                {toYaml(table)}
              </pre>
            </Card>
            {listed.length > 0 && (
              <Notice tone="warning" title="Not finished yet">
                <ul className="list-inside list-disc">
                  {listed.map((problem) => (
                    <li key={problem}>{problem}</li>
                  ))}
                </ul>
              </Notice>
            )}
            {checked && !checked.ok && (
              <Notice tone="error" title="Nineveh can't build that">
                <pre className="overflow-x-auto font-mono text-xs whitespace-pre">
                  {checked.details}
                </pre>
              </Notice>
            )}
            {project && yaml && (
              <PreviewCard
                project={project}
                yaml={yaml}
                table={table}
                ready={checked?.ok === true}
              />
            )}
          </>
        )}
      </div>
    </div>
  );
}

/**
 * What the rules would produce, folded over recent transactions without saving
 * anything. Asked for rather than automatic: it reads a window of the chain, which
 * takes a few seconds.
 */
function PreviewCard({
  project,
  yaml,
  table,
  ready,
}: {
  project: string;
  yaml: string;
  table: StateTable;
  ready: boolean;
}) {
  const [preview, setPreview] = useState<Preview | null>(null);
  const [error, setError] = useState<string | null>(null);
  const [running, setRunning] = useState(false);

  // Anything edited makes what's shown stale.
  useEffect(() => {
    setPreview(null);
    setError(null);
  }, [yaml]);

  const run = useCallback(async () => {
    setRunning(true);
    setError(null);
    try {
      setPreview(await control.preview(project, yaml, table.name));
    } catch (e) {
      setError(e instanceof ApiError ? (e.details ?? e.message) : String(e));
    } finally {
      setRunning(false);
    }
  }, [project, yaml, table.name]);

  const columns = table.columns.map((c) => c.name);
  return (
    <Card className="overflow-hidden">
      <div className="flex items-center justify-between gap-3 border-b border-zinc-200 px-4 py-2 dark:border-zinc-800">
        <span className="text-xs text-zinc-500">
          {preview
            ? `${preview.row_count} row${preview.row_count === 1 ? "" : "s"} from ${preview.transactions} recent transactions`
            : "What these rules would produce, from the chain's recent transactions"}
        </span>
        <Button onClick={() => void run()} disabled={!ready || running}>
          {running ? "Folding…" : preview ? "Run again" : "Preview rows"}
        </Button>
      </div>
      {error && (
        <div className="px-4 py-3">
          <Notice tone="error" title="These rules don't survive real data">
            <pre className="overflow-x-auto font-mono text-xs whitespace-pre-wrap">
              {error}
            </pre>
          </Notice>
        </div>
      )}
      {preview && !error && preview.rows.length === 0 && (
        <p className="px-4 py-3 text-sm text-zinc-500">
          Nothing in the last {preview.transactions} transactions fed this
          table. That isn&apos;t a problem with the rules — try again once the
          contract has been used.
        </p>
      )}
      {preview && preview.rows.length > 0 && (
        <div className="overflow-x-auto">
          <table className="w-full text-left text-sm">
            <thead className="border-b border-zinc-200 text-xs text-zinc-500 dark:border-zinc-800">
              <tr>
                {columns.map((name) => (
                  <th key={name} className="px-4 py-1.5 font-medium">
                    {name}
                  </th>
                ))}
              </tr>
            </thead>
            <tbody>
              {preview.rows.map((row, i) => (
                <tr
                  key={i}
                  className="border-b border-zinc-100 last:border-0 dark:border-zinc-800/60"
                >
                  {columns.map((name) => (
                    <td key={name} className="px-4 py-1.5 font-mono text-xs">
                      {cell(row[name])}
                    </td>
                  ))}
                </tr>
              ))}
            </tbody>
          </table>
          {preview.row_count > preview.rows.length && (
            <p className="px-4 py-2 text-xs text-zinc-500">
              and {preview.row_count - preview.rows.length} more.
            </p>
          )}
        </div>
      )}
    </Card>
  );
}

/** One preview value, short enough for a cell. */
function cell(value: unknown): string {
  if (value === null || value === undefined) return "—";
  const text =
    typeof value === "object" ? JSON.stringify(value) : String(value);
  return text.length > 40 ? `${text.slice(0, 39)}…` : text;
}

/** The shapes most state tables have, filled in from a source's fields. */
function Templates({
  sources,
  existing,
  onPick,
}: {
  sources: [SourceInfo, ...SourceInfo[]];
  existing: Table[];
  onPick: (table: StateTable) => void;
}) {
  const [source, setSource] = useState<SourceInfo>(sources[0]);
  const keys = keyFields(source);
  const amounts = amountFields(source);
  const [key, setKey] = useState(keys[0]?.name ?? "");
  const [amount, setAmount] = useState(amounts[0]?.name ?? "");
  const keyField = keys.find((f) => f.name === key) ?? keys[0];
  const amountField = amounts.find((f) => f.name === amount) ?? amounts[0];

  // What makes a row disappear again: this source's own deletes, or another source
  // that names the same key.
  const gone = source.deletes
    ? { name: source.name, deleted: true }
    : sources
        .filter((s) => s.name !== source.name)
        .filter((s) =>
          s.fields.some(
            (f) => f.name === keyField?.name && f.type === keyField?.type,
          ),
        )
        .map((s) => ({ name: s.name, deleted: false }))[0];

  // A table this one could look a row up in: keyed by one column of the same type as
  // the key, with something to read.
  const joinable = existing
    .filter((t) => t.key.length === 1 && t.name !== source.name)
    .flatMap((t) => {
      const keyColumn = t.columns.find((c) => c.name === t.key[0]);
      if (!keyColumn || keyColumn.type !== keyField?.type) return [];
      const readable = t.columns.filter((c) => !t.key.includes(c.name));
      const column = readable.find((c) => c.type !== "json") ?? readable[0];
      return column ? [{ table: t, column }] : [];
    })[0];

  const pick = (source: SourceInfo) => {
    setSource(source);
    const keys = keyFields(source);
    const amounts = amountFields(source);
    setKey(keys[0]?.name ?? "");
    setAmount(amounts[0]?.name ?? "");
  };

  return (
    <div className="flex flex-col gap-5">
      <div className="flex flex-wrap items-end gap-3 rounded-xl border border-zinc-200/80 bg-well px-4 py-3 dark:border-zinc-800">
        <label className="flex flex-col gap-1.5 text-xs font-medium text-zinc-500">
          Fold records from
          <select
            value={source.name}
            onChange={(e) =>
              pick(sources.find((s) => s.name === e.target.value) ?? sources[0])
            }
            className={`${field} font-mono`}
          >
            {sources.map((s) => (
              <option key={s.name} value={s.name}>
                {s.name} ({s.kind})
              </option>
            ))}
          </select>
        </label>
        <label className="flex flex-col gap-1.5 text-xs font-medium text-zinc-500">
          One row per
          <select
            value={key}
            onChange={(e) => setKey(e.target.value)}
            className={`${field} font-mono`}
          >
            {keys.map((f) => (
              <option key={f.name} value={f.name}>
                {f.name}
              </option>
            ))}
          </select>
        </label>
        {amounts.length > 0 && (
          <label className="flex flex-col gap-1.5 text-xs font-medium text-zinc-500">
            Adding up
            <select
              value={amount}
              onChange={(e) => setAmount(e.target.value)}
              className={`${field} font-mono`}
            >
              {amounts.map((f) => (
                <option key={f.name} value={f.name}>
                  {f.name}
                </option>
              ))}
            </select>
          </label>
        )}
      </div>

      <div className="grid gap-3 sm:grid-cols-2">
        <Template
          title="Count per row"
          body={
            keyField
              ? `How many ${source.name} records each ${keyField.name} has, and when it last happened.`
              : "This source has nothing to key by."
          }
          disabled={!keyField}
          shape={keyField && countPer(source, keyField)}
          onClick={() => keyField && onPick(countPer(source, keyField))}
        />
        <Template
          title="Total per row"
          body={
            keyField && amountField
              ? `${amountField.name} added up per ${keyField.name}, in a column wide enough to hold it.`
              : "This source has no amounts to add up."
          }
          disabled={!keyField || !amountField}
          shape={
            keyField && amountField && sumPer(source, keyField, amountField)
          }
          onClick={() =>
            keyField &&
            amountField &&
            onPick(sumPer(source, keyField, amountField))
          }
        />
        <Template
          title="Latest per row"
          body={
            keyField
              ? `The newest ${source.name} for each ${keyField.name}, field by field.`
              : ""
          }
          disabled={!keyField}
          shape={keyField && latestPer(source, keyField)}
          onClick={() => keyField && onPick(latestPer(source, keyField))}
        />
        <Template
          title="Per day"
          body={
            keyField
              ? `${amountField ? `${amountField.name} added up` : `How many ${source.name} records`} per ${keyField.name}, a row per day: what a chart is made of.`
              : "This source has nothing to key by."
          }
          disabled={!keyField}
          shape={keyField && dailyPer(source, keyField, amountField)}
          onClick={() =>
            keyField && onPick(dailyPer(source, keyField, amountField))
          }
        />
        <Template
          title="Appears and disappears"
          body={
            keyField && gone
              ? `A row per ${keyField.name} while it's open: added on ${source.name}, removed on ${gone.deleted ? `${gone.name} deleted` : gone.name}.`
              : "Nothing here says when a row should go away again."
          }
          disabled={!keyField || !gone}
          shape={keyField && gone && liveSet(source, keyField, gone)}
          onClick={() =>
            keyField && gone && onPick(liveSet(source, keyField, gone))
          }
        />
        <Template
          title="With a value from another table"
          body={
            keyField && joinable
              ? `How many ${source.name} records per ${keyField.name}, plus ${joinable.column.name} read from ${joinable.table.name}.`
              : "No other table is keyed by something this could look up."
          }
          disabled={!keyField || !joinable}
          shape={
            keyField &&
            joinable &&
            withLookup(source, keyField, joinable.table, joinable.column)
          }
          onClick={() =>
            keyField &&
            joinable &&
            onPick(
              withLookup(source, keyField, joinable.table, joinable.column),
            )
          }
        />
      </div>

      <button
        type="button"
        onClick={() => onPick(blank(source))}
        className="rounded-xl border border-dashed border-zinc-300 px-4 py-3 text-sm text-zinc-500 transition-colors hover:border-lapis-400 hover:text-zinc-900 dark:border-zinc-700 dark:hover:text-zinc-100"
      >
        Or start from an empty table and write the columns and rules yourself.
      </button>
    </div>
  );
}

/**
 * A miniature of the table a template would build: its columns, with the key ones
 * marked. Seeing the shape answers "what do I get?" faster than any description.
 */
function Shape({ table }: { table: StateTable }) {
  const columns = table.columns.slice(0, 4);
  const more = table.columns.length - columns.length;
  return (
    <div className="mt-3 overflow-hidden rounded-lg border border-zinc-200/80 bg-card dark:border-zinc-700/60">
      <div className="flex items-center gap-3 border-b border-zinc-200/80 px-2.5 py-1.5 dark:border-zinc-700/60">
        {columns.map((column) => (
          <span
            key={column.name}
            className={`truncate font-mono text-[10px] ${
              column.key
                ? "font-medium text-lapis-600 dark:text-lapis-400"
                : "text-zinc-400"
            }`}
          >
            {column.name}
          </span>
        ))}
        {more > 0 && (
          <span className="text-[10px] text-zinc-300 dark:text-zinc-600">
            +{more}
          </span>
        )}
      </div>
      {[0.7, 0.45].map((fade, row) => (
        <div
          key={row}
          className="flex items-center gap-3 px-2.5 py-1.5"
          style={{ opacity: fade }}
        >
          {columns.map((column, i) => (
            <span
              key={column.name}
              className="h-1.5 rounded-full bg-zinc-200 dark:bg-zinc-700"
              style={{ width: `${[38, 22, 30, 18][i % 4]}px` }}
            />
          ))}
        </div>
      ))}
    </div>
  );
}

function Template({
  title,
  body,
  shape,
  disabled = false,
  onClick,
}: {
  title: string;
  body: string;
  /** The table this would build, drawn small. */
  shape?: StateTable | false | undefined;
  disabled?: boolean;
  onClick: () => void;
}) {
  return (
    <button
      type="button"
      disabled={disabled}
      onClick={onClick}
      className="group flex flex-col rounded-xl border border-zinc-200 bg-card px-4 py-3.5 text-left shadow-card transition-all hover:-translate-y-px hover:border-lapis-400 hover:shadow-lifted disabled:cursor-not-allowed disabled:opacity-50 disabled:hover:translate-y-0 disabled:hover:border-zinc-200 disabled:hover:shadow-card dark:border-zinc-800"
    >
      <div className="text-sm font-semibold group-enabled:group-hover:text-lapis-700 dark:group-enabled:group-hover:text-lapis-300">
        {title}
      </div>
      <div className="mt-1 text-xs leading-relaxed text-zinc-500">{body}</div>
      {shape && <Shape table={shape} />}
    </button>
  );
}

/** The table's columns and rules, all editable. */
function Editor({
  sources,
  existing,
  table,
  onChange,
}: {
  sources: SourceInfo[];
  existing: Table[];
  table: StateTable;
  onChange: (table: StateTable) => void;
}) {
  const set = (changes: Partial<StateTable>) =>
    onChange({ ...table, ...changes });
  const setColumn = (index: number, changes: Partial<Column>) =>
    set({
      columns: table.columns.map((c, i) =>
        i === index ? { ...c, ...changes } : c,
      ),
    });
  const setRule = (index: number, changes: Partial<Rule>) =>
    set({
      rules: table.rules.map((r, i) =>
        i === index ? { ...r, ...changes } : r,
      ),
    });

  const keyColumns = table.columns.filter((c) => c.key).map((c) => c.name);
  return (
    <>
      <Card className="overflow-hidden">
        <div className="flex flex-wrap items-end justify-between gap-3 border-b border-zinc-200 bg-well px-4 py-3 dark:border-zinc-800">
          <label className="flex flex-col gap-1.5">
            <span className="text-xs font-medium text-zinc-600 dark:text-zinc-400">
              Table name
            </span>
            <input
              value={table.name}
              onChange={(e) => set({ name: e.target.value })}
              spellCheck={false}
              className={`${field} w-72 font-mono text-[15px]`}
            />
          </label>
          <p className="pb-2 text-xs text-zinc-500">
            {keyColumns.length > 0 ? (
              <>
                one row per{" "}
                <span className="font-mono text-zinc-700 dark:text-zinc-300">
                  {keyColumns.join(", ")}
                </span>
              </>
            ) : (
              "no key yet: tick the columns that identify a row"
            )}
          </p>
        </div>

        <div className="overflow-x-auto px-4 pt-3 pb-4">
          <table className="w-full text-sm">
            <thead className="text-left text-xs text-zinc-400">
              <tr>
                <th className="py-1 font-medium">Name</th>
                <th className="py-1 font-medium">Type</th>
                <th className="py-1 font-medium">Default</th>
                <th className="py-1 font-medium">Null?</th>
                <th className="py-1 font-medium" title="What identifies a row">
                  Key?
                </th>
                <th />
              </tr>
            </thead>
            <tbody>
              {table.columns.map((column, index) => (
                <tr key={index}>
                  <td className="py-1 pr-2">
                    <input
                      value={column.name}
                      onChange={(e) =>
                        setColumn(index, { name: e.target.value })
                      }
                      spellCheck={false}
                      className={`${field} w-40 font-mono`}
                    />
                  </td>
                  <td className="py-1 pr-2">
                    <select
                      value={column.type}
                      onChange={(e) =>
                        setColumn(index, { type: e.target.value as ColumnType })
                      }
                      className={`${field} font-mono`}
                    >
                      {COLUMN_TYPES.map((type) => (
                        <option key={type} value={type}>
                          {type}
                        </option>
                      ))}
                    </select>
                  </td>
                  <td className="py-1 pr-2">
                    <input
                      value={column.default}
                      onChange={(e) =>
                        setColumn(index, { default: e.target.value })
                      }
                      placeholder="none"
                      spellCheck={false}
                      className={`${field} w-24 font-mono`}
                    />
                  </td>
                  <td className="py-1 pr-2">
                    <input
                      type="checkbox"
                      checked={column.nullable}
                      onChange={(e) =>
                        setColumn(index, { nullable: e.target.checked })
                      }
                      className="accent-lapis-600"
                    />
                  </td>
                  <td className="py-1 pr-2">
                    <input
                      type="checkbox"
                      checked={column.key}
                      onChange={(e) =>
                        setColumn(index, { key: e.target.checked })
                      }
                      className="accent-lapis-600"
                    />
                  </td>
                  <td className="py-1">
                    <button
                      type="button"
                      onClick={() =>
                        set({
                          columns: table.columns.filter((_, i) => i !== index),
                        })
                      }
                      className="text-xs text-zinc-400 hover:text-red-600"
                    >
                      Remove
                    </button>
                  </td>
                </tr>
              ))}
            </tbody>
          </table>
          <Button
            className="mt-3"
            onClick={() =>
              set({
                columns: [
                  ...table.columns,
                  {
                    name: "",
                    type: "u64",
                    default: "0",
                    nullable: false,
                    key: false,
                  },
                ],
              })
            }
          >
            Add column
          </Button>
        </div>
      </Card>

      {table.rules.map((rule, index) => (
        <RuleCard
          key={index}
          n={index + 1}
          sources={sources}
          existing={existing}
          table={table}
          rule={rule}
          onChange={(changes) => setRule(index, changes)}
          onRemove={() =>
            set({ rules: table.rules.filter((_, i) => i !== index) })
          }
        />
      ))}
      <Button
        onClick={() =>
          set({
            rules: [
              ...table.rules,
              {
                on: sources[0]?.name ?? "",
                deleted: false,
                when: "",
                keys: [],
                sets: [],
                removes: false,
              },
            ],
          })
        }
      >
        Add rule
      </Button>
    </>
  );
}

function RuleCard({
  n,
  sources,
  existing,
  table,
  rule,
  onChange,
  onRemove,
}: {
  n: number;
  sources: SourceInfo[];
  existing: Table[];
  table: StateTable;
  rule: Rule;
  onChange: (changes: Partial<Rule>) => void;
  onRemove: () => void;
}) {
  const source = sources.find((s) => s.name === rule.on);
  const readable = rule.deleted
    ? (source?.delete_fields ?? [])
    : (source?.fields ?? []);
  const keyColumns = table.columns.filter((c) => c.key);
  const mapped = new Set(rule.keys.map((k) => k.column));
  // A key column the record doesn't name has to be mapped.
  const unnamed = keyColumns.filter(
    (c) => !mapped.has(c.name) && !readable.some((f) => f.name === c.name),
  );

  const names = namesInScope(rule.on, readable, table.columns, existing);
  // A key picks the row, so it can't read the row's own columns.
  const keyNames = namesInScope(rule.on, readable, [], existing);
  // The expression box the chips type into: whichever one has the focus.
  const active = useRef<Insert | null>(null);
  const chip = (text: string, insert = text) => (
    <button
      key={text}
      type="button"
      // Keep the focused input focused, so the chip knows where to type.
      onMouseDown={(e) => e.preventDefault()}
      onClick={() => active.current?.insert(insert)}
      title="Click to put it in the expression you're editing"
      className="rounded bg-zinc-100 px-1.5 py-0.5 font-mono hover:bg-lapis-100 dark:bg-zinc-800 dark:hover:bg-zinc-700"
    >
      {text}
    </button>
  );

  return (
    <Card className="overflow-hidden">
      <div className="flex items-center justify-between gap-3 border-b border-zinc-200 bg-well px-4 py-2 dark:border-zinc-800">
        <span className="text-xs font-semibold tracking-wide text-zinc-500 uppercase">
          Rule {n}
        </span>
        <span className="truncate text-xs text-zinc-500">
          {rule.removes
            ? "deletes the row"
            : `sets ${rule.sets.length || "no"} column${rule.sets.length === 1 ? "" : "s"}`}
          {" on "}
          <span className="font-mono text-zinc-700 dark:text-zinc-300">
            {rule.on}
            {rule.deleted ? ".deleted" : ""}
          </span>
        </span>
      </div>
      <div className="p-4">
        <div className="flex flex-wrap items-end gap-3">
          <label className="flex flex-col gap-1.5 text-xs font-medium text-zinc-500">
            On each record from
            <select
              value={rule.on}
              onChange={(e) => onChange({ on: e.target.value, deleted: false })}
              className={`${field} font-mono`}
            >
              {sources.map((s) => (
                <option key={s.name} value={s.name}>
                  {s.name}
                </option>
              ))}
            </select>
          </label>
          {source?.deletes && (
            <label className="flex items-center gap-2 pb-2 text-xs text-zinc-500">
              <input
                type="checkbox"
                checked={rule.deleted}
                onChange={(e) => onChange({ deleted: e.target.checked })}
                className="accent-lapis-600"
              />
              when it&apos;s deleted
            </label>
          )}
          <label className="flex flex-1 flex-col gap-1.5 text-xs font-medium text-zinc-500">
            Only when (optional)
            <ExpressionInput
              value={rule.when}
              onChange={(when) => onChange({ when })}
              names={names}
              placeholder="amount > 0"
              onActive={(handle) => (active.current = handle)}
            />
          </label>
          <button
            type="button"
            onClick={onRemove}
            className="pb-2 text-xs text-zinc-400 hover:text-red-600"
          >
            Remove rule
          </button>
        </div>

        <div className="mt-3 flex flex-wrap items-center gap-1.5 text-xs text-zinc-500">
          Click to use:
          {readable.map((f) => chip(f.name))}
          {table.columns.filter((c) => !c.key).map((c) => chip(c.name))}
          {chip("tx.timestamp")}
          {existing.map((t) =>
            chip(`${t.name}[${t.key.join(", ")}]`, `${t.name}[`),
          )}
        </div>

        {unnamed.length > 0 && (
          <p className="mt-2 text-xs text-amber-600">
            Say where {unnamed.map((c) => c.name).join(", ")} comes from: the
            record has no field of that name.
          </p>
        )}

        <label className="mt-3 flex items-center gap-2 text-sm">
          <input
            type="checkbox"
            checked={rule.removes}
            onChange={(e) => onChange({ removes: e.target.checked })}
            className="accent-lapis-600"
          />
          Delete the row instead of setting columns
        </label>

        {!rule.removes && (
          <div className="mt-2">
            <div className="text-xs font-medium text-zinc-500">Set</div>
            {rule.sets.map((assignment, index) => (
              <div key={index} className="mt-1.5 flex items-center gap-2">
                <select
                  value={assignment.column}
                  onChange={(e) =>
                    onChange({
                      sets: rule.sets.map((s, i) =>
                        i === index ? { ...s, column: e.target.value } : s,
                      ),
                    })
                  }
                  className={`${field} w-44 font-mono`}
                >
                  <option value="">column…</option>
                  {table.columns
                    .filter((c) => !c.key)
                    .map((c) => (
                      <option key={c.name} value={c.name}>
                        {c.name}
                      </option>
                    ))}
                </select>
                <span className="text-zinc-400">=</span>
                <ExpressionInput
                  value={assignment.expression}
                  onChange={(expression) =>
                    onChange({
                      sets: rule.sets.map((s, i) =>
                        i === index ? { ...s, expression } : s,
                      ),
                    })
                  }
                  names={names}
                  placeholder="count + 1"
                  onActive={(handle) => (active.current = handle)}
                />
                <button
                  type="button"
                  onClick={() =>
                    onChange({ sets: rule.sets.filter((_, i) => i !== index) })
                  }
                  className="text-xs text-zinc-400 hover:text-red-600"
                >
                  Remove
                </button>
              </div>
            ))}
            <Button
              className="mt-2"
              onClick={() =>
                onChange({
                  sets: [...rule.sets, { column: "", expression: "" }],
                })
              }
            >
              Add a column to set
            </Button>
          </div>
        )}

        {(rule.keys.length > 0 || unnamed.length > 0) && (
          <div className="mt-3">
            <div className="text-xs font-medium text-zinc-500">
              Key columns from the record
            </div>
            {rule.keys.map((assignment, index) => (
              <div key={index} className="mt-1.5 flex items-center gap-2">
                <select
                  value={assignment.column}
                  onChange={(e) =>
                    onChange({
                      keys: rule.keys.map((k, i) =>
                        i === index ? { ...k, column: e.target.value } : k,
                      ),
                    })
                  }
                  className={`${field} w-44 font-mono`}
                >
                  <option value="">key column…</option>
                  {keyColumns.map((c) => (
                    <option key={c.name} value={c.name}>
                      {c.name}
                    </option>
                  ))}
                </select>
                <span className="text-zinc-400">=</span>
                <ExpressionInput
                  value={assignment.expression}
                  onChange={(expression) =>
                    onChange({
                      keys: rule.keys.map((k, i) =>
                        i === index ? { ...k, expression } : k,
                      ),
                    })
                  }
                  names={keyNames}
                  placeholder="key"
                  onActive={(handle) => (active.current = handle)}
                />
                <button
                  type="button"
                  onClick={() =>
                    onChange({ keys: rule.keys.filter((_, i) => i !== index) })
                  }
                  className="text-xs text-zinc-400 hover:text-red-600"
                >
                  Remove
                </button>
              </div>
            ))}
            <Button
              className="mt-2"
              onClick={() =>
                onChange({
                  keys: [
                    ...rule.keys,
                    { column: unnamed[0]?.name ?? "", expression: "" },
                  ],
                })
              }
            >
              Map a key column
            </Button>
          </div>
        )}
      </div>
    </Card>
  );
}
