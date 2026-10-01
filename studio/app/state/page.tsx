"use client";

import { useRouter, useSearchParams } from "next/navigation";
import { PageHeader } from "@/components/page-header";
import { Suspense, useCallback, useEffect, useMemo, useRef, useState, type ReactNode } from "react";

import { ExpressionInput, type Insert, type Name } from "@/components/expression";
import { FUNCTIONS } from "@/lib/language";
import { SourceSchema } from "@/components/source-schema";
import { Button, Card, Notice, Select } from "@/components/ui";
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
  isInteger,
  dailyPer,
  keyFields,
  latestPer,
  liveSet,
  problems,
  sumPer,
  toDsl,
  withLookup,
  withDslTable,
  withReducersKey,
  reducersFile,
} from "@/lib/state-table";


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
function isFilled(sources: SourceInfo[]): sources is [SourceInfo, ...SourceInfo[]] {
  return sources.length > 0;
}

const field =
  "rounded-sm border border-outline-variant bg-surface-container-low px-2.5 py-1.5 text-sm focus:border-primary focus:outline-none";

/**
 * One stage of the page.
 *
 * The page is a sequence because the data model is one: a rule folds records from a
 * source, so there is nothing to ask about a key until the source is known, and no
 * meaning to a column until a row has one. Each step is numbered and carries the
 * question it answers in words, not jargon — and a step that is settled says what it was
 * answered with, so the page reads back as a sentence once it's finished.
 */
function Step({
  n,
  title,
  hint,
  answer,
  action,
  children,
}: {
  n: number;
  title: string;
  hint?: ReactNode;
  /** What this step was answered with, shown once it has been. */
  answer?: ReactNode;
  /** A way to undo the answer, beside it. */
  action?: ReactNode;
  children?: ReactNode;
}) {
  return (
    <section className="flex gap-4">
      <span
        aria-hidden
        className="mt-0.5 flex size-6 shrink-0 items-center justify-center rounded-full bg-surface-container-high text-xs font-medium text-on-surface-variant"
      >
        {n}
      </span>
      <div className="min-w-0 flex-1">
        <div className="flex flex-wrap items-baseline gap-x-3 gap-y-1">
          <h2 className="text-sm font-medium text-on-surface">{title}</h2>
          {answer && <span className="min-w-0 text-xs text-on-surface-variant">{answer}</span>}
          {action && <span className="ml-auto">{action}</span>}
        </div>
        {hint && <p className="mt-1 max-w-2xl text-xs leading-relaxed text-on-surface-variant">{hint}</p>}
        {children && <div className="mt-3">{children}</div>}
      </div>
    </section>
  );
}

/** A quiet link that undoes a step, so a wrong turn costs one click. */
function Undo({ onClick, children }: { onClick: () => void; children: ReactNode }) {
  return (
    <button
      type="button"
      onClick={onClick}
      className="text-xs text-on-surface-variant hover:text-on-surface"
    >
      {children}
    </button>
  );
}

/**
 * What a column of `type` should hold before any rule has written it.
 *
 * Zero for the integers, which is what makes a fold total: `count = count + 1` on a row
 * that doesn't exist yet is only legal because the column it writes starts somewhere.
 * Nothing for the rest — there is no obvious empty address or id, and a rule that creates
 * a row has to say what goes there.
 */
