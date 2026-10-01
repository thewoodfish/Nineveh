"use client";

import { useRouter, useSearchParams } from "next/navigation";
import { PageHeader } from "@/components/page-header";
import {
  Fragment,
  Suspense,
  useCallback,
  useEffect,
  useMemo,
  useRef,
  useState,
  type ReactNode,
} from "react";

import { SourceSchema } from "@/components/source-schema";
import { Button, Card, Icon, Notice, Select } from "@/components/ui";
import {
  ApiError,
  type FieldInfo,
  type Preview,
  type SourceInfo,
  type Table,
  control,
  getTables,
} from "@/lib/api";
import { useProject } from "@/lib/project";
import {
  type StateTable,
  amountFields,
  blank,
  countPer,
  dslTableBlock,
  dailyPer,
  keyFields,
  latestPer,
  liveSet,
  sumPer,
  withLookup,
  withDslTable,
  withReducersKey,
  reducersFile,
} from "@/lib/state-table";


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
    /* The rail: a line down the left with the number on it, so the steps read as one
       sequence rather than as a stack of unrelated headings. The last one's line runs
       out on its own, since there is nothing below it to join. */
    <section className="group relative flex gap-4 pb-1">
      <div className="flex shrink-0 flex-col items-center">
        <span
          aria-hidden
          className={`flex size-7 items-center justify-center rounded-full text-xs font-medium ${
            answer
              ? "bg-primary-container text-on-primary-container"
              : "bg-surface-container-high text-on-surface-variant"
          }`}
        >
          {n}
        </span>
        <span aria-hidden className="mt-1 w-px flex-1 bg-outline-variant group-last:hidden" />
      </div>
      <div className="min-w-0 flex-1 pb-6">
        <div className="flex min-h-7 flex-wrap items-center gap-x-3 gap-y-1">
          <h2 className="text-[15px] leading-6 font-medium text-on-surface">{title}</h2>
          {answer && (
            <span className="min-w-0 truncate text-xs text-on-surface-variant">{answer}</span>
          )}
          {action && <span className="ml-auto">{action}</span>}
        </div>
        {hint && !answer && (
          <p className="mt-1.5 max-w-prose text-xs leading-relaxed text-on-surface-variant">
            {hint}
          </p>
        )}
        {children && <div className="mt-3.5">{children}</div>}
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
  /** Which shape this table started as, so its step can read back what it answered. */
  const [started, setStarted] = useState<string | null>(null);
  // Held apart from the table so it can be answered before there is one. A shape brings
  // its own name (`listed_per_owner`), which is only taken if nothing has been typed.
  const [name, setName] = useState("");
  const editor = useRef<HTMLTextAreaElement>(null);
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
        .then((saved) => {
          setName(saved.name);
          setTable({ name: saved.name, columns: saved.columns, rules: saved.rules });
        })
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
    () => (code !== null && config ? withReducersKey(config, file) : null),
    [code, config, file],
  );
  const dsl = code;
  // Renaming the table in the file has to move the preview and the redirect with it.
  const tableName = (code !== null ? declaredName(code) : null) ?? name.trim();

  // Opening a saved table puts its file on screen. One already in the reducers file is
  // shown as it was written, comments and all; one declared in YAML is rendered into it.
  useEffect(() => {
    if (!editing || !table || reducers === undefined || code !== null) return;
    setCode(
      dslTableBlock(reducers ?? "", table.name) !== null
        ? (reducers ?? "")
        : withDslTable(reducers ?? "", table),
    );
  }, [editing, table, reducers, code]);

  // Check with the server as it's written, once it's worth checking.
  useEffect(() => {
    if (!project || !yaml || !dsl) {
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
  }, [project, yaml, dsl]);

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

  const rename = setName;

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
        <Button tone="primary" disabled={saving || !checked?.ok} onClick={() => void save()}>
          {saving ? "Saving…" : editing ? "Save changes" : "Create table"}
        </Button>
      </PageHeader>

      <div className="flex max-w-6xl flex-col gap-0 px-8 py-6">
        {code === null && !editing && (
          <div className="mb-8">
            <h2 className="text-xl font-semibold tracking-tight">What should this table hold?</h2>
            <p className="mt-1.5 max-w-2xl text-sm text-on-surface-variant text-pretty">
              A row, and what each record does to it. Nineveh folds every record in order and
              serves the result over REST with a change feed, like any other table.
            </p>
          </div>
        )}

        {/* Some of these aren't failures: a mirror or a log has no rules to edit, and a
            table declared in nineveh.yaml is edited there. Saying "couldn't read this
            project" over either explains nothing. */}
        {loadError && (
          <div className="mb-6">
            {loadError.includes("no rules to edit") ? (
            <Notice tone="neutral" title="Nothing to edit here">
              {loadError}. Only tables built by reducers have rules; use New state table to fold
              these records into one of your own.
            </Notice>
          ) : loadError.includes("declared in nineveh.yaml") ? (
            <Notice tone="neutral" title="This one lives in the config">
              {loadError}.
            </Notice>
          ) : (
            <Notice tone="error" title="Couldn't read this project">
                {loadError}
              </Notice>
            )}
          </div>
        )}
        {error && (
          <div className="mb-6">
            <Notice tone="error" title="That didn't save">
              <pre className="overflow-x-auto font-mono text-xs whitespace-pre-wrap">{error}</pre>
            </Notice>
          </div>
        )}
        {sources?.length === 0 && (
          <div className="mb-6">
            <Notice tone="warning" title="This project follows nothing yet">
              Add a source to its config first.
            </Notice>
          </div>
        )}

        {/* The sequence. The name first: it is the one answer that depends on nothing, and
            a page that opens by asking for a word is easier to start than one that opens by
            asking about the data model. A saved table skips the three after it — it already
            has a source and a key, and there is no parser here to read its rules back into
            those pickers. */}
        <Step
          n={1}
          title="What will you call it?"
          hint="The table's name in the REST API, the change feed and the reducers file. Lower snake case."
        >
          <input
            value={name}
            onChange={(e) => rename(e.target.value)}
            spellCheck={false}
            autoFocus={!editing}
            placeholder={source ? `${source.name}_per_owner` : "orders_per_trader"}
            className={`${field} w-full max-w-sm font-mono text-[15px]`}
          />
        </Step>

        {sources && isFilled(sources) && source && !editing && (
          <>
            <Step
              n={2}
              title="What are you folding?"
              hint="The source whose records bring rows into being. It needn't be the only one — a rule further down can fold a second source into the same table, which is how a balance goes up on deposits and down on withdrawals."
              answer={
                code !== null && (
                  <>
                    <span className="font-mono text-on-surface">{source.name}</span> ({source.kind})
                  </>
                )
              }
            >
              {code === null && (
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
              n={3}
              title="What is one row?"
              hint="The key, and the table's whole meaning: one row per seller counts sellers, one row per day draws a chart. Everything else is derived from this."
              answer={
                code !== null &&
                keyField && (
                  <>
                    one row per{" "}
                    <span className="font-mono text-on-surface">{keyField.name}</span>
                  </>
                )
              }
            >
              {code === null && (
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
              n={4}
              title="Start from a shape"
              hint={
                code !== null
                  ? undefined
                  : "Each one arrives with its columns typed, its defaults set and its widths wide enough to hold the totals. Change anything about it afterwards."
              }
              answer={code !== null && started && <span className="text-on-surface">{started}</span>}
            >
              {code === null && (
                <Shapes
                  source={source}
                  sources={sources}
                  keyField={keyField}
                  amountField={amountField}
                  existing={existing}
                  onPick={(shape, picked) => {
                    setStarted(shape);
                    const chosen = name.trim() || picked.name;
                    setName(chosen);
                    setCode(withDslTable(reducers ?? "", { ...picked, name: chosen }));
                  }}
                />
              )}
            </Step>
          </>
        )}

        {sources && code !== null && (
          <>
            {/* Columns and rules were two steps and two forms. They are one file: the
                DSL declares what a row holds and what writes it in the same block, and
                splitting that across two surfaces gave one table two sources of truth.
                The schema sits beside the code instead of inside every rule, where it
                used to be repeated once per rule. */}
            <Step
              n={editing ? 2 : 5}
              title="Write the fold"
              hint="What a row holds, and what each record does to it. `b` is the row this rule writes and `r` is the record it is folding; `tx.version` and `tx.timestamp` are the only clock there is."
              action={
                <Undo onClick={startOver}>
                  {editing ? "Discard changes" : "Start over"}
                </Undo>
              }
            >
              <div className="grid gap-4 lg:grid-cols-[1fr_20rem] lg:items-start">
                <Editor
                  ref={editor}
                  file={file}
                  code={code}
                  onChange={setCode}
                  status={
                    checked?.ok
                      ? { tone: "ok", text: "checks out" }
                      : checked
                        ? { tone: "bad", text: "won't build" }
                        : { tone: "quiet", text: "checking…" }
                  }
                />
                <div className="flex flex-col gap-3">
                  <Palette sources={sources} tables={existing} />
                  <p className="text-xs leading-relaxed text-on-surface-variant">
                    This is the whole reducers file, in the{" "}
                    <a
                      href="https://www.nineveh.dev/docs/reducers"
                      target="_blank"
                      rel="noreferrer"
                      className="text-primary hover:underline"
                    >
                      reducer language
                    </a>
                    . Nineveh checks it as you type, and saving is held until it passes.
                  </p>
                </div>
              </div>
            </Step>
            {checked && !checked.ok && (
              <div className="mb-6 ml-11">
                <Notice tone="error" title="Nineveh can't build that">
                  <pre className="overflow-x-auto font-mono text-xs whitespace-pre">
                    {checked.details}
                  </pre>
                </Notice>
              </div>
            )}
            {project && yaml && tableName && (
              <Step
                n={editing ? 3 : 6}
                title="Try it on real data"
                hint="Folded over a window of the chain without saving anything. It is the only step that can tell you the rules are right rather than merely legal."
              >
                <PreviewCard
                  project={project}
                  yaml={yaml}
                  reducers={dsl ?? undefined}
                  name={tableName}
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
  ready,
}: {
  project: string;
  yaml: string;
  reducers: string | undefined;
  name: string;
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

  // The file says what the columns are and Studio doesn't parse it, so they come back
  // with the rows.
  const shown = [...new Set((preview?.rows ?? []).flatMap((row) => Object.keys(row)))];
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
 * The reducers file, with the chrome an editor has.
 *
 * Line numbers because the errors are located — `./market.nineveh.ts:11:14` — and a bare
 * textarea leaves you counting. The gutter is a second element rather than anything
 * clever inside the box, so it has to be kept in step: same line height, and scrolled to
 * match whenever the text is.
 */
function Editor({
  ref,
  file,
  code,
  onChange,
  status,
}: {
  ref: React.RefObject<HTMLTextAreaElement | null>;
  file: string;
  code: string;
  onChange: (code: string) => void;
  status: { tone: "ok" | "bad" | "quiet"; text: string };
}) {
  const gutter = useRef<HTMLDivElement>(null);
  const lines = code.split("\n").length;
  const tone =
    status.tone === "ok"
      ? "text-on-tertiary-container"
      : status.tone === "bad"
        ? "text-error"
        : "text-on-surface-variant";

  return (
    <Card className="overflow-hidden">
      <div className="flex items-center gap-3 border-b border-outline-variant bg-surface-container-high px-3 py-2">
        <Icon name="code" className="text-[16px] text-on-surface-variant" />
        <span className="min-w-0 flex-1 truncate font-mono text-xs text-on-surface">{file}</span>
        <span className={`shrink-0 text-[11px] ${tone}`}>{status.text}</span>
      </div>
      <div className="flex">
        <div
          ref={gutter}
          aria-hidden
          className="shrink-0 overflow-hidden border-r border-outline-variant bg-surface-container py-3 font-mono text-[13px] leading-relaxed text-on-surface-variant/70 select-none"
        >
          {Array.from({ length: lines }, (_, i) => (
            <div key={i} className="px-2.5 text-right tabular-nums">
              {i + 1}
            </div>
          ))}
        </div>
        {/* `wrap="off"`: a file that reflows mid-identifier is unreadable, so it scrolls
            sideways the way an editor does — and a wrapped line would put the gutter out
            of step with the text beside it. */}
        <textarea
          ref={ref}
          value={code}
          onChange={(e) => onChange(e.target.value)}
          onScroll={(e) => {
            if (gutter.current) gutter.current.scrollTop = e.currentTarget.scrollTop;
          }}
          spellCheck={false}
          wrap="off"
          aria-label="Reducers"
          rows={Math.min(34, Math.max(14, lines + 1))}
          className="block min-w-0 flex-1 resize-y overflow-auto bg-surface-container-low px-3 py-3 font-mono text-[13px] leading-relaxed text-on-surface outline-none focus:ring-1 focus:ring-inset focus:ring-primary"
        />
      </div>
    </Card>
  );
}

/**
 * What this project has, to write the file against.
 *
 * Reference, not a tool: a reducers file folds as many sources as it likes and reads
 * other tables by key (ADR 0019), and the question halfway through a line is "what is
 * that field called". It answers that and stays out of the way — nothing here types for
 * you, because a panel that edits the file from the side is a second way to write it.
 */
function Palette({ sources, tables }: { sources: SourceInfo[] | null; tables: Table[] }) {
  const [open, setOpen] = useState<string | null>(null);

  const entry = (
    id: string,
    name: string,
    kind: string,
    sub: string,
    fields: { name: string; type: string }[],
  ) => {
    const shown = open === id;
    return (
      <div key={id}>
        <button
          type="button"
          onClick={() => setOpen(shown ? null : id)}
          aria-expanded={shown}
          className="state flex w-full items-center gap-2 rounded-xs px-2 py-1.5 text-left"
        >
          <Icon
            name="chevron_right"
            className={`shrink-0 text-[16px] text-on-surface-variant transition-transform ${
              shown ? "rotate-90" : ""
            }`}
          />
          <span className="min-w-0 flex-1 truncate font-mono text-xs text-on-surface">{name}</span>
          <span className="shrink-0 rounded-full bg-surface-container-high px-1.5 py-0.5 text-[10px] text-on-surface-variant">
            {kind}
          </span>
        </button>
        {shown && (
          <div className="mb-1 ml-6 border-l border-outline-variant pl-3">
            <p className="py-1 text-[11px] text-on-surface-variant">{sub}</p>
            <dl className="grid grid-cols-[1fr_auto] gap-x-4">
              {fields.map((f) => (
                <Fragment key={f.name}>
                  <dt className="truncate py-0.5 font-mono text-[11px] text-on-surface">
                    {f.name}
                  </dt>
                  <dd className="py-0.5 font-mono text-[11px] text-on-surface-variant">{f.type}</dd>
                </Fragment>
              ))}
            </dl>
          </div>
        )}
      </div>
    );
  };

  return (
    <Card className="divide-y divide-outline-variant">
      <div className="px-3 py-2.5">
        <h3 className="text-xs font-medium text-on-surface">Sources</h3>
        <p className="mt-0.5 text-[11px] text-on-surface-variant">
          What a handler can fold. Its fields are on the record — <code>r.name</code>.
        </p>
        <div className="mt-1.5">
          {(sources ?? []).map((source) =>
            entry(
              `s:${source.name}`,
              source.name,
              source.kind,
              `Every field a ${source.kind} record carries.`,
              source.fields.map((f) => ({ name: f.name, type: f.type })),
            ),
          )}
        </div>
      </div>
      {tables.length > 0 && (
        <div className="px-3 py-2.5">
          <h3 className="text-xs font-medium text-on-surface">Tables you can read</h3>
          <p className="mt-0.5 text-[11px] text-on-surface-variant">
            By key, and <code>null</code> when there is no such row —{" "}
            <code>name.get(key)?.column</code>.
          </p>
          <div className="mt-1.5">
            {tables.map((table) =>
              entry(
                `t:${table.name}`,
                table.name,
                table.kind,
                `Keyed by ${table.key.join(", ")}.`,
                table.columns.map((c) => ({ name: c.name, type: c.type })),
              ),
            )}
          </div>
        </div>
      )}
      <div className="px-3 py-2.5">
        <h3 className="text-xs font-medium text-on-surface">Always there</h3>
        <dl className="mt-1.5 grid grid-cols-[1fr_auto] gap-x-4">
          <dt className="py-0.5 font-mono text-[11px] text-on-surface">tx.version</dt>
          <dd className="py-0.5 font-mono text-[11px] text-on-surface-variant">u64</dd>
          <dt className="py-0.5 font-mono text-[11px] text-on-surface">tx.timestamp</dt>
          <dd className="py-0.5 font-mono text-[11px] text-on-surface-variant">u64</dd>
        </dl>
        <p className="mt-1.5 text-[11px] leading-relaxed text-on-surface-variant">
          The only clock there is. A reducer that read the wall clock would fold a
          different answer on replay.
        </p>
      </div>
    </Card>
  );
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
