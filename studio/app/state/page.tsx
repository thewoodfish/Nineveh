"use client";

import { useRouter } from "next/navigation";
import { useCallback, useEffect, useMemo, useState } from "react";

import { Button, Card, Notice, PageHeader } from "@/components/ui";
import {
  ApiError,
  type ColumnType,
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
  keyFields,
  latestPer,
  problems,
  sumPer,
  toYaml,
  withTable,
} from "@/lib/state-table";

/** Narrows a list to one with a first element, so pickers always have a selection. */
function isFilled(sources: SourceInfo[]): sources is [SourceInfo, ...SourceInfo[]] {
  return sources.length > 0;
}

const field =
  "rounded-md border border-zinc-200 bg-white px-2.5 py-1.5 text-sm focus:border-lapis-400 focus:outline-none dark:border-zinc-700 dark:bg-zinc-900";

/**
 * Design a state table: a key, typed columns, and rules that fold records into them.
 * The control plane checks the config as it's written, and saving it rebuilds the
 * project's tables (ADR 0016).
 */
export default function NewStateTable() {
  const { name: project, mode, base } = useProject();
  const router = useRouter();
  const [sources, setSources] = useState<SourceInfo[] | null>(null);
  const [existing, setExisting] = useState<Table[]>([]);
  const [config, setConfig] = useState<string | null>(null);
  const [table, setTable] = useState<StateTable | null>(null);
  const [checked, setChecked] = useState<{ ok: boolean; details?: string } | null>(null);
  const [saving, setSaving] = useState(false);
  const [loadError, setLoadError] = useState<string | null>(null);
  const [error, setError] = useState<string | null>(null);

  useEffect(() => {
    if (!project) return;
    const failed = (e: unknown) => setLoadError(e instanceof Error ? e.message : String(e));
    control.sources(project).then(setSources).catch(failed);
    control
      .project(project)
      .then((p) => setConfig(p.config))
      .catch(failed);
  }, [project]);

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
            details: e instanceof ApiError ? (e.details ?? e.message) : String(e),
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
      router.push(`/tables?project=${encodeURIComponent(project)}&name=${encodeURIComponent(table.name)}`);
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
      <PageHeader title="New state table">
        {checked?.ok && <span className="text-xs text-emerald-600">checks out</span>}
        <Button
          tone="primary"
          disabled={saving || !checked?.ok}
          onClick={() => void save()}
        >
          {saving ? "Saving…" : "Create table"}
        </Button>
      </PageHeader>

      <div className="mx-auto flex max-w-5xl flex-col gap-6 px-8 py-6">
        <p className="text-sm text-zinc-500">
          Your backend&apos;s own table: you say what a row is and how each record changes it.
          Nineveh folds every record into it in order, and serves it over REST with a change
          feed, like any other table.
        </p>

        {loadError && (
          <Notice tone="error" title="Couldn&apos;t read this project">
            {loadError}
          </Notice>
        )}
        {error && (
          <Notice tone="error" title="That didn&apos;t save">
            <pre className="overflow-x-auto font-mono text-xs whitespace-pre-wrap">{error}</pre>
          </Notice>
        )}
        {sources?.length === 0 && (
          <Notice tone="warning" title="This project follows nothing yet">
            Add a source to its config first.
          </Notice>
        )}

        {sources && isFilled(sources) && !table && <Templates sources={sources} onPick={setTable} />}

        {sources && table && (
          <>
            <Editor sources={sources} existing={existing} table={table} onChange={setTable} />
            <Card className="overflow-hidden">
              <div className="flex items-center justify-between border-b border-zinc-200 px-4 py-2 dark:border-zinc-800">
                <span className="text-xs text-zinc-500">
                  <span className="font-mono">nineveh.yaml</span>, as this will be saved
                </span>
                <button
                  type="button"
                  onClick={() => setTable(null)}
                  className="text-xs text-zinc-500 hover:text-zinc-900 dark:hover:text-zinc-100"
                >
                  Start over
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
              <Notice tone="error" title="Nineveh can&apos;t build that">
                <pre className="overflow-x-auto font-mono text-xs whitespace-pre">{checked.details}</pre>
              </Notice>
            )}
          </>
        )}
      </div>
    </div>
  );
}

