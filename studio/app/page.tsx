"use client";

import Link from "next/link";
import { PageHeader } from "@/components/page-header";
import { useState, type ReactNode } from "react";

import {
  Card,
  filledButton,
  Ghost,
  Icon,
  Notice,
  Offline,
  PhaseDot,
  Reading,
} from "@/components/ui";
import {
  API_URL,
  GEOMI_BILLING,
  type ProjectSummary,
  type SourceInfo,
  type Status,
  type Usage,
  control,
  isCreditCap,
} from "@/lib/api";
import {
  behind,
  formatBytes,
  formatDuration,
  formatInteger,
  progress,
  shortHex,
} from "@/lib/format";
import { useSources, useStatus, useTables, useUsage } from "@/lib/hooks";
import { useHref, useProject } from "@/lib/project";

export default function Home() {
  const { mode, name, error } = useProject();
  // Not `null`. This is the first paint of the app and of every project switch, and an
  // empty document here is why a cold Studio reads as broken rather than as starting.
  if (mode === "loading") return <OverviewSkeleton />;
  if (mode === "offline")
    return <Offline error={error ?? "Nineveh isn't answering"} />;
  if (mode === "control" && !name) return <Projects />;
  return <Overview />;
}

/**
 * A project that has stopped.
 *
 * Nineveh halts at a version on a deterministic error rather than skipping it, so this
 * is a designed state and not an exception — but it used to render as the error's own
 * `Display` string in a monospace span, which for a `thiserror` chain is a paragraph of
 * Rust. The audience includes teams who do not write Rust, and the three things they
 * need were all already on screen somewhere else: where it stopped, what stopped it,
 * and that the fix is a config change rather than a support ticket.
 *
 * The raw chain stays, behind a disclosure. It is what you paste into an issue, and
 * hiding it outright would trade one group's clarity for another's.
 */
function Halted({
  phase,
  failure,
  cursor,
  href,
}: {
  phase: string;
  failure: string | null;
  cursor: string | null;
  href: (to: string) => string;
}) {
  // The first line of a `thiserror` chain is the outermost cause, which is the one
  // written for a person; the rest is the chain under it.
  const lines = (failure ?? "").split("\n").filter((l) => l.trim() !== "");
  const [headline, ...rest] = lines;
  const detail = rest.join("\n");

  return (
    <Notice
      tone="error"
      title={
        phase === "halted"
          ? cursor
            ? `Stopped at version ${formatInteger(Number(cursor))}`
            : "Stopped"
          : "The project failed"
      }
    >
      <p className="text-pretty">
        {headline ?? "No reason was reported."}
      </p>
      <p className="mt-2 text-on-surface-variant text-pretty">
        Nothing is lost. A project stops at the version it could not fold rather than
        skipping it, so the rows served so far are correct and it resumes from here once
        the cause is fixed.
      </p>
      <div className="mt-3 flex flex-wrap items-center gap-3">
        <Link href={href("/state")} className={filledButton}>
          Edit the rules
        </Link>
        <Link
          href={href("/settings")}
          className="text-sm text-primary hover:underline"
        >
          Project settings
        </Link>
      </div>
      {detail !== "" && (
        <details className="mt-4">
          <summary className="state cursor-pointer rounded-xs text-xs text-on-surface-variant select-none">
            Show technical detail
          </summary>
          <pre className="mt-2 overflow-x-auto rounded-sm bg-surface-container-high px-3 py-2 font-mono text-xs leading-relaxed whitespace-pre-wrap">
            {detail}
          </pre>
        </details>
      )}
    </Notice>
  );
}

/**
 * Overview before its status has arrived.
 *
 * Shaped like the Health card that lands here — one big figure, a sentence under it, a
 * progress rail, then the demoted stats — because that card is the page's anchor and
 * the layout must not jump when the real number appears.
 */
