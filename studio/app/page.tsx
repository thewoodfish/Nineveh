"use client";

import Link from "next/link";

import { Card, Notice, Offline, PageHeader, PhaseDot, Stat } from "@/components/ui";
import type { Status } from "@/lib/api";
import { behind, formatDuration, formatInteger, progress, shortHex } from "@/lib/format";
import { useStatus, useTables } from "@/lib/hooks";

export default function Overview() {
  const { data: status, error } = useStatus();
  const { tables } = useTables();

  if (error && !status) return <Offline error={error} />;
  if (!status) return null;

  const pipeline = status.pipeline;
  const phase = pipeline?.phase ?? "serving";
  const cursor = pipeline?.cursor ?? status.build?.cursor ?? null;

  return (
    <div>
      <PageHeader title="Overview">
        <PhaseDot phase={error ? "offline" : phase} label />
      </PageHeader>

      <div className="mx-auto flex max-w-5xl flex-col gap-6 px-8 py-6">
        {error && <Notice tone="warning" title="Lost the API">{error}. Showing the last known state.</Notice>}
        {pipeline?.phase === "halted" && (
          <Notice tone="error" title="The pipeline halted">
            <span className="font-mono text-xs">{pipeline.last_error}</span>
          </Notice>
        )}
        {pipeline?.phase === "retrying" && (
          <Notice tone="warning" title={`Retrying (attempt ${pipeline.retries})`}>
            <span className="font-mono text-xs">{pipeline.last_error}</span>
          </Notice>
        )}
        {status.rebuild && <Rebuild status={status} />}

        <Backfill status={status} />

        <div className="grid grid-cols-2 gap-3 lg:grid-cols-4">
          <Stat label="Cursor" value={formatInteger(cursor)} hint="last committed version" />
          <Stat
            label="Behind the chain"
            value={formatInteger(behind(cursor, pipeline?.chain_version ?? null))}
            hint={pipeline?.chain_version ? `chain at ${formatInteger(pipeline.chain_version)}` : "no pipeline here"}
          />
          <Stat label="Lag" value={formatDuration(pipeline?.lag_secs)} hint="since the last block committed" />
          <Stat
            label="Throughput"
            value={pipeline?.versions_per_sec != null ? formatInteger(pipeline.versions_per_sec) : "—"}
            hint="versions / second"
          />
        </div>

        <Card>
          <div className="flex items-center justify-between border-b border-zinc-200 px-4 py-3 dark:border-zinc-800">
            <h2 className="text-sm font-medium">State tables</h2>
            <span className="text-xs text-zinc-400">{tables?.length ?? 0} tables</span>
          </div>
          <ul className="divide-y divide-zinc-100 dark:divide-zinc-800">
            {tables?.map((table) => (
              <li key={table.name}>
                <Link
                  href={`/tables?name=${encodeURIComponent(table.name)}`}
                  className="flex items-center gap-4 px-4 py-2.5 text-sm hover:bg-zinc-50 dark:hover:bg-zinc-800/40"
                >
                  <span className="w-48 truncate font-mono">{table.name}</span>
                  <span className="w-16 text-xs text-zinc-400">{table.kind}</span>
                  <span className="truncate text-xs text-zinc-500">
                    key {table.key.join(", ")} · {table.columns.length} columns
                  </span>
                </Link>
              </li>
            ))}
          </ul>
        </Card>

        {status.build && (
          <Card className="grid grid-cols-1 gap-x-8 gap-y-2 px-4 py-3 text-xs text-zinc-500 sm:grid-cols-3">
            <div>
              Build <span className="font-mono text-zinc-700 dark:text-zinc-300">{shortHex(`0x${status.build.fingerprint}`, 8, 6)}</span>
            </div>
            <div>Created {new Date(status.build.created_at).toLocaleString()}</div>
            <div>Last commit {new Date(status.build.updated_at).toLocaleString()}</div>
          </Card>
        )}
      </div>
    </div>
  );
}

function Backfill({ status }: { status: Status }) {
  const pipeline = status.pipeline;
  if (!pipeline || pipeline.schema !== status.schema) return null;
  const done = progress(pipeline.start_version, pipeline.cursor, pipeline.chain_version);
  if (done === null) return null;
  const caughtUp = done >= 0.9999;
  return (
    <Card className="px-4 py-4">
      <div className="flex items-baseline justify-between">
        <div className="text-sm font-medium">{caughtUp ? "Following the chain" : "Backfilling"}</div>
        <div className="font-mono text-sm tabular-nums">{(done * 100).toFixed(done < 0.1 ? 2 : 1)}%</div>
      </div>
      <div className="mt-3 h-1.5 overflow-hidden rounded-full bg-zinc-100 dark:bg-zinc-800">
        <div
          className="h-full rounded-full bg-lapis-500 transition-[width] duration-700 ease-out"
          style={{ width: `${Math.max(done * 100, 0.5)}%` }}
        />
      </div>
      <div className="mt-2 flex justify-between font-mono text-xs text-zinc-400">
        <span>{formatInteger(pipeline.start_version)}</span>
        <span>{formatInteger(pipeline.chain_version)}</span>
      </div>
    </Card>
  );
}

function Rebuild({ status }: { status: Status }) {
  const rebuild = status.rebuild;
  const pipeline = status.pipeline;
  if (!rebuild) return null;
  const done =
    pipeline && pipeline.schema === rebuild.schema
      ? progress(pipeline.start_version, rebuild.cursor, pipeline.chain_version)
      : null;
  return (
    <Notice tone="neutral" title="Rebuilding under a new config">
      The current tables stay served until the rebuild in{" "}
      <span className="font-mono">{rebuild.schema}</span> catches up and swaps in
      {done !== null && <> — {(done * 100).toFixed(1)}% done</>}.
    </Notice>
  );
}
