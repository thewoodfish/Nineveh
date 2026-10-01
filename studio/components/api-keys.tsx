"use client";

import { useCallback, useEffect, useState } from "react";

import { API_URL, type ApiKey, control } from "@/lib/api";
import { formatDuration } from "@/lib/format";

import { ConfirmDialog, Dialog } from "./dialog";
import { Button, Card, Field, Icon, field } from "./ui";

/**
 * A project's API keys (ADR 0018): what an app sends to reach this project's API. A
 * new key is shown once; after that only its first characters are.
 *
 * The settings page owns the heading and the explanation, so this is the list and the
 * two things you do to it. Creating a key is occasional, so it asks for the label in a
 * dialog rather than keeping an empty text box on screen for the times you aren't.
 */
export function ApiKeys({ project, api }: { project: string; api: string }) {
  const [keys, setKeys] = useState<ApiKey[] | null>(null);
  const [created, setCreated] = useState<ApiKey | null>(null);
  const [copied, setCopied] = useState(false);
  const [naming, setNaming] = useState(false);
  const [confirming, setConfirming] = useState<number | null>(null);
  const [error, setError] = useState<string | null>(null);

  const load = useCallback(() => {
    control
      .keys(project)
      .then(setKeys)
      .catch((e: unknown) => setError(e instanceof Error ? e.message : String(e)));
  }, [project]);
  useEffect(load, [load]);

  const create = async (label: string) => {
    setError(null);
    try {
      setCreated(await control.createKey(project, label.trim() || "default"));
      setNaming(false);
      load();
    } catch (e) {
      setError(e instanceof Error ? e.message : String(e));
    }
  };

  const revoke = async (id: number) => {
    setConfirming(null);
    try {
      await control.revokeKey(project, id);
      if (created?.id === id) setCreated(null);
      load();
    } catch (e) {
      setError(e instanceof Error ? e.message : String(e));
    }
  };

  const copy = async (text: string) => {
    await navigator.clipboard.writeText(text);
    setCopied(true);
    setTimeout(() => setCopied(false), 1500);
  };

  const url = `${API_URL}${api}/v1/tables`;
  const revoking = keys?.find((k) => k.id === confirming) ?? null;

  return (
    <>
      <Card>
        {/* The count rather than the section's title again: it says something the
            heading above doesn't, and leaves the row to the one action. */}
        <div className="flex items-center justify-between gap-3 border-b border-outline-variant px-4 py-3">
          <span className="text-xs text-on-surface-variant">
            {keys === null ? "Reading this project's keys…" : counted(keys.length)}
          </span>
          <Button tone="primary" onClick={() => setNaming(true)}>
            <Icon name="add" className="text-[18px]" />
            Create key
          </Button>
        </div>

        {error && (
          <p className="border-b border-outline-variant px-4 py-2 text-sm text-error">{error}</p>
        )}

        {created?.key && (
          <div className="border-b border-outline-variant bg-tertiary-container px-4 py-3">
            <div className="text-sm font-medium">
              Copy your key now: it won&apos;t be shown again.
            </div>
            <div className="mt-2 flex items-center gap-2">
              <code className="min-w-0 flex-1 truncate rounded bg-surface-container-low px-2 py-1 font-mono text-xs">
                {created.key}
              </code>
              <Button onClick={() => void copy(created.key ?? "")}>
                {copied ? "Copied" : "Copy"}
              </Button>
            </div>
            <pre className="mt-2 overflow-x-auto rounded-sm bg-surface-container-high px-3 py-2 font-mono text-xs text-on-surface">
              {`curl -H 'Authorization: Bearer ${created.key}' \\\n  '${url}'`}
            </pre>
          </div>
        )}

        {keys && keys.length === 0 && !created && (
          <p className="px-4 py-3 text-sm text-on-surface-variant">
            No keys yet. Create one for each app that reads this project.
          </p>
        )}
        {keys && keys.length > 0 && (
          <ul className="divide-y divide-outline-variant">
            {keys.map((key) => (
              <li
                key={key.id}
                className="flex flex-wrap items-center gap-x-4 gap-y-1 px-4 py-2.5 text-sm"
              >
                <span className="min-w-0 flex-1 truncate">{key.label}</span>
                <code className="shrink-0 font-mono text-xs text-on-surface-variant">
                  {key.prefix}…
                </code>
                {/* Relative, like every other age in Studio: "used 3 d ago" answers
                    "is this one still in service?" that a locale timestamp doesn't. */}
                <span
                  className="shrink-0 text-xs text-on-surface-variant"
                  title={key.last_used_at ?? undefined}
                >
                  {used(key.last_used_at)}
                </span>
                <Button tone="danger" size="sm" onClick={() => setConfirming(key.id)}>
                  Revoke
                </Button>
              </li>
            ))}
          </ul>
        )}
      </Card>

      <NameIt open={naming} onClose={() => setNaming(false)} onCreate={create} />

      <ConfirmDialog
        danger
        open={confirming !== null}
        onClose={() => setConfirming(null)}
        onConfirm={() => confirming !== null && void revoke(confirming)}
        title="Revoke this key?"
        confirmLabel="Revoke key"
      >
        Anything still sending{" "}
        <span className="font-mono text-on-surface">{revoking?.prefix ?? ""}…</span> stops being
        answered straight away. This can&apos;t be undone; issue a new key instead.
      </ConfirmDialog>
    </>
  );
}

/** The label for a new key, which is the only thing creating one needs to know. */
function NameIt({
  open,
  onClose,
  onCreate,
}: {
  open: boolean;
  onClose: () => void;
  onCreate: (label: string) => Promise<void>;
}) {
  const [label, setLabel] = useState("");
  const [busy, setBusy] = useState(false);

  const submit = async () => {
    setBusy(true);
    try {
      await onCreate(label);
      setLabel("");
    } finally {
      setBusy(false);
    }
  };

  return (
    <Dialog
      open={open}
      onClose={onClose}
      title="Create an API key"
      actions={
        <>
          <Button tone="text" disabled={busy} onClick={onClose}>
            Cancel
          </Button>
          <Button tone="primary" disabled={busy} onClick={() => void submit()}>
            {busy ? "Creating…" : "Create key"}
          </Button>
        </>
      }
    >
      <Field
        label="Label"
        hint="Which app this one is for, so you know what you're revoking later."
      >
        <input
          value={label}
          autoFocus
          onChange={(e) => setLabel(e.target.value)}
          onKeyDown={(e) => {
            if (e.key === "Enter" && !busy) void submit();
          }}
          placeholder="web_app"
          maxLength={100}
          className={field}
        />
      </Field>
    </Dialog>
  );
}

function counted(n: number): string {
  return n === 1 ? "1 key" : `${n} keys`;
}

function used(at: string | null): string {
  if (!at) return "never used";
  const seconds = (Date.now() - new Date(at).getTime()) / 1000;
  if (Number.isNaN(seconds)) return "used just now";
  return `used ${formatDuration(Math.max(seconds, 0))} ago`;
}