function OverviewSkeleton() {
  return (
    <Reading label="Reading this project">
      <div className="flex items-baseline gap-3 border-b border-outline-variant px-8 py-4">
        <Ghost className="h-5 w-28" />
      </div>
      <div className="flex max-w-6xl flex-col gap-6 px-8 py-6">
        <Card className="p-6">
          <Ghost className="h-3 w-24" />
          <Ghost className="mt-4 h-12 w-56" />
          <Ghost className="mt-3 h-4 w-80" />
          <Ghost className="mt-5 h-1.5 w-full rounded-full" />
          <div className="mt-5 flex gap-10 border-t border-outline-variant pt-4">
            {[0, 1, 2, 3].map((i) => (
              <div key={i} className="space-y-2">
                <Ghost className="h-2.5 w-16" />
                <Ghost className="h-4 w-20" />
              </div>
            ))}
          </div>
        </Card>
        <div className="grid gap-4 sm:grid-cols-2">
          {[0, 1].map((i) => (
            <Card key={i} className="space-y-3 p-5">
              <Ghost className="h-3 w-20" />
              <Ghost className="h-6 w-32" />
              <Ghost className="h-3 w-full" />
            </Card>
          ))}
        </div>
      </div>
    </Reading>
  );
}

/** The project list before it has arrived: the cards it is about to hold. */
function ProjectsSkeleton() {
  return (
    <Reading label="Reading your projects">
      <div className="flex items-baseline gap-3 border-b border-outline-variant px-8 py-4">
        <Ghost className="h-5 w-24" />
      </div>
      <div className="grid max-w-6xl gap-4 px-8 py-6 sm:grid-cols-2 lg:grid-cols-3">
        {[0, 1, 2].map((i) => (
          <Card key={i} className="space-y-3 p-5">
            <Ghost className="h-4 w-32" />
            <Ghost className="h-3 w-20" />
            <Ghost className="h-3 w-28" />
          </Card>
        ))}
      </div>
    </Reading>
  );
}

/** Every project under the control plane. */
function Projects() {
  const { projects } = useProject();
  if (!projects) return <ProjectsSkeleton />;
  if (projects.length === 0) {
    return (
      <div className="mx-auto mt-24 max-w-lg px-6 text-center">
        <Icon name="database" className="text-[40px] text-on-surface-variant" />
        <h1 className="mt-4 text-2xl leading-8 text-on-surface">
          Create your first backend
        </h1>
        <p className="mt-3 text-sm text-on-surface-variant">
          Paste an Aptos contract address, pick what to follow, and Nineveh
          builds live tables you can query and subscribe to. No config to write.
        </p>
        <Link href="/new" className={`mt-8 ${filledButton}`}>
          <Icon name="add" className="text-[18px]" />
          New project
        </Link>
      </div>
    );
  }
  const following = projects.filter((p) => p.running).length;
  return (
    <div>
      <PageHeader
        title="Projects"
        hint={`${projects.length} ${projects.length === 1 ? "backend" : "backends"}, ${following} following the chain`}
      >
        <Link href="/new" className={filledButton}>
          <Icon name="add" className="text-[18px]" />
          New project
        </Link>
      </PageHeader>
      <div className="grid max-w-6xl auto-rows-fr gap-4 px-6 py-6 sm:grid-cols-2 xl:grid-cols-3">
        {projects.map((p) => (
          <ProjectCard key={p.name} project={p} />
        ))}
      </div>
    </div>
  );
}

/**
 * The Transaction Stream closes a connection once it has run for its maximum duration
 * and expects the client to open another; the pipeline does, and carries on. It is by
 * far the commonest thing to find in `error`, and it is not a fault — so it doesn't get
 * to make every healthy project on this page look broken.
 */
function routine(error: string): boolean {
  return error.includes("Stream-duration-limit-reached");
}