/** The shapes most state tables have, filled in from a source's fields. */
function Templates({
  sources,
  onPick,
}: {
  sources: [SourceInfo, ...SourceInfo[]];
  onPick: (table: StateTable) => void;
}) {
  const [source, setSource] = useState<SourceInfo>(sources[0]);
  const keys = keyFields(source);
  const amounts = amountFields(source);
  const [key, setKey] = useState(keys[0]?.name ?? "");
  const [amount, setAmount] = useState(amounts[0]?.name ?? "");
  const keyField = keys.find((f) => f.name === key) ?? keys[0];
  const amountField = amounts.find((f) => f.name === amount) ?? amounts[0];

  const pick = (source: SourceInfo) => {
    setSource(source);
    const keys = keyFields(source);
    const amounts = amountFields(source);
    setKey(keys[0]?.name ?? "");
    setAmount(amounts[0]?.name ?? "");
  };

  return (
    <Card className="p-4">
      <div className="flex flex-wrap items-end gap-3">
        <label className="flex flex-col gap-1.5 text-xs font-medium text-zinc-500">
          Fold records from
          <select
            value={source.name}
            onChange={(e) => pick(sources.find((s) => s.name === e.target.value) ?? sources[0])}
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
          <select value={key} onChange={(e) => setKey(e.target.value)} className={`${field} font-mono`}>
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
            <select value={amount} onChange={(e) => setAmount(e.target.value)} className={`${field} font-mono`}>
              {amounts.map((f) => (
                <option key={f.name} value={f.name}>
                  {f.name}
                </option>
              ))}
            </select>
          </label>
        )}
      </div>

      <div className="mt-4 grid gap-2 sm:grid-cols-2">
        <Template
          title="Count per row"
          body={keyField ? `How many ${source.name} records each ${keyField.name} has, and when it last happened.` : "This source has nothing to key by."}
          disabled={!keyField}
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
          onClick={() => keyField && amountField && onPick(sumPer(source, keyField, amountField))}
        />
        <Template
          title="Latest per row"
          body={keyField ? `The newest ${source.name} for each ${keyField.name}, field by field.` : ""}
          disabled={!keyField}
          onClick={() => keyField && onPick(latestPer(source, keyField))}
        />
        <Template
          title="Empty table"
          body="Name the columns and write the rules yourself."
          onClick={() => onPick(blank(source))}
        />
      </div>
    </Card>
  );
}

