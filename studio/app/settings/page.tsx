"use client";

import { useRouter } from "next/navigation";
import { useState, type ReactNode } from "react";

import { ApiKeys } from "@/components/api-keys";
import { ConfigPanel } from "@/components/config-panel";
import { ConfirmDialog } from "@/components/dialog";
import { PageHeader } from "@/components/page-header";
import { Button, Card, Icon, Offline, PhaseDot } from "@/components/ui";
import { Webhooks } from "@/components/webhooks";
import { type Limits, type Usage, control } from "@/lib/api";
import { formatBytes, formatInteger } from "@/lib/format";
import { useReaders, useUsage } from "@/lib/hooks";
import { useProject } from "@/lib/project";

/**
 * Everything about a project that isn't watching it run: the config it was built from,
 * the keys and endpoints that reach it, and the two things that stop it existing. The
 * Overview used to carry the middle of that list, which left it half health report and
 * half administration.
 */
export default function Settings() {
  const { mode, current, hosted, error, limits } = useProject();
  const { data: usage } = useUsage();
  if (mode === "loading") return null;
  if (mode === "offline") return <Offline error={error ?? "Nineveh isn't answering"} />;
  if (!current) {
    return (
      <div>
        <PageHeader title="Settings" />
        <p className="px-6 py-6 text-sm text-on-surface-variant">Open a project first.</p>
      </div>
    );
  }
  return (
    <div>
      <PageHeader title="Settings" hint={current.name} />
      <div className="flex max-w-4xl flex-col gap-8 px-6 py-6">
        <Section
          title="Configuration"
          hint="The file the CLI reads. Saving a change that alters what's built rebuilds the tables beside the served ones and swaps them in once caught up."
        >
          <ConfigRow name={current.name} />
        </Section>

        {hosted && (
          <Section title="API keys" hint="Send one with every request to this project's API.">
            <ApiKeys project={current.name} api={current.api} />
          </Section>
        )}

        <Section
          title="Webhooks"
          hint="Signed deliveries of every change, to the endpoints declared in the config."
        >
          <Webhooks project={current.name} />
        </Section>

        <Section
          title="Plan"
          hint="What this project keeps, and what it is allowed. Nothing here is billed — Nineveh has no billing yet."
        >
          <Plan limits={limits} usage={usage} />
        </Section>

        <Section
          title="Ingest"
          hint="Nineveh reads each network once and hands the transactions to every project on it, so adding a project costs no new connection to Aptos."
        >
          <Readers />
        </Section>

        <Section title="Danger zone" hint="Both of these interrupt everything reading this API.">
          <DangerZone name={current.name} running={current.running} />
        </Section>
      </div>
    </div>
  );
}

/**
 * Limits as facts about the tier rather than a bill: the numbers, what they are
 * measured in, and — for the two that aren't here yet — that they are coming. There is
 * nothing to upgrade to, so nothing offers to.
 */
function Plan({ limits, usage }: { limits: Limits | null; usage: Usage | null }) {
  if (!limits) {
    return (
      <Card className="px-5 py-4 text-sm text-on-surface-variant">
        Running locally, on your own machine and your own Aptos key. Nothing is limited.
      </Card>
    );
  }
  const used = usage && limits.log_bytes > 0 ? usage.bytes / limits.log_bytes : 0;
  return (
    <Card className="divide-y divide-outline-variant">
      <div className="flex flex-wrap items-baseline justify-between gap-2 px-5 py-4">
        <div>
          <div className="text-base font-medium text-on-surface">{limits.name}</div>
          <p className="mt-0.5 text-xs text-on-surface-variant">
            {limits.projects} projects on {limits.networks.join(" and ")}. Mainnet and deeper
            history are coming soon.
          </p>
        </div>
        <span className="rounded-full bg-secondary-container px-2.5 py-1 text-xs font-medium text-on-secondary-container">
          Current plan
        </span>
      </div>
      <div className="px-5 py-4">
        <div className="flex flex-wrap items-baseline justify-between gap-2">
          <span className="text-sm text-on-surface">History kept</span>
          <span className="font-mono text-xs text-on-surface-variant tnum">
            {formatBytes(usage?.bytes ?? null)} of {formatBytes(limits.log_bytes)}
          </span>
        </div>
        <div className="mt-2.5 h-1.5 overflow-hidden rounded-full bg-surface-container-highest">
          <div
            className={`h-full rounded-full transition-[width] duration-700 ease-out ${
              used > 0.9 ? "bg-warning" : "bg-primary"
            }`}
            style={{ width: `${Math.min(Math.max(used * 100, 0.5), 100)}%` }}
          />
        </div>
        <p className="mt-2 text-xs text-on-surface-variant">
          {usage ? `${formatInteger(usage.records)} records` : "Reading…"} — what a rebuild replays
          instead of reading the chain again. Past the limit the oldest go first; state tables are
          never touched.
        </p>
      </div>
      <dl className="grid grid-cols-2 divide-outline-variant sm:grid-cols-3 sm:divide-x">
        <Fact label="Projects" value={`${limits.projects}`} />
        <Fact label="Change feed kept" value={`${limits.history_days} days`} />
        <Fact label="Start within" value={`${limits.look_back_hours} h of the tip`} />
      </dl>
    </Card>
  );
}

