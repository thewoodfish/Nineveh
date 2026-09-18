"use client";

import { useRouter } from "next/navigation";
import { useState, type ReactNode } from "react";

import { ApiKeys } from "@/components/api-keys";
import { ConfigPanel } from "@/components/config-panel";
import { ConfirmDialog } from "@/components/dialog";
import { PageHeader } from "@/components/page-header";
import { Button, Card, Icon, Offline } from "@/components/ui";
import { Webhooks } from "@/components/webhooks";
import { control } from "@/lib/api";
import { useProject } from "@/lib/project";

/**
 * Everything about a project that isn't watching it run: the config it was built from,
 * the keys and endpoints that reach it, and the two things that stop it existing. The
 * Overview used to carry the middle of that list, which left it half health report and
 * half administration.
 */
export default function Settings() {
  const { mode, current, hosted, error } = useProject();
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

        <Section title="Danger zone" hint="Both of these interrupt everything reading this API.">
          <DangerZone name={current.name} running={current.running} />
        </Section>
      </div>
    </div>
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
