"use client";

import Link from "next/link";
import { useRouter } from "next/navigation";
import { useState } from "react";

import { ApiKeys } from "@/components/api-keys";
import { Webhooks } from "@/components/webhooks";
import { ConfigPanel } from "@/components/config-panel";
import { Button, Card, Notice, Offline, PageHeader, PhaseDot, Stat } from "@/components/ui";
import { API_URL, type ProjectSummary, type Status, control } from "@/lib/api";
import { behind, formatDuration, formatInteger, progress, shortHex } from "@/lib/format";
import { useStatus, useTables } from "@/lib/hooks";
import { useHref, useProject } from "@/lib/project";

export default function Home() {
  const { mode, name, error } = useProject();
  if (mode === "loading") return null;
  if (mode === "offline") return <Offline error={error ?? "Nineveh isn't answering"} />;
  if (mode === "control" && !name) return <Projects />;
  return <Overview />;
}

/** Every project under the control plane. */
function Projects() {
  const { projects } = useProject();
  if (!projects) return null;
  if (projects.length === 0) {
    return (
      <div className="mx-auto mt-24 max-w-lg px-6 text-center">
        <h1 className="text-2xl font-semibold tracking-tight">Create your first backend</h1>
        <p className="mt-3 text-sm text-dim">
          Paste an Aptos contract address, pick what to follow, and Nineveh builds live tables you
          can query and subscribe to. No config to write.
        </p>
        <Link
          href="/new"
          className="mt-8 inline-flex rounded-lg bg-blue-600 px-4 py-2 text-sm font-medium text-white shadow-card transition-colors hover:bg-blue-500"
        >
          New project
        </Link>
      </div>
    );
  }
  return (
    <div>
      <PageHeader title="Projects">
        <Link
          href="/new"
          className="rounded-lg bg-blue-600 px-3 py-1.5 text-sm font-medium text-white shadow-card transition-colors hover:bg-blue-500"
        >
          New project
        </Link>
      </PageHeader>
      <div className="grid max-w-6xl gap-3 px-8 py-6 sm:grid-cols-2 xl:grid-cols-3">
        {projects.map((p) => (
          <ProjectCard key={p.name} project={p} />
        ))}
      </div>
    </div>
  );
}

function ProjectCard({ project }: { project: ProjectSummary }) {
  const pipeline = project.pipeline;
  const done = pipeline
    ? progress(pipeline.start_version, pipeline.cursor, pipeline.chain_version)
    : null;
  return (
    <Link href={`/?project=${encodeURIComponent(project.name)}`} className="group">
      <Card className="h-full overflow-hidden transition-colors group-hover:border-edge">
        <div className="px-4 py-3.5">
          <div className="flex items-center justify-between gap-2">
            <span className="truncate font-medium text-white">{project.name}</span>
            <PhaseDot phase={project.state} label />
          </div>
          <div className="mt-2 flex items-center gap-2 text-xs">
            <span className="rounded bg-white/[0.07] px-1.5 py-0.5 font-mono text-faint">
              {project.network}
            </span>
            <span className="truncate text-dim tnum">
              cursor {formatInteger(pipeline?.cursor ?? null)}
            </span>
          </div>
          {done !== null && (
            <>
              <div className="mt-3.5 h-1 overflow-hidden rounded-full bg-black/40">
                <div
                  className={`h-full rounded-full transition-[width] duration-700 ease-out ${
                    done >= 0.9999 ? "bg-emerald-400" : "bg-blue-400"
                  }`}
                  style={{ width: `${Math.max(done * 100, 0.5)}%` }}
                />
              </div>
              <div className="mt-1.5 text-[11px] text-faint tnum">
                {done >= 0.9999 ? "following the chain" : `${(done * 100).toFixed(1)}% backfilled`}
              </div>
            </>
          )}
        </div>
        {/* The pipeline records the last thing that went wrong even while it recovers, so
            this is history rather than an alarm: readable, and not dressed as a failure
            when the dot beside the name says the project is running. */}
        {project.error && (
          <div className="border-t border-line bg-black/20 px-4 py-2.5" title={project.error}>
            <div className="text-[10px] font-medium tracking-[0.08em] text-faint uppercase">
              Last error
            </div>
            <p className="mt-1 line-clamp-2 font-mono text-[11px] leading-relaxed text-amber-300/90">
              {project.error}
            </p>
          </div>
        )}
      </Card>
    </Link>
  );
}