function ProjectCard({ project }: { project: ProjectSummary }) {
  const pipeline = project.pipeline;
  const done = pipeline
    ? progress(pipeline.start_version, pipeline.cursor, pipeline.chain_version)
    : null;
  const backfilling = done !== null && done < 0.9999;
  const worrying =
    project.error && !(project.running && routine(project.error));

  return (
    <Link
      href={`/?project=${encodeURIComponent(project.name)}`}
      className="group"
    >
      <Card className="flex h-full flex-col p-5 transition-shadow group-hover:shadow-e2">
        <div className="flex items-start justify-between gap-3">
          <div className="min-w-0">
            <div className="truncate text-base font-medium text-on-surface">
              {project.name}
            </div>
            <div className="mt-1.5 flex items-center gap-2">
              <span className="rounded-xs bg-surface-container-high px-1.5 py-0.5 font-mono text-[11px] text-on-surface-variant">
                {project.network}
              </span>
              <span className="truncate text-xs text-on-surface-variant">
                since {new Date(project.created_at).toLocaleDateString()}
              </span>
            </div>
          </div>
          <span className="flex shrink-0 items-center gap-1.5">
            {worrying && (
              <Icon
                name="warning"
                className="text-[16px] text-on-warning-container"
              />
            )}
            <PhaseDot phase={project.state} label />
          </span>
        </div>

        {/* The reason, in the card. It used to live in a `title` on the glyph above —
            hover-only, so unreachable by keyboard or touch and silent to a screen
            reader, on the one card that needs attention. The full chain is on the
            project's own page; one line here is what decides whether to open it. */}
        {worrying && (
          <p className="mt-3 line-clamp-2 text-xs text-on-warning-container text-pretty">
            {(project.error ?? "").split("\n")[0]}
          </p>
        )}

        <div className="mt-5">
          <div className="truncate font-mono text-2xl leading-none text-on-surface tnum">
            {formatInteger(pipeline?.cursor ?? null)}
          </div>
          <div className="mt-1.5 text-xs text-on-surface-variant">
            last committed version
          </div>
        </div>

        <div className="mt-auto pt-5">
          {done !== null && (
            <div className="h-1 overflow-hidden rounded-full bg-surface-container-highest">
              <div
                className={`h-full rounded-full transition-[width] duration-700 ease-out ${
                  backfilling ? "bg-primary" : "bg-tertiary"
                }`}
                style={{ width: `${Math.max(done * 100, 0.5)}%` }}
              />
            </div>
          )}
          <div className="mt-2 flex items-baseline justify-between gap-2 text-[11px] text-on-surface-variant tnum">
            <span className="truncate">
              {project.idle
                ? "keeping records"
                : done === null
                  ? "no pipeline"
                  : backfilling
                    ? `${(done * 100).toFixed(1)}% backfilled`
                    : "following the chain"}
            </span>
            {/* An idle project's throughput is zero by design, and a zero here reads
                as a stall. */}
            {!project.idle && pipeline?.versions_per_sec != null && (
              <span className="shrink-0">
                {formatInteger(pipeline.versions_per_sec)}/s
              </span>
            )}
          </div>
        </div>
      </Card>
    </Link>
  );
}