function defaultFor(type: ColumnType): string {
  return isInteger(type) ? "0" : "";
}

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
  // Carried through untouched: this editor writes YAML, but a project whose reducers
  // are in the DSL has to be saved with them or it isn't the same project (ADR 0025).
  const [reducers, setReducers] = useState<string | undefined>(undefined);
  const [table, setTable] = useState<StateTable | null>(null);
  // The first two answers, which every shape below is built from. Held here rather than
  // inside the shapes, because they are steps of the page in their own right.
  const [sourceName, setSourceName] = useState<string | null>(null);
  const [keyName, setKeyName] = useState<string | null>(null);
  const [amountName, setAmountName] = useState<string | null>(null);
  /** Which shape this table started as, so step three can read back what it answered. */
  const [started, setStarted] = useState<string | null>(null);
  // Set once the reducer has been taken over by hand. From then on it is the file, and
  // the builder above is only what it started from — there is no parser here to read an
  // edited file back into pickers, and pretending otherwise would silently discard it.
  const [code, setCode] = useState<string | null>(null);
  const [checked, setChecked] = useState<{
    ok: boolean;
    details?: string;
  } | null>(null);
  const [saving, setSaving] = useState(false);
  const [loadError, setLoadError] = useState<string | null>(null);
  const [error, setError] = useState<string | null>(null);

  useEffect(() => {
    if (!project) return;
    const failed = (e: unknown) => setLoadError(e instanceof Error ? e.message : String(e));
    control.sources(project).then(setSources).catch(failed);
    control
      .project(project)
      .then((p) => {
        setConfig(p.config);
        setReducers(p.reducers);
      })
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

  // The table is written into the reducers file, and the config gains the key naming
  // it if it doesn't have one yet: those two together are what gets saved.
  const file = project ? reducersFile(project) : "";
  const yaml = useMemo(
    () => (table && config ? withReducersKey(config, file) : null),
    [table, config, file],
  );
  const dsl = useMemo(
    () => (code !== null ? code : table ? withDslTable(reducers ?? "", table) : null),
    [code, table, reducers],
  );
  // What the builder can tell is unfinished. Hand-written code is checked by the server
  // instead, which is the only thing that can read it.
  const listed = table && code === null ? problems(table) : [];
  // Renaming the table in the file has to move the preview and the redirect with it.
  const tableName = (code !== null ? declaredName(code) : null) ?? table?.name ?? "";

  // Check with the server as it's written, once it's worth checking.
  useEffect(() => {
    if (!project || !yaml || !dsl || listed.length > 0) {
      setChecked(null);
      return;
    }
    const timer = setTimeout(() => {
      control
        .check(project, yaml, dsl ?? undefined)
        .then(() => setChecked({ ok: true }))
        .catch((e: unknown) =>
          setChecked({
            ok: false,
            details: e instanceof ApiError ? (e.details ?? e.message) : String(e),
          }),
        );
    }, 400);
    return () => clearTimeout(timer);
  }, [project, yaml, dsl, listed.length]);

  // The source is whichever is chosen, or the first one as soon as there are any: a
  // picker with nothing selected has no question to ask.
  const source = sources?.find((s) => s.name === sourceName) ?? sources?.[0];
  const keys = source ? keyFields(source) : [];
  const amounts = source ? amountFields(source) : [];
  const keyField = keys.find((f) => f.name === keyName) ?? keys[0];
  const amountField = amounts.find((f) => f.name === amountName) ?? amounts[0];

  const pickSource = (next: SourceInfo) => {
    setSourceName(next.name);
    // Its fields are different, so the key and the amount chosen for the last one mean
    // nothing here.
    setKeyName(null);
    setAmountName(null);
  };

  const startOver = () => {
    setStarted(null);
    setCode(null);
    setTable(null);
  };

  const save = useCallback(async () => {
    if (!project || !yaml || !dsl || !tableName) return;
    setSaving(true);
    setError(null);
    try {
      await control.update(project, yaml, dsl ?? undefined);
      router.push(
        `/tables?project=${encodeURIComponent(project)}&name=${encodeURIComponent(tableName)}`,
      );
    } catch (e) {
      setError(e instanceof ApiError ? (e.details ?? e.message) : String(e));
      setSaving(false);
    }
  }, [project, yaml, dsl, tableName, router]);

  if (mode === "single" || !base) {
    return (
      <div className="mx-auto mt-24 max-w-md px-6 text-center text-sm text-on-surface-variant">
        Open a project first: state tables belong to one.
      </div>
    );
  }

  return (
    <div>
      <PageHeader title={editing ? `Edit ${editing}` : "New state table"}>
        {checked?.ok && <span className="text-xs text-on-tertiary-container">checks out</span>}
        <Button tone="primary" disabled={saving || !checked?.ok} onClick={() => void save()}>
          {saving ? "Saving…" : editing ? "Save changes" : "Create table"}
        </Button>
      </PageHeader>

      <div className="flex max-w-6xl flex-col gap-6 px-8 py-6">
        {!table && !editing && (
          <div>
            <h2 className="text-xl font-semibold tracking-tight">What should this table hold?</h2>
            <p className="mt-1.5 max-w-2xl text-sm text-on-surface-variant text-pretty">
              A row, and what each record does to it. Nineveh folds every record in order and
              serves the result over REST with a change feed, like any other table.
            </p>
          </div>
        )}

        {/* Some of these aren't failures: a mirror or a log has no rules to edit, and
            saying "couldn't read this project" over that explains nothing. */}
        {loadError &&
          (loadError.includes("no rules to edit") ? (
            <Notice tone="neutral" title="Nothing to edit here">
              {loadError}. Only tables built by reducers have rules; use New state table to fold
              these records into one of your own.
            </Notice>
          ) : (
            <Notice tone="error" title="Couldn't read this project">
              {loadError}
            </Notice>
          ))}
        {error && (
          <Notice tone="error" title="That didn't save">
            <pre className="overflow-x-auto font-mono text-xs whitespace-pre-wrap">{error}</pre>
          </Notice>
        )}
        {sources?.length === 0 && (
          <Notice tone="warning" title="This project follows nothing yet">
            Add a source to its config first.
          </Notice>
        )}

        {/* The sequence. A saved table skips the first three: it already has a source and
            a key, and there is no parser here to read its rules back into those pickers. */}
        {sources && isFilled(sources) && source && !editing && (
          <>
            <Step
              n={1}
              title="What are you folding?"
              hint="A reducer folds one source's records. Until that's chosen there are no field names to offer, which is why it comes first."
              answer={
                table && (
                  <>
                    <span className="font-mono text-on-surface">{source.name}</span> ({source.kind})
                  </>
                )
              }
            >
              {!table && (
                <div className="flex flex-col gap-4">
                  <Select
                    value={source.name}
                    onChange={(e) =>
                      pickSource(sources.find((s) => s.name === e.target.value) ?? sources[0])
                    }
                    className="w-full max-w-sm font-mono"
                  >
                    {sources.map((s) => (
                      <option key={s.name} value={s.name}>
                        {s.name} ({s.kind})
                      </option>
                    ))}
                  </Select>
                  <SourceSchema source={source} />
                </div>
              )}
            </Step>

            <Step
              n={2}
              title="What is one row?"
              hint="The key, and the table's whole meaning: one row per seller counts sellers, one row per day draws a chart. Everything else is derived from this."
              answer={
                table &&
                keyField && (
                  <>
                    one row per{" "}
                    <span className="font-mono text-on-surface">{keyField.name}</span>
                  </>
                )
              }
            >
              {!table && (
                <div className="flex flex-wrap items-end gap-3">
                  {keys.length > 0 ? (
                    <label className="flex flex-col gap-1.5 text-xs font-medium text-on-surface-variant">
                      One row per
                      <Select
                        value={keyField?.name ?? ""}
                        onChange={(e) => setKeyName(e.target.value)}
                        className="font-mono"
                      >
                        {keys.map((f) => (
                          <option key={f.name} value={f.name}>
                            {f.name} ({f.type})
                          </option>
                        ))}
                      </Select>
                    </label>
                  ) : (
                    <p className="text-xs text-on-surface-variant">
                      Nothing on <span className="font-mono">{source.name}</span> can identify a
                      row. Start from an empty table below and write the key yourself.
                    </p>
                  )}
                  {amounts.length > 0 && (
                    <label className="flex flex-col gap-1.5 text-xs font-medium text-on-surface-variant">
                      Adding up
                      <Select
                        value={amountField?.name ?? ""}
                        onChange={(e) => setAmountName(e.target.value)}
                        className="font-mono"
                      >
                        {amounts.map((f) => (
                          <option key={f.name} value={f.name}>
                            {f.name} ({f.type})
                          </option>
                        ))}
                      </Select>
                    </label>
                  )}
                </div>
              )}
            </Step>

            <Step
              n={3}
              title="Start from a shape"
              hint={
                table
                  ? undefined
                  : "Each one arrives with its columns typed, its defaults set and its widths wide enough to hold the totals. Change anything about it afterwards."
              }
              answer={table && started && <span className="text-on-surface">{started}</span>}
              action={table && <Undo onClick={startOver}>Start over</Undo>}
            >
              {!table && (
                <Shapes
                  source={source}
                  sources={sources}
                  keyField={keyField}
                  amountField={amountField}
                  existing={existing}
                  onPick={(name, picked) => {
                    setStarted(name);
                    setTable(picked);
                  }}
                />
              )}
            </Step>
          </>
        )}

        {sources && table && (
          <>
            {code === null && (
              <>
                <Step
                  n={editing ? 1 : 4}
                  title="What does it keep?"
                  hint="One column per thing a row remembers, and the key ticked. An integer starts at zero so a rule can add to a row that doesn't exist yet; a column with no default has to be set by any rule that creates one."
                >
                  <Columns table={table} onChange={setTable} />
                </Step>
                <Step
                  n={editing ? 2 : 5}
                  title="How does each record change it?"
                  hint="One rule per way a row moves. Several sources can write the same column — deposits add, withdrawals subtract — and the rules apply in this order, once per record, in version order."
                >
                  <Rules
                    sources={sources}
                    existing={existing}
                    table={table}
                    onChange={setTable}
                  />
                </Step>
              </>
            )}
            <Step
              n={editing ? 3 : 6}
              title={code === null ? "What gets saved" : "Your reducers"}
              hint={
                code === null
                  ? "The builder writes this. Take it over and the builder stops being the source of truth — there is no parser here to read an edited file back into pickers."
                  : undefined
              }
            >
            <Card className="overflow-hidden">
              <div className="flex flex-wrap items-center justify-between gap-3 border-b border-outline-variant px-4 py-2">
                {/* No filename: there is a file under this, but only the people who run
                    Nineveh themselves ever see it, and everyone else is just writing
                    reducers. */}
                <span className="text-xs text-on-surface-variant">
                  {code === null ? (
                    <>
                      Your reducers, as this will be saved
                    </>
                  ) : (
                    <>
                      Your reducers — <span className="text-on-surface">yours now</span>. The
                      builder above is what it started from.
                    </>
                  )}
                </span>
                <div className="flex items-center gap-3">
                  {code === null && (
                    <button
                      type="button"
                      onClick={() => setCode(withDslTable(reducers ?? "", table))}
                      className="text-xs font-medium text-primary hover:underline"
                    >
                      Edit it yourself
                    </button>
                  )}
                  <Undo onClick={startOver}>
                    {code !== null ? "Throw it away" : editing ? "Discard changes" : "Start over"}
                  </Undo>
                </div>
              </div>
              {/* `wrap="off"`: a file that reflows mid-identifier is unreadable, so it
                  scrolls sideways the way an editor does. */}
              {code === null ? (
                <pre className="max-h-64 overflow-auto px-4 py-3 font-mono text-xs leading-relaxed">
                  {toDsl(table)}
                </pre>
              ) : (
                <textarea
                  value={code}
                  onChange={(e) => setCode(e.target.value)}
                  spellCheck={false}
                  wrap="off"
                  autoFocus
                  aria-label="Reducers"
                  rows={Math.min(30, Math.max(12, code.split("\n").length + 1))}
                  className="block w-full resize-y overflow-auto bg-surface-container-low px-4 py-3 font-mono text-[13px] leading-relaxed text-on-surface outline-none focus:ring-1 focus:ring-inset focus:ring-primary"
                />
              )}
            </Card>
            {code !== null && (
              <p className="text-xs leading-relaxed text-on-surface-variant">
                These are all of this project's reducers, in the{" "}
                <a
                  href="https://www.nineveh.dev/docs/reducers"
                  target="_blank"
                  rel="noreferrer"
                  className="text-primary hover:underline"
                >
                  reducer language
                </a>
                . Nineveh checks them as you type, and saving is held until they pass.
              </p>
            )}
            </Step>
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
            {project && yaml && tableName && (
              <Step
                n={editing ? 4 : 7}
                title="Try it on real data"
                hint="Folded over a window of the chain without saving anything. It is the only step that can tell you the rules are right rather than merely legal."
              >
                <PreviewCard
                  project={project}
                  yaml={yaml}
                  reducers={dsl ?? undefined}
                  name={tableName}
                  columns={code === null ? table.columns.map((c) => c.name) : null}
                  ready={checked?.ok === true}
                />
              </Step>
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
  reducers,
  name,
  columns,
  ready,
}: {
  project: string;
  yaml: string;
  reducers: string | undefined;
  name: string;
  /**
   * The columns to show, in the order the builder put them in — or `null` for a reducer
   * written by hand, whose columns Studio can't know until the rows come back.
   */
  columns: string[] | null;
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
      setPreview(await control.preview(project, yaml, name, reducers));
    } catch (e) {
      setError(e instanceof ApiError ? (e.details ?? e.message) : String(e));
    } finally {
      setRunning(false);
    }
  }, [project, yaml, reducers, name]);

  const shown =
    columns ?? [...new Set((preview?.rows ?? []).flatMap((row) => Object.keys(row)))];
  return (
    <Card className="overflow-hidden">
      <div className="flex items-center justify-between gap-3 border-b border-outline-variant px-4 py-2">
        <span className="text-xs text-on-surface-variant">
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
            <pre className="overflow-x-auto font-mono text-xs whitespace-pre-wrap">{error}</pre>
          </Notice>
        </div>
      )}
      {preview && !error && preview.rows.length === 0 && (
        <p className="px-4 py-3 text-sm text-on-surface-variant">
          Nothing in the last {preview.transactions} transactions fed this table. That isn&apos;t a
          problem with the rules — try again once the contract has been used.
        </p>
      )}
      {preview && preview.rows.length > 0 && (
        <div className="overflow-x-auto">
          <table className="w-full text-left text-sm">
            <thead className="border-b border-outline-variant text-xs text-on-surface-variant">
              <tr>
                {shown.map((column) => (
                  <th key={column} className="px-4 py-1.5 font-medium">
                    {column}
                  </th>
                ))}
              </tr>
            </thead>
            <tbody>
              {preview.rows.map((row, i) => (
                <tr key={i} className="border-b border-outline-variant last:border-0">
                  {shown.map((column) => (
                    <td key={column} className="px-4 py-1.5 font-mono text-xs">
                      {cell(row[column])}
                    </td>
                  ))}
                </tr>
              ))}
            </tbody>
          </table>
          {preview.row_count > preview.rows.length && (
            <p className="px-4 py-2 text-xs text-on-surface-variant">
              and {preview.row_count - preview.rows.length} more.
            </p>
          )}
        </div>
      )}
    </Card>
  );
}

/**
 * The table a hand-written reducers file declares first, so the preview and the redirect
 * follow a rename made in the file. A regex rather than a parse: the real parser is in
 * Rust, this only needs the name, and getting it wrong costs a preview, not a save — the
 * server is what rejects a file that doesn't declare it.
 */
function declaredName(code: string): string | null {
  return /^export const ([A-Za-z_][A-Za-z0-9_]*) = table\(/m.exec(code)?.[1] ?? null;
}

/** One preview value, short enough for a cell. */
function cell(value: unknown): string {
  if (value === null || value === undefined) return "—";
  const text = typeof value === "object" ? JSON.stringify(value) : String(value);
  return text.length > 40 ? `${text.slice(0, 39)}…` : text;
}

/**
 * The shapes most state tables have, filled in from the chosen source and key.
 *
 * These are the front door, not a shortcut: `count per`, `total per`, `latest per` and
 * `per day` are most of what an app actually wants, and each arrives with its columns
 * typed, its defaults set and its width already widened — `sumPer` sums `u64`s into a
 * `u128` because a `u64` would overflow. Starting from an empty table is the escape
 * hatch underneath.
 */
function Shapes({
  source,
  sources,
  keyField,
  amountField,
  existing,
  onPick,
}: {
  source: SourceInfo;
  sources: [SourceInfo, ...SourceInfo[]];
  keyField: FieldInfo | undefined;
  amountField: FieldInfo | undefined;
  existing: Table[];
  onPick: (started: string, table: StateTable) => void;
}) {
  // What makes a row disappear again: this source's own deletes, or another source
  // that names the same key.
  const gone = source.deletes
    ? { name: source.name, deleted: true }
    : sources
        .filter((s) => s.name !== source.name)
        .filter((s) => s.fields.some((f) => f.name === keyField?.name && f.type === keyField?.type))
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

  return (
    <div className="flex flex-col gap-5">
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
          onClick={() => keyField && onPick("Count per row", countPer(source, keyField))}
        />
        <Template
          title="Total per row"
          body={
            keyField && amountField
              ? `${amountField.name} added up per ${keyField.name}, in a column wide enough to hold it.`
              : "This source has no amounts to add up."
          }
          disabled={!keyField || !amountField}
          shape={keyField && amountField && sumPer(source, keyField, amountField)}
          onClick={() =>
            keyField && amountField && onPick("Total per row", sumPer(source, keyField, amountField))
          }
        />
        <Template
          title="Latest per row"
          body={
            keyField ? `The newest ${source.name} for each ${keyField.name}, field by field.` : ""
          }
          disabled={!keyField}
          shape={keyField && latestPer(source, keyField)}
          onClick={() => keyField && onPick("Latest per row", latestPer(source, keyField))}
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
          onClick={() => keyField && onPick("Per day", dailyPer(source, keyField, amountField))}
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
            keyField && gone && onPick("Appears and disappears", liveSet(source, keyField, gone))
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
            keyField && joinable && withLookup(source, keyField, joinable.table, joinable.column)
          }
          onClick={() =>
            keyField &&
            joinable &&
            onPick(
              "With a value from another table",
              withLookup(source, keyField, joinable.table, joinable.column),
            )
          }
        />
      </div>

      <button
        type="button"
        onClick={() => onPick("An empty table", blank(source))}
        className="rounded-sm border border-dashed border-outline px-4 py-3 text-sm text-on-surface-variant transition-colors hover:border-primary hover:text-on-surface"
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
    <div className="mt-3 overflow-hidden rounded-sm border border-outline-variant bg-surface-container-low">
      <div className="flex items-center gap-3 border-b border-outline-variant px-2.5 py-1.5">
        {columns.map((column) => (
          <span
            key={column.name}
            className={`truncate font-mono text-[10px] ${
              column.key ? "font-medium text-primary" : "text-on-surface-variant"
            }`}
          >
            {column.name}
          </span>
        ))}
        {more > 0 && <span className="text-[10px] text-on-surface-variant/50">+{more}</span>}
      </div>
      {[0.7, 0.45].map((fade, row) => (
        <div key={row} className="flex items-center gap-3 px-2.5 py-1.5" style={{ opacity: fade }}>
          {columns.map((column, i) => (
            <span
              key={column.name}
              className="h-1.5 rounded-full bg-outline-variant"
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
      className="group flex flex-col rounded-sm border border-outline-variant bg-surface-container-low px-4 py-3.5 text-left shadow-e1 transition-all hover:-translate-y-px hover:border-primary hover:shadow-e3 disabled:cursor-not-allowed disabled:opacity-50 disabled:hover:translate-y-0 disabled:hover:border-outline-variant disabled:hover:shadow-e1"
    >
      <div className="text-sm font-semibold group-enabled:group-hover:text-on-secondary-container">
        {title}
      </div>
      <div className="mt-1 text-xs leading-relaxed text-on-surface-variant">{body}</div>
      {shape && <Shape table={shape} />}
    </button>
  );
}

/** The name, and the typed columns a row is made of. */
function Columns({
  table,
  onChange,
}: {
  table: StateTable;
  onChange: (table: StateTable) => void;
}) {
  const set = (changes: Partial<StateTable>) => onChange({ ...table, ...changes });
  const setColumn = (index: number, changes: Partial<Column>) =>
    set({
      columns: table.columns.map((c, i) => (i === index ? { ...c, ...changes } : c)),
    });

  const keyColumns = table.columns.filter((c) => c.key).map((c) => c.name);
  return (
    <>
      <Card className="overflow-hidden">
        <div className="flex flex-wrap items-end justify-between gap-3 border-b border-outline-variant bg-surface-container-high px-4 py-3">
          <label className="flex flex-col gap-1.5">
            <span className="text-xs font-medium text-on-surface-variant">Table name</span>
            <input
              value={table.name}
              onChange={(e) => set({ name: e.target.value })}
              spellCheck={false}
              className={`${field} w-72 font-mono text-[15px]`}
            />
          </label>
          <p className="pb-2 text-xs text-on-surface-variant">
            {keyColumns.length > 0 ? (
              <>
                one row per{" "}
                <span className="font-mono text-on-surface">{keyColumns.join(", ")}</span>
              </>
            ) : (
              "no key yet: tick the columns that identify a row"
            )}
          </p>
        </div>

        <div className="overflow-x-auto px-4 pt-3 pb-4">
          <table className="w-full text-sm">
            <thead className="text-left text-xs text-on-surface-variant">
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
                      onChange={(e) => setColumn(index, { name: e.target.value })}
                      spellCheck={false}
                      className={`${field} w-40 font-mono`}
                    />
                  </td>
                  <td className="py-1 pr-2">
                    <Select
                      value={column.type}
                      onChange={(e) => {
                        const type = e.target.value as ColumnType;
                        // Carry the default with the type: an integer column wants its
                        // zero, and keeping `0` on an address column would be a lie.
                        const kept = column.default === defaultFor(column.type);
                        setColumn(index, {
                          type,
                          default: kept ? defaultFor(type) : column.default,
                        });
                      }}
                      className="font-mono"
                    >
                      {COLUMN_TYPES.map((type) => (
                        <option key={type} value={type}>
                          {type}
                        </option>
                      ))}
                    </Select>
                  </td>
                  <td className="py-1 pr-2">
                    <input
                      value={column.default}
                      onChange={(e) => setColumn(index, { default: e.target.value })}
                      placeholder="none"
                      spellCheck={false}
                      className={`${field} w-24 font-mono`}
                    />
                  </td>
                  <td className="py-1 pr-2">
                    <input
                      type="checkbox"
                      checked={column.nullable}
                      onChange={(e) => setColumn(index, { nullable: e.target.checked })}
                      className="accent-primary"
                    />
                  </td>
                  <td className="py-1 pr-2">
                    <input
                      type="checkbox"
                      checked={column.key}
                      onChange={(e) => setColumn(index, { key: e.target.checked })}
                      className="accent-primary"
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
                      className="text-xs text-on-surface-variant hover:text-error"
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
                    default: defaultFor("u64"),
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
    </>
  );
}

/** One rule per way a record changes a row. */
function Rules({
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
  const set = (changes: Partial<StateTable>) => onChange({ ...table, ...changes });
  const setRule = (index: number, changes: Partial<Rule>) =>
    set({
      rules: table.rules.map((r, i) => (i === index ? { ...r, ...changes } : r)),
    });

  return (
    <div className="flex flex-col gap-4">
      {table.rules.map((rule, index) => (
        <RuleCard
          key={index}
          n={index + 1}
          sources={sources}
          existing={existing}
          table={table}
          rule={rule}
          onChange={(changes) => setRule(index, changes)}
          onRemove={() => set({ rules: table.rules.filter((_, i) => i !== index) })}
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
    </div>
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
  const readable = rule.deleted ? (source?.delete_fields ?? []) : (source?.fields ?? []);
  const keyColumns = table.columns.filter((c) => c.key);
  const mapped = new Set(rule.keys.map((k) => k.column));
  // A key column the record doesn't name has to be mapped.
  const unnamed = keyColumns.filter(
    (c) => !mapped.has(c.name) && !readable.some((f) => f.name === c.name),
  );

  // A placeholder naming a field of the chosen source, rather than an `amount` that may
  // well not exist on it: the example is only useful if it could be typed as it stands.
  // Numeric and readable here: a `.deleted` rule sees only the key fields, so the whole
  // source's amounts are the wrong list to draw from.
  const counted = source
    ? amountFields(source).filter((f) => readable.some((r) => r.name === f.name))
    : [];
  const example = `${counted[0]?.name ?? readable[0]?.name ?? "amount"} > 0`;

  // Both a field of the record and a column of the row: a bare name would be ambiguous.
  const ambiguous = (name: string) =>
    readable.some((f) => f.name === name) && table.columns.some((c) => c.name === name);

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
      className="rounded bg-surface-container-high px-1.5 py-0.5 font-mono hover:bg-secondary-container"
    >
      {text}
    </button>
  );

  return (
    <Card className="overflow-hidden">
      <div className="flex items-center justify-between gap-3 border-b border-outline-variant bg-surface-container-high px-4 py-2">
        <span className="text-xs font-semibold tracking-wide text-on-surface-variant uppercase">
          Rule {n}
        </span>
        <span className="truncate text-xs text-on-surface-variant">
          {rule.removes
            ? "deletes the row"
            : `sets ${rule.sets.length || "no"} column${rule.sets.length === 1 ? "" : "s"}`}
          {" on "}
          <span className="font-mono text-on-surface">
            {rule.on}
            {rule.deleted ? ".deleted" : ""}
          </span>
        </span>
      </div>
      <div className="p-4">
        <div className="flex flex-wrap items-end gap-3">
          <label className="flex flex-col gap-1.5 text-xs font-medium text-on-surface-variant">
            On each record from
            <Select
              value={rule.on}
              onChange={(e) => onChange({ on: e.target.value, deleted: false })}
              className="font-mono"
            >
              {sources.map((s) => (
                <option key={s.name} value={s.name}>
                  {s.name}
                </option>
              ))}
            </Select>
          </label>
          {source?.deletes && (
            <label className="flex items-center gap-2 pb-2 text-xs text-on-surface-variant">
              <input
                type="checkbox"
                checked={rule.deleted}
                onChange={(e) => onChange({ deleted: e.target.checked })}
                className="accent-primary"
              />
              when it&apos;s deleted
            </label>
          )}
          <label className="flex flex-1 basis-64 flex-col gap-1.5 text-xs font-medium text-on-surface-variant">
            Only when (optional)
            <ExpressionInput
              value={rule.when}
              onChange={(when) => onChange({ when })}
              names={names}
              placeholder={example}
              onActive={(handle) => (active.current = handle)}
            />
            {/* A labelled box with a placeholder says nothing about what is legal in it.
                What it needs said is that blank is the common answer and that the thing
                must be true or false: a bare `price` is a type error, not a truth test. */}
            <span className="text-[11px] font-normal text-on-surface-variant">
              True or false, not a value — blank runs on every record. Compare with{" "}
              <span className="font-mono text-on-surface">{"== != < <= > >="}</span>, join with{" "}
              <span className="font-mono text-on-surface">{"&& || !"}</span>.{" "}
              <a
                href="https://www.nineveh.dev/docs/expressions"
                target="_blank"
                rel="noreferrer"
                className="text-primary hover:underline"
              >
                The language
              </a>
            </span>
          </label>
          <button
            type="button"
            onClick={onRemove}
            className="pb-2 text-xs text-on-surface-variant hover:text-error"
          >
            Remove rule
          </button>
        </div>

        {source && (
          <div className="mt-3">
            <SourceSchema
              source={source}
              deleted={rule.deleted}
              // A name that is also a column of this table has to be inserted qualified,
              // the same way `namesInScope` offers it, or the expression is ambiguous.
              onInsert={(name) =>
                active.current?.insert(ambiguous(name) ? `${rule.on}.${name}` : name)
              }
            />
          </div>
        )}

        <div className="mt-2 flex flex-wrap items-center gap-1.5 text-xs text-on-surface-variant">
          Also in scope:
          {table.columns.filter((c) => !c.key).map((c) => chip(c.name))}
          {chip("tx.timestamp")}
          {existing.map((t) => chip(`${t.name}[${t.key.join(", ")}]`, `${t.name}[`))}
        </div>

        {unnamed.length > 0 && (
          <p className="mt-2 text-xs text-on-warning-container">
            Say where {unnamed.map((c) => c.name).join(", ")} comes from: the record has no field of
            that name.
          </p>
        )}

        <label className="mt-3 flex items-center gap-2 text-sm">
          <input
            type="checkbox"
            checked={rule.removes}
            onChange={(e) => onChange({ removes: e.target.checked })}
            className="accent-primary"
          />
          Delete the row instead of setting columns
        </label>

        {!rule.removes && (
          <div className="mt-2">
            <div className="text-xs font-medium text-on-surface-variant">Set</div>
            {rule.sets.map((assignment, index) => (
              <div key={index} className="mt-1.5 flex items-center gap-2">
                <Select
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
                </Select>
                <span className="text-on-surface-variant">=</span>
                <ExpressionInput
                  value={assignment.expression}
                  onChange={(expression) =>
                    onChange({
                      sets: rule.sets.map((s, i) => (i === index ? { ...s, expression } : s)),
                    })
                  }
                  names={names}
                  placeholder="count + 1"
                  onActive={(handle) => (active.current = handle)}
                />
                <button
                  type="button"
                  onClick={() => onChange({ sets: rule.sets.filter((_, i) => i !== index) })}
                  className="text-xs text-on-surface-variant hover:text-error"
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
            <div className="text-xs font-medium text-on-surface-variant">
              Key columns from the record
            </div>
            {rule.keys.map((assignment, index) => (
              <div key={index} className="mt-1.5 flex items-center gap-2">
                <Select
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
                </Select>
                <span className="text-on-surface-variant">=</span>
                <ExpressionInput
                  value={assignment.expression}
                  onChange={(expression) =>
                    onChange({
                      keys: rule.keys.map((k, i) => (i === index ? { ...k, expression } : k)),
                    })
                  }
                  names={keyNames}
                  placeholder="key"
                  onActive={(handle) => (active.current = handle)}
                />
                <button
                  type="button"
                  onClick={() => onChange({ keys: rule.keys.filter((_, i) => i !== index) })}
                  className="text-xs text-on-surface-variant hover:text-error"
                >
                  Remove
                </button>
              </div>
            ))}
            <Button
              className="mt-2"
              onClick={() =>
                onChange({
                  keys: [...rule.keys, { column: unnamed[0]?.name ?? "", expression: "" }],
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