function Template({
  title,
  body,
  disabled = false,
  onClick,
}: {
  title: string;
  body: string;
  disabled?: boolean;
  onClick: () => void;
}) {
  return (
    <button
      type="button"
      disabled={disabled}
      onClick={onClick}
      className="rounded-lg border border-zinc-200 px-4 py-3 text-left transition-colors hover:border-lapis-400 disabled:cursor-not-allowed disabled:opacity-50 dark:border-zinc-800"
    >
      <div className="text-sm font-medium">{title}</div>
      <div className="mt-0.5 text-xs text-zinc-500">{body}</div>
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
  const set = (changes: Partial<StateTable>) => onChange({ ...table, ...changes });
  const setColumn = (index: number, changes: Partial<Column>) =>
    set({ columns: table.columns.map((c, i) => (i === index ? { ...c, ...changes } : c)) });
  const setRule = (index: number, changes: Partial<Rule>) =>
    set({ rules: table.rules.map((r, i) => (i === index ? { ...r, ...changes } : r)) });

  return (
    <>
      <Card className="p-4">
        <label className="flex flex-col gap-1.5 text-xs font-medium text-zinc-500">
          Table name
          <input
            value={table.name}
            onChange={(e) => set({ name: e.target.value })}
            spellCheck={false}
            className={`${field} max-w-xs font-mono text-zinc-900 dark:text-zinc-100`}
          />
        </label>

        <div className="mt-4 text-xs font-medium text-zinc-500">Columns</div>
        <div className="mt-2 overflow-x-auto">
          <table className="w-full text-sm">
            <thead className="text-left text-xs text-zinc-400">
              <tr>
                <th className="py-1 font-medium">Name</th>
                <th className="py-1 font-medium">Type</th>
                <th className="py-1 font-medium">Default</th>
                <th className="py-1 font-medium">Null?</th>
                <th className="py-1 font-medium" title="What identifies a row">Key?</th>
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
                    <select
                      value={column.type}
                      onChange={(e) => setColumn(index, { type: e.target.value as ColumnType })}
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
                      className="accent-lapis-600"
                    />
                  </td>
                  <td className="py-1 pr-2">
                    <input
                      type="checkbox"
                      checked={column.key}
                      onChange={(e) => setColumn(index, { key: e.target.checked })}
                      className="accent-lapis-600"
                    />
                  </td>
                  <td className="py-1">
                    <button
                      type="button"
                      onClick={() => set({ columns: table.columns.filter((_, i) => i !== index) })}
                      className="text-xs text-zinc-400 hover:text-red-600"
                    >
                      Remove
                    </button>
                  </td>
                </tr>
              ))}
            </tbody>
          </table>
        </div>
        <Button
          className="mt-2"
          onClick={() =>
            set({
              columns: [
                ...table.columns,
                { name: "", type: "u64", default: "0", nullable: false, key: false },
              ],
            })
          }
        >
          Add column
        </Button>
      </Card>

      {table.rules.map((rule, index) => (
        <RuleCard
          key={index}
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
              { on: sources[0]?.name ?? "", deleted: false, when: "", keys: [], sets: [], removes: false },
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
  sources,
  existing,
  table,
  rule,
  onChange,
  onRemove,
}: {
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

  return (
    <Card className="p-4">
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
          <input
            value={rule.when}
            onChange={(e) => onChange({ when: e.target.value })}
            placeholder="amount > 0"
            spellCheck={false}
            className={`${field} font-mono`}
          />
        </label>
        <button type="button" onClick={onRemove} className="pb-2 text-xs text-zinc-400 hover:text-red-600">
          Remove rule
        </button>
      </div>

      <div className="mt-3 flex flex-wrap items-center gap-1.5 text-xs text-zinc-500">
        Readable here:
        {readable.map((f) => (
          <span key={f.name} className="rounded bg-zinc-100 px-1.5 py-0.5 font-mono dark:bg-zinc-800">
            {f.name}
            <span className="text-zinc-400"> {f.type}</span>
          </span>
        ))}
        <span className="rounded bg-zinc-100 px-1.5 py-0.5 font-mono dark:bg-zinc-800">tx.version</span>
        <span className="rounded bg-zinc-100 px-1.5 py-0.5 font-mono dark:bg-zinc-800">tx.timestamp</span>
        <span>and the row&apos;s own columns.</span>
      </div>

      {existing.length > 0 && (
        <div className="mt-1.5 flex flex-wrap items-center gap-1.5 text-xs text-zinc-500">
          And a row of another table, which is null when there isn&apos;t one:
          {existing.map((t) => (
            <span
              key={t.name}
              title={t.columns.map((c) => c.name).join(", ")}
              className="rounded bg-zinc-100 px-1.5 py-0.5 font-mono dark:bg-zinc-800"
            >
              {t.name}[{t.key.join(", ")}].column
            </span>
          ))}
        </div>
      )}

      {unnamed.length > 0 && (
        <p className="mt-2 text-xs text-amber-600">
          Say where {unnamed.map((c) => c.name).join(", ")} comes from: the record has no field of
          that name.
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
                    sets: rule.sets.map((s, i) => (i === index ? { ...s, column: e.target.value } : s)),
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
              <input
                value={assignment.expression}
                onChange={(e) =>
                  onChange({
                    sets: rule.sets.map((s, i) =>
                      i === index ? { ...s, expression: e.target.value } : s,
                    ),
                  })
                }
                placeholder="count + 1"
                spellCheck={false}
                className={`${field} flex-1 font-mono`}
              />
              <button
                type="button"
                onClick={() => onChange({ sets: rule.sets.filter((_, i) => i !== index) })}
                className="text-xs text-zinc-400 hover:text-red-600"
              >
                Remove
              </button>
            </div>
          ))}
          <Button
            className="mt-2"
            onClick={() => onChange({ sets: [...rule.sets, { column: "", expression: "" }] })}
          >
            Add a column to set
          </Button>
        </div>
      )}

      {(rule.keys.length > 0 || unnamed.length > 0) && (
        <div className="mt-3">
          <div className="text-xs font-medium text-zinc-500">Key columns from the record</div>
          {rule.keys.map((assignment, index) => (
            <div key={index} className="mt-1.5 flex items-center gap-2">
              <select
                value={assignment.column}
                onChange={(e) =>
                  onChange({
                    keys: rule.keys.map((k, i) => (i === index ? { ...k, column: e.target.value } : k)),
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
              <input
                value={assignment.expression}
                onChange={(e) =>
                  onChange({
                    keys: rule.keys.map((k, i) =>
                      i === index ? { ...k, expression: e.target.value } : k,
                    ),
                  })
                }
                placeholder="key"
                spellCheck={false}
                className={`${field} flex-1 font-mono`}
              />
              <button
                type="button"
                onClick={() => onChange({ keys: rule.keys.filter((_, i) => i !== index) })}
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
                keys: [...rule.keys, { column: unnamed[0]?.name ?? "", expression: "" }],
              })
            }
          >
            Map a key column
          </Button>
        </div>
      )}
    </Card>
  );
}