function Fact({ label, value }: { label: string; value: string }) {
  return (
    <div className="px-5 py-3.5">
      <dt className="text-xs text-on-surface-variant">{label}</dt>
      <dd className="mt-1 truncate font-mono text-sm text-on-surface tnum">{value}</dd>
    </div>
  );
}

/**
 * The shared readers (ADR 0021). Worth showing because it is the one resource a
 * hosted plane can actually run out of: Aptos caps concurrent streams per
 * organization, so "free slots" is a real number, not a gauge for its own sake.
 */
function Readers() {
  const { data: readers } = useReaders();
  if (!readers) return null;
  if (readers.length === 0) {
    return (
      <Card className="px-5 py-4 text-sm text-on-surface-variant">
        No network is being read yet.
      </Card>
    );
  }
  return (
    <Card className="divide-y divide-outline-variant">
      {readers.map((reader) => (
        <div key={reader.network} className="flex flex-wrap items-center gap-x-6 gap-y-2 px-5 py-4">
          <div className="flex min-w-32 items-center gap-2">
            <PhaseDot phase={reader.position ? "running" : "starting"} />
            <span className="font-mono text-sm text-on-surface">{reader.network}</span>
          </div>
          <div className="text-xs text-on-surface-variant">
            at version{" "}
            <span className="font-mono text-on-surface">{formatInteger(reader.position)}</span>
          </div>
          <div className="text-xs text-on-surface-variant">
            {reader.projects} {reader.projects === 1 ? "project" : "projects"} on one stream
          </div>
          <div className="ml-auto text-xs text-on-surface-variant">
            {reader.slots_free} catch-up {reader.slots_free === 1 ? "stream" : "streams"} free
          </div>
        </div>
      ))}
    </Card>
  );
}

function Section({ title, hint, children }: { title: string; hint: string; children: ReactNode }) {
  return (
    <section>
      <h2 className="text-base font-medium text-on-surface">{title}</h2>
      <p className="mt-1 max-w-2xl text-sm text-on-surface-variant">{hint}</p>
      <div className="mt-4">{children}</div>
    </section>
  );
}

function ConfigRow({ name }: { name: string }) {
  const [open, setOpen] = useState(false);
  return (
    <>
      <Card className="flex flex-wrap items-center justify-between gap-3 px-5 py-4">
        <span className="flex items-center gap-3">
          <Icon name="description" className="text-[20px] text-on-surface-variant" />
          <span className="font-mono text-sm text-on-surface">nineveh.yaml</span>
        </span>
        <Button tone="tonal" onClick={() => setOpen(true)}>
          Open editor
        </Button>
      </Card>
      <ConfigPanel name={name} open={open} onClose={() => setOpen(false)} />
    </>
  );
}

function DangerZone({ name, running }: { name: string; running: boolean }) {
  const { refresh } = useProject();
  const router = useRouter();
  const [busy, setBusy] = useState(false);
  const [confirming, setConfirming] = useState(false);
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

  return (
    <Card className="divide-y divide-outline-variant">
      <Row
        title={running ? "Stop the project" : "Start the project"}
        body={
          running
            ? "The API keeps serving what it already folded; nothing new arrives until it starts again."
            : "It resumes from its cursor, so nothing is counted twice."
        }
      >
        <Button
          tone="secondary"
          disabled={busy}
          onClick={() => void act(() => (running ? control.stop(name) : control.start(name)))}
        >
          {running ? "Stop" : "Start"}
        </Button>
      </Row>
      <Row
        title="Delete the project"
        body="Removes the project and every table Nineveh folded for it. The contract is untouched, but the backfill starts from nothing if you create it again."
      >
        <Button tone="danger" disabled={busy} onClick={() => setConfirming(true)}>
          Delete
        </Button>
      </Row>
      {error && <p className="px-5 py-3 text-sm text-error">{error}</p>}
      <ConfirmDialog
        danger
        open={confirming}
        busy={busy}
        onClose={() => setConfirming(false)}
        onConfirm={() => {
          void (async () => {
            await act(() => control.remove(name));
            setConfirming(false);
            router.push("/");
          })();
        }}
        title={`Delete ${name}?`}
        confirmLabel="Delete project"
      >
        This removes the project and every table Nineveh folded for it. The contract is untouched —
        but the backfill starts from nothing if you create it again.
      </ConfirmDialog>
    </Card>
  );
}

function Row({ title, body, children }: { title: string; body: string; children: ReactNode }) {
  return (
    <div className="flex flex-wrap items-center justify-between gap-4 px-5 py-4">
      <div className="min-w-0 flex-1">
        <div className="text-sm font-medium text-on-surface">{title}</div>
        <p className="mt-0.5 max-w-lg text-sm text-on-surface-variant">{body}</p>
      </div>
      {children}
    </div>
  );
}
