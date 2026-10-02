"use client";

import { Fragment, useState } from "react";

import type { SourceInfo, Table } from "@/lib/api";

import { Card, Icon } from "./ui";

/**
 * What this project has, to write the file against.
 *
 * Reference, not a tool: a reducers file folds as many sources as it likes and reads
 * other tables by key (ADR 0019), and the question halfway through a line is "what is
 * that field called". It answers that and stays out of the way — nothing here types for
 * you, because a panel that edits the file from the side is a second way to write it.
 */
export function Palette({ sources, tables }: { sources: SourceInfo[] | null; tables: Table[] }) {
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
