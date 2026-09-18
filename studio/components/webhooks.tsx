"use client";

import { useCallback, useEffect, useState } from "react";

import { type WebhookInfo, control } from "@/lib/api";

import { Button, Card } from "./ui";

/**
 * A project's webhook endpoints (ADR 0020): where its state changes are delivered, how
 * that's going, and the secret a receiver checks signatures with.
 *
 * Endpoints come from the config, so they're added and removed there; what lives here
 * is the part the config can't hold — the secret, and whether deliveries are landing.
 */
export function Webhooks({ project }: { project: string }) {
  const [hooks, setHooks] = useState<WebhookInfo[] | null>(null);
  const [shown, setShown] = useState<string | null>(null);
  const [copied, setCopied] = useState<string | null>(null);
  const [confirming, setConfirming] = useState<string | null>(null);
  const [error, setError] = useState<string | null>(null);

  const load = useCallback(() => {
    control
      .webhooks(project)
      .then(setHooks)
      .catch((e: unknown) => setError(e instanceof Error ? e.message : String(e)));
  }, [project]);
  useEffect(load, [load]);

  // Deliveries move on their own, so keep the health honest while this is open.
  useEffect(() => {
    const timer = setInterval(load, 5000);
    return () => clearInterval(timer);
  }, [load]);

  const copy = async (name: string, text: string) => {
    await navigator.clipboard.writeText(text);
    setCopied(name);
    setTimeout(() => setCopied((c) => (c === name ? null : c)), 1500);
  };

  const rotate = async (name: string) => {
    if (confirming !== name) {
      setConfirming(name);
      setTimeout(() => setConfirming((c) => (c === name ? null : c)), 4000);
      return;
    }
    setConfirming(null);
    try {
      await control.rotateWebhook(project, name);
      setShown(name);
      load();
    } catch (e) {
      setError(e instanceof Error ? e.message : String(e));
    }
  };

  if (hooks && hooks.length === 0) return null;

  return (
    <Card>
      <div className="border-b border-outline-variant px-4 py-3">
        <h2 className="text-sm font-medium">Webhooks</h2>
        <p className="text-xs text-on-surface-variant">
          Where this project&apos;s changes are delivered. Add and remove them in the config; check
          the signature with the secret below.
        </p>
      </div>

      {error && (
        <p className="border-b border-outline-variant px-4 py-2 text-sm text-error">{error}</p>
      )}

      <ul className="divide-y divide-outline-variant">
        {hooks?.map((hook) => (
          <li key={hook.name} className="px-4 py-3">
            <div className="flex flex-wrap items-baseline gap-x-3 gap-y-1">
              <span className="text-sm font-medium">{hook.name}</span>
              <code className="min-w-0 flex-1 truncate font-mono text-xs text-on-surface-variant">
                {hook.url}
              </code>
              <Health hook={hook} />
            </div>

            <div className="mt-1.5 flex flex-wrap items-center gap-1.5 text-xs text-on-surface-variant">
              {hook.on.map((on) => (
                <span
                  key={on}
                  className="rounded bg-surface-container-high px-1.5 py-0.5 font-mono"
                >
                  {on}
                </span>
              ))}
              <span>{hook.rows ? "with the row" : "keys only"}</span>
            </div>

            {hook.last_error && (
              <p className="mt-1.5 truncate text-xs text-error" title={hook.last_error}>
                last attempt: {hook.last_error}
              </p>
            )}

            <div className="mt-2 flex items-center gap-2">
              <code className="min-w-0 flex-1 truncate rounded bg-surface-container-high px-2 py-1 font-mono text-xs">
                {shown === hook.name ? hook.secret : `${hook.secret.slice(0, 10)}${"•".repeat(12)}`}
              </code>
              <Button onClick={() => setShown(shown === hook.name ? null : hook.name)}>
                {shown === hook.name ? "Hide" : "Reveal"}
              </Button>
              <Button onClick={() => void copy(hook.name, hook.secret)}>
                {copied === hook.name ? "Copied" : "Copy"}
              </Button>
              <Button tone="danger" onClick={() => void rotate(hook.name)}>
                {confirming === hook.name ? "Replace it?" : "Rotate"}
              </Button>
            </div>
          </li>
        ))}
      </ul>
    </Card>
  );
}

/** Whether deliveries are landing, in a few words. */
function Health({ hook }: { hook: WebhookInfo }) {
  if (hook.failures > 0) {
    return (
      <span className="text-xs text-error">
        failing · {hook.failures} {hook.failures === 1 ? "attempt" : "attempts"}
      </span>
    );
  }
  if (!hook.delivered)
    return <span className="text-xs text-on-surface-variant">nothing sent yet</span>;
  return <span className="text-xs text-on-tertiary-container">delivered to {hook.delivered}</span>;
}
