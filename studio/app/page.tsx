"use client";

import Link from "next/link";
import { useRouter } from "next/navigation";
import { useState, type ReactNode } from "react";

import { ApiKeys } from "@/components/api-keys";
import { Webhooks } from "@/components/webhooks";
import { ConfigPanel } from "@/components/config-panel";
import {
  Button,
  Card,
  filledButton,
  Icon,
  Notice,
  Offline,
  PageHeader,
  PhaseDot,
} from "@/components/ui";
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
        <p className="mt-3 text-sm text-on-surface-variant">
          Paste an Aptos contract address, pick what to follow, and Nineveh builds live tables you
          can query and subscribe to. No config to write.
        </p>
        <Link href="/new" className={`mt-8 ${filledButton}`}>
          New project
        </Link>
      </div>
    );
  }
  return (
    <div>
      <PageHeader title="Projects">
        <Link href="/new" className={filledButton}>
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
      <Card className="h-full overflow-hidden transition-colors group-hover:border-outline">
        <div className="px-4 py-3.5">
          <div className="flex items-center justify-between gap-2">
            <span className="truncate font-medium text-on-surface">{project.name}</span>
            <PhaseDot phase={project.state} label />
          </div>
          <div className="mt-2 flex items-center gap-2 text-xs">
            <span className="rounded bg-surface-container-high px-1.5 py-0.5 font-mono text-on-surface-variant">
              {project.network}
            </span>
            <span className="truncate text-on-surface-variant tnum">
              cursor {formatInteger(pipeline?.cursor ?? null)}
            </span>
          </div>
          {done !== null && (
            <>
              <div className="mt-3.5 h-1 overflow-hidden rounded-full bg-surface-container-highest">
                <div
                  className={`h-full rounded-full transition-[width] duration-700 ease-out ${
                    done >= 0.9999 ? "bg-tertiary" : "bg-primary"
                  }`}
                  style={{ width: `${Math.max(done * 100, 0.5)}%` }}
                />
              </div>
              <div className="mt-1.5 text-[11px] text-on-surface-variant tnum">
                {done >= 0.9999 ? "following the chain" : `${(done * 100).toFixed(1)}% backfilled`}
              </div>
            </>
          )}
        </div>
        {/* The pipeline records the last thing that went wrong even while it recovers, so
            this is history rather than an alarm: readable, and not dressed as a failure
            when the dot beside the name says the project is running. */}
        {project.error && (
          <div
            className="border-t border-outline-variant bg-surface-container px-4 py-2.5"
            title={project.error}
          >
            <div className="text-[10px] font-medium tracking-[0.08em] text-on-surface-variant uppercase">
              Last error
            </div>
            <p className="mt-1 line-clamp-2 font-mono text-[11px] leading-relaxed text-on-warning-container">
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
      {/* The project and its network are named in the app bar, so the page says which
          page it is. */}
      <PageHeader
        title="Overview"
        hint={
          tables ? `${tables.length} state ${tables.length === 1 ? "table" : "tables"}` : undefined
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

        <Health status={status} />

        <Card>
          <div className="flex items-center justify-between border-b border-outline-variant px-4 py-3">
            <h2 className="text-sm font-semibold text-on-surface">State tables</h2>
            <div className="flex items-center gap-3">
              <span className="text-xs text-on-surface-variant">{tables?.length ?? 0} tables</span>
              {mode === "control" && (
                <Link
                  href={href("/state")}
                  className="text-xs font-medium text-primary hover:underline"
                >
                  + New state table
                </Link>
              )}
            </div>
          </div>
          <ul className="divide-y divide-outline-variant">
            {tables?.map((table) => (
              <li key={table.name}>
                <Link
                  href={href("/tables", { name: table.name })}
                  className="group flex items-center gap-4 px-4 py-2.5 text-sm transition-colors hover:bg-on-surface/[0.06]"
                >
                  <span className="w-52 truncate font-mono font-medium group-hover:text-primary">
                    {table.name}
                  </span>
                  <Kind kind={table.kind} />
                  <span className="truncate text-xs text-on-surface-variant">
                    key {table.key.join(", ")} · {table.columns.length} columns
                  </span>
                </Link>
              </li>
            ))}
          </ul>
        </Card>

        {current && (
          <Card className="px-5 py-4 text-sm">
            <div className="text-[11px] font-medium tracking-[0.08em] text-on-surface-variant uppercase">
              Your API
            </div>
            <div className="mt-2 rounded-sm bg-surface-container-high px-3 py-2 font-mono text-sm break-all">
              {`${API_URL}${current.api}/v1/tables`}
            </div>
            <p className="mt-1 text-xs text-on-surface-variant">
              REST over every table, and a live change feed at{" "}
              <span className="font-mono">/v1/changes</span>.
              {hosted && <> Send one of this project&apos;s API keys with each request.</>} Try it
              in the{" "}
              <Link href={href("/playground")} className="text-primary hover:underline">
                API playground
              </Link>
              .
            </p>
          </Card>
        )}

        {current && hosted && <ApiKeys project={current.name} api={current.api} />}
        {current && <Webhooks project={current.name} />}

        {status.build && (
          <Card className="grid grid-cols-1 gap-x-8 gap-y-2 px-4 py-3 text-xs text-on-surface-variant sm:grid-cols-3">
            <div>
              Build{" "}
              <span className="font-mono text-on-surface">
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
      {error && <span className="text-xs text-error">{error}</span>}
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
    reduce: "bg-secondary-container text-primary",
    mirror: "bg-tertiary-container text-on-tertiary-container",
    log: "bg-surface-container-high text-on-surface-variant",
  };
  return (
    <span
      className={`w-16 shrink-0 rounded px-1.5 py-0.5 text-center text-[11px] font-medium ${tones[kind] ?? ""}`}
    >
      {kind}
    </span>
  );
}

/**
 * The one question this page exists to answer — *is my backend keeping up?* — as one
 * number, with the sentence that makes it mean something. Four equally loud stats
 * answered it four times and therefore not at all; those are the supporting row now.
 */
function Health({ status }: { status: Status }) {
  const pipeline = status.pipeline;
  const cursor = pipeline?.cursor ?? status.build?.cursor ?? null;
  const chain = pipeline?.chain_version ?? null;
  const done =
    pipeline && pipeline.schema === status.schema
      ? progress(pipeline.start_version, pipeline.cursor, pipeline.chain_version)
      : null;
  const backfilling = done !== null && done < 0.9999;
  const behindBy = behind(cursor, chain);

  return (
    <Card className="overflow-hidden">
      <div className="px-6 pt-6 pb-5">
        <div className="flex flex-wrap items-start justify-between gap-3">
          <div className="min-w-0">
            <div className="text-xs font-medium tracking-[0.08em] text-on-surface-variant uppercase">
              {backfilling ? "Reading history" : "Following the chain"}
            </div>
            <div className="mt-2 flex items-baseline gap-1 font-mono text-[52px] leading-none text-on-surface tnum">
              {backfilling ? (
                <>
                  {(done * 100).toFixed(done < 0.1 ? 2 : 1)}
                  <span className="text-2xl text-on-surface-variant">%</span>
                </>
              ) : (
                formatInteger(behindBy ?? "0")
              )}
            </div>
            <p className="mt-3 text-sm text-on-surface-variant">
              {backfilling ? (
                <>
                  folded, of the history between{" "}
                  <span className="font-mono text-on-surface">
                    {formatInteger(pipeline?.start_version ?? null)}
                  </span>{" "}
                  and <span className="font-mono text-on-surface">{formatInteger(chain)}</span> —
                  the tables serve what has landed so far
                </>
              ) : (
                <>
                  versions behind the chain · committed through{" "}
                  <span className="font-mono text-on-surface">{formatInteger(cursor)}</span>
                  {pipeline?.lag_secs != null && (
                    <> · last block {formatDuration(pipeline.lag_secs)} ago</>
                  )}
                </>
              )}
            </p>
          </div>
          <span
            className={`inline-flex shrink-0 items-center gap-1.5 rounded-sm px-2 py-1 text-xs font-medium ${
              backfilling
                ? "bg-secondary-container text-on-secondary-container"
                : "bg-tertiary-container text-on-tertiary-container"
            }`}
          >
            <Icon name={backfilling ? "history" : "check_circle"} className="text-[14px]" />
            {backfilling ? "Backfilling" : "Caught up"}
          </span>
        </div>

        {done !== null && (
          <div className="mt-6 h-1.5 overflow-hidden rounded-full bg-surface-container-highest">
            <div
              className={`h-full rounded-full transition-[width] duration-700 ease-out ${
                backfilling ? "bg-primary" : "bg-tertiary"
              }`}
              style={{ width: `${Math.max(done * 100, 0.5)}%` }}
            />
          </div>
        )}
      </div>

      {/* The supporting numbers, demoted to a row: still there, no longer shouting. */}
      <dl className="grid grid-cols-2 divide-outline-variant border-t border-outline-variant sm:grid-cols-4 sm:divide-x">
        <Supporting label="Cursor" value={formatInteger(cursor)} />
        <Supporting label="Chain head" value={formatInteger(chain)} />
        <Supporting label="Lag" value={formatDuration(pipeline?.lag_secs)} />
        <Supporting
          label="Throughput"
          value={
            pipeline?.versions_per_sec != null
              ? `${formatInteger(pipeline.versions_per_sec)}/s`
              : "—"
          }
        />
      </dl>
    </Card>
  );
}

function Supporting({ label, value }: { label: string; value: ReactNode }) {
  return (
    <div className="px-6 py-3.5">
      <dt className="text-xs text-on-surface-variant">{label}</dt>
      <dd className="mt-1 truncate font-mono text-sm text-on-surface tnum">{value}</dd>
    </div>
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
