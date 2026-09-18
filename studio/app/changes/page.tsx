"use client";

import { memo, useCallback, useState } from "react";

import { Button, Cell, field, Live, OpBadge, PageHeader, Select } from "@/components/ui";
import type { Change, Table } from "@/lib/api";
import { formatInteger } from "@/lib/format";
import { useFeed, useTables } from "@/lib/hooks";

const KEEP = 300;
/** Up to this many tables are chips; more than that is a picker. */
const CHIPS = 8;

/** Every committed change, across all tables, as it happens. */
export default function ChangesPage() {
  const { tables } = useTables();
  const [changes, setChanges] = useState<(Change & { at: number })[]>([]);
  const [paused, setPaused] = useState(false);
  const [only, setOnly] = useState<string | null>(null);

  const onChanges = useCallback(
    (batch: Change[]) => {
      if (paused) return;
      const at = Date.now();
      const newest = batch
        .slice(-KEEP)
        .reverse()
        .map((change) => ({ ...change, at }));
      setChanges((current) => [...newest, ...current].slice(0, KEEP));
    },
    [paused],
  );
  const connected = useFeed({ onChanges, onReset: () => setChanges([]) });

  const shown = only ? changes.filter((c) => c.table === only) : changes;
  const byName = new Map(tables?.map((t) => [t.name, t]));

  return (
    <div className="flex h-full flex-col">
      <PageHeader title="Change feed">
        <Button onClick={() => setPaused(!paused)}>{paused ? "Resume" : "Pause"}</Button>
        <Live connected={connected && !paused} />
      </PageHeader>

      <div className="flex flex-wrap items-center gap-1.5 border-b border-line px-8 py-2.5">
        {/* A handful of tables fit as chips; a project with a hundred needs a picker. */}
        {(tables?.length ?? 0) <= CHIPS ? (
          <>
            <Chip active={only === null} onClick={() => setOnly(null)}>
              All tables
            </Chip>
            {tables?.map((t) => (
              <Chip key={t.name} active={only === t.name} onClick={() => setOnly(t.name)}>
                <span className="font-mono">{t.name}</span>
              </Chip>
            ))}
          </>
        ) : (
          <>
            <span className="text-xs text-dim">Showing</span>
            <Select
              value={only ?? ""}
              onChange={(e) => setOnly(e.target.value || null)}
              className={`${field} py-1 font-mono text-xs`}
            >
              <option value="">all {tables?.length} tables</option>
              {tables?.map((t) => (
                <option key={t.name} value={t.name}>
                  {t.name}
                </option>
              ))}
            </Select>
            {only && (
              <button
                type="button"
                onClick={() => setOnly(null)}
                className="text-xs font-medium text-blue-300 hover:underline"
              >
                Clear
              </button>
            )}
          </>
        )}
        <span className="ml-auto text-xs text-faint tnum">
          {shown.length > 0 && `${shown.length}${shown.length === KEEP ? "+" : ""} shown`}
        </span>
      </div>

      <div className="min-h-0 flex-1 overflow-y-auto">
        {shown.length === 0 ? (
          <div className="mx-auto mt-24 max-w-sm px-8 text-center">
            <div className="mx-auto flex size-10 items-center justify-center rounded-full bg-well">
              <span className="size-2 animate-ping rounded-full bg-blue-400" />
            </div>
            <p className="mt-4 text-sm font-medium">
              {paused ? "Paused" : only ? `Watching ${only}` : "Watching for changes"}
            </p>
            <p className="mt-1 text-sm text-dim text-pretty">
              {paused
                ? "Nothing is being collected while this is paused."
                : "Every commit that changes a row shows up here the moment it lands."}
            </p>
          </div>
        ) : (
          <ul className="divide-y divide-line">
            {shown.map((change) => (
              <ChangeRow
                key={`${change.version}.${change.seq}`}
                change={change}
                table={byName.get(change.table)}
              />
            ))}
          </ul>
        )}
      </div>
    </div>
  );
}

// Memoized: a batch adds rows at the top, and the rest shouldn't render again.
const ChangeRow = memo(function ChangeRow({
  change,
  table,
}: {
  change: Change;
  table: Table | undefined;
}) {
  const [open, setOpen] = useState(false);
  const types = new Map(table?.columns.map((c) => [c.name, c.type]));
  return (
    <li className="enter px-8 py-2 text-sm">
      <button
        type="button"
        onClick={() => setOpen(!open)}
        className="flex w-full items-center gap-3 text-left"
      >
        <span className="w-28 shrink-0 font-mono text-xs text-faint tabular-nums">
          {formatInteger(change.version)}
        </span>
        <OpBadge op={change.op} />
        <span className="w-40 shrink-0 truncate font-mono text-[13px]">{change.table}</span>
        <span className="flex min-w-0 flex-1 gap-3 truncate text-xs text-dim">
          {Object.entries(change.key).map(([column, value]) => (
            <span key={column} className="truncate">
              {column} <Cell plain type={types.get(column) ?? "string"} value={value} />
            </span>
          ))}
        </span>
      </button>
      {open && (
        <pre className="mt-2 ml-31 overflow-x-auto rounded-lg bg-well p-3 font-mono text-xs">
          {JSON.stringify(change.row, null, 2)}
        </pre>
      )}
    </li>
  );
});

function Chip({
  active,
  onClick,
  children,
}: {
  active: boolean;
  onClick: () => void;
  children: React.ReactNode;
}) {
  return (
    <button
      type="button"
      onClick={onClick}
      className={`rounded-full px-2.5 py-0.5 text-xs transition-colors ${
        active ? "bg-blue-600 text-white" : "bg-well text-dim hover:bg-white/10"
      }`}
    >
      {children}
    </button>
  );
}