function Overview() {
  const { data: status, error } = useStatus();
  const { tables } = useTables();
  const { current, mode, hosted } = useProject();
  const href = useHref();

  if (error && !status) return <Offline error={error} />;
  if (!status) return null;

  const pipeline = status.pipeline;
  const phase = current?.state ?? pipeline?.phase ?? "serving";
  const cursor = pipeline?.cursor ?? status.build?.cursor ?? null;

  return (
    <div>
      <PageHeader
        title={mode === "control" ? status.project : "Overview"}
        hint={
          <span className="font-mono text-xs">
            {status.network}
            {tables && ` · ${tables.length} ${tables.length === 1 ? "table" : "tables"}`}
          </span>
        }
      >
        <PhaseDot phase={error ? "offline" : phase} label />
        {current && <ProjectActions project={current} />}
      </PageHeader>

      <div className="flex max-w-6xl flex-col gap-6 px-8 py-6">
        {error && (
          <Notice tone="warning" title="Lost the API">
            {error}. Showing the last known state.
          </Notice>
        )}
        {(phase === "halted" || phase === "failed") && (
          <Notice
            tone="error"
            title={phase === "halted" ? "The pipeline halted" : "The project failed"}
          >
            <span className="font-mono text-xs whitespace-pre-wrap">
              {current?.error ?? pipeline?.last_error}
            </span>
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
            hint={
              pipeline?.chain_version
                ? `chain at ${formatInteger(pipeline.chain_version)}`
                : "no pipeline here"
            }
          />
          <Stat
            label="Lag"
            value={formatDuration(pipeline?.lag_secs)}
            hint="since the last block committed"
          />
          <Stat
            label="Throughput"
            value={
              pipeline?.versions_per_sec != null ? formatInteger(pipeline.versions_per_sec) : "—"
            }
            hint="versions / second"
          />
        </div>

        <Card>
          <div className="flex items-center justify-between border-b border-line px-4 py-3">
            <h2 className="text-sm font-semibold text-white">State tables</h2>
            <div className="flex items-center gap-3">
              <span className="text-xs text-faint">{tables?.length ?? 0} tables</span>
              {mode === "control" && (
                <Link
                  href={href("/state")}
                  className="text-xs font-medium text-blue-300 hover:underline"
                >
                  + New state table
                </Link>
              )}
            </div>
          </div>
          <ul className="divide-y divide-line">
            {tables?.map((table) => (
              <li key={table.name}>
                <Link
                  href={href("/tables", { name: table.name })}
                  className="group flex items-center gap-4 px-4 py-2.5 text-sm transition-colors hover:bg-white/[0.04]"
                >
                  <span className="w-52 truncate font-mono font-medium group-hover:text-blue-300">
                    {table.name}
                  </span>
                  <Kind kind={table.kind} />
                  <span className="truncate text-xs text-dim">
                    key {table.key.join(", ")} · {table.columns.length} columns
                  </span>
                </Link>
              </li>
            ))}
          </ul>
        </Card>

        {current && (
          <Card className="px-5 py-4 text-sm">
            <div className="text-[11px] font-medium tracking-[0.08em] text-faint uppercase">
              Your API
            </div>
            <div className="mt-2 rounded-lg bg-well px-3 py-2 font-mono text-sm break-all">
              {`${API_URL}${current.api}/v1/tables`}
            </div>
            <p className="mt-1 text-xs text-faint">
              REST over every table, and a live change feed at{" "}
              <span className="font-mono">/v1/changes</span>.
              {hosted && <> Send one of this project&apos;s API keys with each request.</>} Try it
              in the{" "}
              <Link href={href("/playground")} className="text-blue-300 hover:underline">
                API playground
              </Link>
              .
            </p>
          </Card>
        )}

        {current && hosted && <ApiKeys project={current.name} api={current.api} />}
        {current && <Webhooks project={current.name} />}

        {status.build && (
          <Card className="grid grid-cols-1 gap-x-8 gap-y-2 px-4 py-3 text-xs text-dim sm:grid-cols-3">
            <div>
              Build{" "}
              <span className="font-mono text-white/75">
                {shortHex(`0x${status.build.fingerprint}`, 8, 6)}
              </span>
            </div>
            <div>Created {new Date(status.build.created_at).toLocaleString()}</div>
            <div>Last commit {new Date(status.build.updated_at).toLocaleString()}</div>
          </Card>
        )}
      </div>
    </div>
  );
}

/** Start or stop the project, see and edit its config, or delete it. */
function ProjectActions({ project }: { project: ProjectSummary }) {
  const { refresh } = useProject();
  const router = useRouter();
  const [busy, setBusy] = useState(false);
  const [confirming, setConfirming] = useState(false);
  const [showConfig, setShowConfig] = useState(false);
  const [error, setError] = useState<string | null>(null);

  const act = async (action: () => Promise<unknown>) => {
    setBusy(true);
    setError(null);
    try {
      await action();
      await refresh();
    } catch (e) {
      setError(e instanceof Error ? e.message : String(e));
    } finally {
      setBusy(false);
    }
  };

  const remove = async () => {
    if (!confirming) {
      setConfirming(true);
      setTimeout(() => setConfirming(false), 5000);
      return;
    }
    await act(() => control.remove(project.name));
    router.push("/");
  };

  return (
    <>
      {error && <span className="text-xs text-red-300">{error}</span>}
      <Button onClick={() => setShowConfig(true)}>Config</Button>
      {project.running ? (
        <Button disabled={busy} onClick={() => void act(() => control.stop(project.name))}>
          Stop
        </Button>
      ) : (
        <Button disabled={busy} onClick={() => void act(() => control.start(project.name))}>
          Start
        </Button>
      )}
      <Button tone="danger" disabled={busy} onClick={() => void remove()}>
        {confirming ? "Delete its data too?" : "Delete"}
      </Button>
      {showConfig && <ConfigPanel name={project.name} onClose={() => setShowConfig(false)} />}
    </>
  );
}

/** What builds a table: the three kinds read differently, so they look different. */
function Kind({ kind }: { kind: string }) {
  const tones: Record<string, string> = {
    reduce: "bg-blue-500/15 text-blue-300",
    mirror: "bg-emerald-500/15 text-emerald-300",
    log: "bg-white/[0.07] text-dim",
  };
  return (
    <span
      className={`w-16 shrink-0 rounded px-1.5 py-0.5 text-center text-[11px] font-medium ${tones[kind] ?? ""}`}
    >
      {kind}
    </span>
  );
}

function Backfill({ status }: { status: Status }) {
  const pipeline = status.pipeline;
  if (!pipeline || pipeline.schema !== status.schema) return null;
  const done = progress(pipeline.start_version, pipeline.cursor, pipeline.chain_version);
  if (done === null) return null;
  const caughtUp = done >= 0.9999;
  return (
    <Card className="px-5 py-5">
      <div className="flex flex-wrap items-baseline justify-between gap-2">
        <div className="flex items-baseline gap-2">
          <span className="text-sm font-semibold">
            {caughtUp ? "Following the chain" : "Backfilling"}
          </span>
          <span className="text-xs text-dim">
            {caughtUp
              ? "every new transaction, as it commits"
              : "reading history before it can follow along"}
          </span>
        </div>
        <div className="flex items-baseline gap-0.5 font-mono text-2xl leading-none font-semibold text-white tnum">
          {(done * 100).toFixed(done < 0.1 ? 2 : 1)}
          <span className="text-sm font-normal text-faint">%</span>
        </div>
      </div>
      <div className="mt-4 h-2 overflow-hidden rounded-full bg-black/40 inset-ring inset-ring-white/5">
        <div
          className={`h-full rounded-full transition-[width] duration-700 ease-out ${
            caughtUp
              ? "bg-emerald-400 shadow-[0_0_12px_oklch(0.77_0.15_162_/_0.6)]"
              : "bg-blue-400 shadow-[0_0_12px_oklch(0.716_0.152_259_/_0.6)]"
          }`}
          style={{ width: `${Math.max(done * 100, 0.5)}%` }}
        />
      </div>
      <div className="mt-2 flex justify-between font-mono text-xs text-faint tnum">
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
