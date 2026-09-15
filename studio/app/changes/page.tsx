"use client";

import { memo, useCallback, useState } from "react";

import { Cell, Live, OpBadge, PageHeader } from "@/components/ui";
import type { Change, Table } from "@/lib/api";
import { formatInteger } from "@/lib/format";
import { useFeed, useTables } from "@/lib/hooks";

const KEEP = 300;

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
      const newest = batch.slice(-KEEP).reverse().map((change) => ({ ...change, at }));
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
        <button
          type="button"
          onClick={() => setPaused(!paused)}
          className="rounded-md border border-zinc-200 px-2.5 py-1 text-xs font-medium hover:bg-zinc-50 dark:border-zinc-700 dark:hover:bg-zinc-800"
        >
          {paused ? "Resume" : "Pause"}
        </button>
        <Live connected={connected && !paused} />
      </PageHeader>

      <div className="flex flex-wrap gap-1.5 border-b border-zinc-200 px-8 py-2.5 dark:border-zinc-800">
        <Chip active={only === null} onClick={() => setOnly(null)}>
          All tables
        </Chip>
        {tables?.map((t) => (
          <Chip key={t.name} active={only === t.name} onClick={() => setOnly(t.name)}>
            <span className="font-mono">{t.name}</span>
          </Chip>
        ))}
      </div>

      <div className="min-h-0 flex-1 overflow-y-auto">
        {shown.length === 0 ? (
          <div className="px-8 py-20 text-center text-sm text-zinc-500">
            Waiting for changes. Each commit that changes a row shows up here the moment it lands.
          </div>
        ) : (
          <ul className="divide-y divide-zinc-100 dark:divide-zinc-800/70">
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
const ChangeRow = memo(function ChangeRow({ change, table }: { change: Change; table: Table | undefined }) {
  const [open, setOpen] = useState(false);
  const types = new Map(table?.columns.map((c) => [c.name, c.type]));
  return (
    <li className="enter px-8 py-2 text-sm">
      <button type="button" onClick={() => setOpen(!open)} className="flex w-full items-center gap-3 text-left">
        <span className="w-28 shrink-0 font-mono text-xs text-zinc-400 tabular-nums">
          {formatInteger(change.version)}
        </span>
        <OpBadge op={change.op} />
        <span className="w-40 shrink-0 truncate font-mono text-[13px]">{change.table}</span>
        <span className="flex min-w-0 flex-1 gap-3 truncate text-xs text-zinc-500">
          {Object.entries(change.key).map(([column, value]) => (
            <span key={column} className="truncate">
              {column} <Cell plain type={types.get(column) ?? "string"} value={value} />
            </span>
          ))}
        </span>
      </button>
      {open && (
        <pre className="mt-2 ml-31 overflow-x-auto rounded-lg bg-zinc-50 p-3 font-mono text-xs dark:bg-zinc-900">
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
        active
          ? "bg-zinc-900 text-white dark:bg-white dark:text-zinc-900"
          : "bg-zinc-100 text-zinc-600 hover:bg-zinc-200 dark:bg-zinc-800 dark:text-zinc-400 dark:hover:bg-zinc-700"
      }`}
    >
      {children}
    </button>
  );
}
