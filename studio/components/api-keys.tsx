"use client";

import { useCallback, useEffect, useState } from "react";

import { API_URL, type ApiKey, control } from "@/lib/api";

import { Button, Card } from "./ui";

/**
 * A project's API keys (ADR 0018): what an app sends to reach this project's API. A
 * new key is shown once; after that only its first characters are.
 */
export function ApiKeys({ project, api }: { project: string; api: string }) {
  const [keys, setKeys] = useState<ApiKey[] | null>(null);
  const [label, setLabel] = useState("");
  const [created, setCreated] = useState<ApiKey | null>(null);
  const [copied, setCopied] = useState(false);
  const [confirming, setConfirming] = useState<number | null>(null);
  const [error, setError] = useState<string | null>(null);
  const [busy, setBusy] = useState(false);

  const load = useCallback(() => {
    control
      .keys(project)
      .then(setKeys)
      .catch((e: unknown) => setError(e instanceof Error ? e.message : String(e)));
  }, [project]);
  useEffect(load, [load]);

  const create = async () => {
    setBusy(true);
    setError(null);
    try {
      const key = await control.createKey(project, label.trim() || "default");
      setCreated(key);
      setLabel("");
      load();
    } catch (e) {
      setError(e instanceof Error ? e.message : String(e));
    } finally {
      setBusy(false);
    }
  };

  const revoke = async (id: number) => {
    if (confirming !== id) {
      setConfirming(id);
      setTimeout(() => setConfirming((c) => (c === id ? null : c)), 4000);
      return;
    }
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

  const field =
    "rounded-md border border-zinc-200 bg-white px-2.5 py-1.5 text-sm focus:border-lapis-400 focus:outline-none dark:border-zinc-700 dark:bg-zinc-900";
  const url = `${API_URL}${api}/v1/tables`;

  return (
    <Card>
      <div className="flex flex-wrap items-center justify-between gap-3 border-b border-zinc-200 px-4 py-3 dark:border-zinc-800">
        <div>
          <h2 className="text-sm font-medium">API keys</h2>
          <p className="text-xs text-zinc-500">
            What your app sends to read this project. Safe in browser code: revoke one any time.
          </p>
        </div>
        <form
          className="flex gap-2"
          onSubmit={(e) => {
            e.preventDefault();
            void create();
          }}
        >
          <input
            value={label}
            onChange={(e) => setLabel(e.target.value)}
            placeholder="Label, e.g. web app"
            maxLength={100}
            className={`${field} w-44`}
          />
          <Button type="submit" tone="primary" disabled={busy}>
            Create key
          </Button>
        </form>
      </div>

      {error && <p className="border-b border-zinc-200 px-4 py-2 text-sm text-red-600 dark:border-zinc-800">{error}</p>}

      {created?.key && (
        <div className="border-b border-zinc-200 bg-emerald-50/60 px-4 py-3 dark:border-zinc-800 dark:bg-emerald-950/30">
          <div className="text-sm font-medium">Copy your key now: it won&apos;t be shown again.</div>
          <div className="mt-2 flex items-center gap-2">
            <code className="min-w-0 flex-1 truncate rounded bg-white px-2 py-1 font-mono text-xs dark:bg-zinc-900">
              {created.key}
            </code>
            <Button onClick={() => void copy(created.key ?? "")}>{copied ? "Copied" : "Copy"}</Button>
          </div>
          <pre className="mt-2 overflow-x-auto rounded-lg bg-zinc-900 px-3 py-2 font-mono text-xs text-zinc-100">
            {`curl -H 'Authorization: Bearer ${created.key}' \\\n  '${url}'`}
          </pre>
        </div>
      )}

      {keys && keys.length === 0 && !created && (
        <p className="px-4 py-3 text-sm text-zinc-500">No keys yet. Create one for each app that reads this project.</p>
      )}
      {keys && keys.length > 0 && (
        <ul className="divide-y divide-zinc-100 dark:divide-zinc-800">
          {keys.map((key) => (
            <li key={key.id} className="flex items-center gap-4 px-4 py-2.5 text-sm">
              <span className="w-40 truncate">{key.label}</span>
              <code className="font-mono text-xs text-zinc-500">{key.prefix}…</code>
              <span className="ml-auto text-xs text-zinc-400">
                {key.last_used_at ? `used ${new Date(key.last_used_at).toLocaleString()}` : "never used"}
              </span>
              <Button tone="danger" onClick={() => void revoke(key.id)}>
                {confirming === key.id ? "Revoke it?" : "Revoke"}
              </Button>
            </li>
          ))}
        </ul>
      )}
    </Card>
  );
}