function Overview() {
  const { data: status, error } = useStatus();
  const { tables } = useTables();
  const { data: usage } = useUsage();
  const { data: sources } = useSources();
  const { current, mode, hosted } = useProject();
  const href = useHref();

  if (error && !status) return <Offline error={error} />;
  if (!status) return <OverviewSkeleton />;

  const pipeline = status.pipeline;
  const phase = current?.state ?? pipeline?.phase ?? "serving";
  const cursor = pipeline?.cursor ?? status.build?.cursor ?? null;
  const failure = current?.error ?? pipeline?.last_error ?? null;

  return (
    <div>
      {/* The project and its network are named in the app bar, so the page says which
          page it is. */}
      <PageHeader
        title="Overview"
        hint={
          tables
            ? `${tables.length} state ${tables.length === 1 ? "table" : "tables"}`
            : undefined
        }
      ></PageHeader>

      <div className="flex max-w-6xl flex-col gap-6 px-8 py-6">
        {error && (
          <Notice tone="warning" title="Lost the API">
            {error}. Showing the last known state.
          </Notice>
        )}
        {/* The cap is a billing state, not a broken project, and it stops every
            project on the key at once — so it gets its own notice rather than the
            upstream sentence in a monospace box. */}
        {isCreditCap(failure) ? (
          <Notice tone="error" title="Out of monthly chain credit">
            <p>
              The Aptos API key this backend streams with has used up its
              organization&apos;s monthly credit, so the chain is refusing every
              request. Nothing is lost: the project stopped where it was and
              resumes from its cursor once the credit is back.
            </p>
            <p className="mt-2">
              The credit refreshes at the start of next month.{" "}
              <a
                href={GEOMI_BILLING}
                target="_blank"
                rel="noreferrer"
                className="underline underline-offset-2"
              >
                Attaching a payment method
              </a>{" "}
              lifts it now.
            </p>
          </Notice>
        ) : (
          <>
            {(phase === "halted" || phase === "failed") && (
              <Halted phase={phase} failure={failure} cursor={cursor} href={href} />
            )}
            {pipeline?.phase === "retrying" && (
              <Notice
                tone="warning"
                title={`Retrying (attempt ${pipeline.retries})`}
              >
                <span className="font-mono text-xs">{pipeline.last_error}</span>
              </Notice>
            )}
          </>
        )}
        {status.rebuild && <Rebuild status={status} />}
        <Silent sources={sources} status={status} />

        <Health status={status} />

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
              {hosted && (
                <>
                  {" "}
                  Send one of this project&apos;s API keys with each request.
                </>
              )}{" "}
              Try it in the{" "}
              <Link
                href={href("/playground")}
                className="text-primary hover:underline"
              >
                API
              </Link>{" "}
              page.
            </p>
          </Card>
        )}

        {usage && <Storage usage={usage} />}

        {status.build && (
          <Card className="grid grid-cols-1 gap-x-8 gap-y-2 px-4 py-3 text-xs text-on-surface-variant sm:grid-cols-3">
            <div>
              Build{" "}
              <span className="font-mono text-on-surface">
                {shortHex(`0x${status.build.fingerprint}`, 8, 6)}
              </span>
            </div>
            <div>
              Created {new Date(status.build.created_at).toLocaleString()}
            </div>
            <div>
              Last commit {new Date(status.build.updated_at).toLocaleString()}
            </div>
          </Card>
        )}
      </div>
    </div>
  );
}

/**
 * The record log is what makes a rebuild local instead of another backfill (ADR 0022),
 * so its size is really an answer to "how far back can this project be rebuilt without
 * paying for history again". That is the sentence, and the bar is the number.
 */
function Storage({ usage }: { usage: Usage }) {
  const used = usage.limit_bytes > 0 ? usage.bytes / usage.limit_bytes : 0;
  const tight = used > 0.9;
  return (
    <Card className="px-5 py-4">
      <div className="flex flex-wrap items-baseline justify-between gap-2">
        <div className="text-[11px] font-medium tracking-[0.08em] text-on-surface-variant uppercase">
          History kept
        </div>
        <div className="font-mono text-xs text-on-surface-variant tnum">
          {formatBytes(usage.bytes)} of {formatBytes(usage.limit_bytes)}
        </div>
      </div>
      <div className="mt-3 h-1.5 overflow-hidden rounded-full bg-surface-container-highest">
        <div
          className={`h-full rounded-full transition-[width] duration-700 ease-out ${
            tight ? "bg-warning" : "bg-primary"
          }`}
          style={{ width: `${Math.min(Math.max(used * 100, 0.5), 100)}%` }}
        />
      </div>
      <p className="mt-2.5 text-xs text-on-surface-variant">
        {formatInteger(usage.records)} records
        {usage.earliest_version && (
          <>
            , back to version{" "}
            <span className="font-mono text-on-surface">
              {formatInteger(usage.earliest_version)}
            </span>
          </>
        )}
        . Editing a rule replays these instead of re-reading the chain. Past the
        limit the oldest go first; your state tables are never touched.
      </p>
    </Card>
  );
}

/** What builds a table: the three kinds read differently, so they look different. */
/**
 * The one question this page exists to answer — *is my backend keeping up?* — as one
 * number, with the sentence that makes it mean something. Four equally loud stats
 * answered it four times and therefore not at all; those are the supporting row now.
 */
function Health({ status }: { status: Status }) {
  const pipeline = status.pipeline;
  const cursor = pipeline?.cursor ?? status.build?.cursor ?? null;
  // "Chain head" and "behind" are about where the chain is; progress is about what the
  // run is working towards, which stops moving on purpose.
  const chain = pipeline?.chain_head ?? pipeline?.chain_version ?? null;
  const done =
    pipeline && pipeline.schema === status.schema
      ? progress(
          pipeline.start_version,
          pipeline.cursor,
          pipeline.chain_version,
        )
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
                  and{" "}
                  <span className="font-mono text-on-surface">
                    {formatInteger(chain)}
                  </span>{" "}
                  — the tables serve what has landed so far
                </>
              ) : (
                <>
                  versions behind the chain · committed through{" "}
                  <span className="font-mono text-on-surface">
                    {formatInteger(cursor)}
                  </span>
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
            <Icon
              name={backfilling ? "history" : "check_circle"}
              className="text-[14px]"
            />
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
      <dd className="mt-1 truncate font-mono text-sm text-on-surface tnum">
        {value}
      </dd>
    </div>
  );
}

/**
 * Sources that have never matched anything.
 *
 * Not an error — a contract can be quiet, and a project that follows a silent one is
 * working correctly. But the usual cause is a type name that is close and wrong, and
 * nothing else in the product mentions it: the project runs, the cursor climbs, and
 * the table stays empty. So it is raised only once the project has read far enough
 * that silence is worth remarking on, and it is phrased as a question rather than a
 * failure.
 */
function Silent({
  sources,
  status,
}: {
  sources: SourceInfo[] | null;
  status: Status;
}) {
  // Dismissible, unlike the notices above it. Those report a condition — a halted
  // pipeline, a lost API — and closing one would claim it had been dealt with. This one
  // is advice, and advice you have read is in the way. It comes back on reload, because
  // the sources really are still quiet.
  const [hidden, setHidden] = useState(false);
  if (hidden) return null;
  if (!sources || sources.length === 0) return null;

  // Far enough to be worth mentioning. Below this, a quiet minute proves nothing.
  const ENOUGH = 50_000n;
  const cursor = status.pipeline?.cursor ?? status.build?.cursor;
  const start = status.pipeline?.start_version;
  if (!cursor || !start) return null;
  let read: bigint;
  try {
    read = BigInt(cursor) - BigInt(start);
  } catch {
    return null;
  }
  if (read < ENOUGH) return null;

  const quiet = sources.filter((s) => s.matched === 0);
  if (quiet.length === 0) return null;

  return (
    <Notice
      tone="warning"
      onClose={() => setHidden(true)}
      title={
        quiet.length === 1
          ? `${quiet[0]!.name} hasn't matched anything yet`
          : `${quiet.length} sources haven't matched anything yet`
      }
    >
      <p>
        The project has read {read.toLocaleString()} versions and found no
        records for{" "}
        {quiet.map((s, i) => (
          <span key={s.name}>
            {i > 0 && ", "}
            <span className="font-mono text-xs">{s.follows}</span>
          </span>
        ))}
        .
      </p>
      <p className="mt-2">
        That is what a silent contract looks like, and also what a type name
        that is subtly wrong looks like —{" "}
        <span className="font-mono text-xs">NewBlock</span> and{" "}
        <span className="font-mono text-xs">NewBlockEvent</span> are different
        types. Check the name against the contract on a chain explorer.
      </p>
    </Notice>
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
      <span className="font-mono">{rebuild.schema}</span> catches up and swaps
      in
      {done !== null && <> — {(done * 100).toFixed(1)}% done</>}.
    </Notice>
  );
}
