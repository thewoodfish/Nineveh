"use client";

import { useEffect, useState } from "react";

import { ApiError, control } from "@/lib/api";
import { useProject } from "@/lib/project";

import { Button } from "./ui";

/**
 * A project's `nineveh.yaml`: read it, copy it into your repo, or change it. Saving a
 * change that alters what's built rebuilds the tables beside the served ones (ADR 0016).
 */
export function ConfigPanel({ name, onClose }: { name: string; onClose: () => void }) {
  const { refresh } = useProject();
  const [saved, setSaved] = useState<string | null>(null);
  const [text, setText] = useState("");
  const [error, setError] = useState<{
    message: string;
    details?: string;
  } | null>(null);
  const [saving, setSaving] = useState(false);
  const [copied, setCopied] = useState(false);

  useEffect(() => {
    control
      .project(name)
      .then((p) => {
        setSaved(p.config);
        setText(p.config);
      })
      .catch((e: unknown) => setError({ message: e instanceof Error ? e.message : String(e) }));
  }, [name]);

  useEffect(() => {
    const onKey = (e: KeyboardEvent) => e.key === "Escape" && onClose();
    window.addEventListener("keydown", onKey);
    return () => window.removeEventListener("keydown", onKey);
  }, [onClose]);

  const save = async () => {
    setSaving(true);
    setError(null);
    try {
      const updated = await control.update(name, text);
      setSaved(updated.config);
      await refresh();
    } catch (e) {
      setError(
        e instanceof ApiError
          ? { message: e.message, details: e.details }
          : { message: e instanceof Error ? e.message : String(e) },
      );
    } finally {
      setSaving(false);
    }
  };

  const copy = async () => {
    await navigator.clipboard.writeText(text);
    setCopied(true);
    setTimeout(() => setCopied(false), 1500);
  };

  const changed = saved !== null && text !== saved;

  return (
    <div className="fixed inset-0 z-30 flex justify-end bg-black/60" onMouseDown={onClose}>
      <div
        className="flex h-full w-full max-w-2xl flex-col border-l border-line bg-card shadow-xl"
        onMouseDown={(e) => e.stopPropagation()}
      >
        <div className="flex items-center justify-between gap-3 border-b border-line px-5 py-4">
          <div>
            <div className="font-semibold tracking-tight">nineveh.yaml</div>
            <div className="text-xs text-dim">
              The same file the CLI reads. Keep a copy in your repo.
            </div>
          </div>
          <div className="flex items-center gap-2">
            <Button onClick={() => void copy()}>{copied ? "Copied" : "Copy"}</Button>
            <Button tone="primary" disabled={!changed || saving} onClick={() => void save()}>
              {saving ? "Saving…" : "Save changes"}
            </Button>
            <Button onClick={onClose} aria-label="Close">
              ✕
            </Button>
          </div>
        </div>
        {error && (
          <div className="border-b border-red-500/30 bg-red-500/15 px-5 py-3 text-sm text-red-200">
            <div className="font-medium">{error.message}</div>
            {error.details && (
              <pre className="mt-2 overflow-x-auto font-mono text-xs whitespace-pre">
                {error.details}
              </pre>
            )}
          </div>
        )}
        {changed && !error && (
          <div className="border-b border-line bg-well px-5 py-2 text-xs text-dim">
            Saving pins the layouts again and restarts the project. If the change alters what&apos;s
            built, the tables are rebuilt beside the served ones and swapped in once caught up.
          </div>
        )}
        <textarea
          value={text}
          onChange={(e) => setText(e.target.value)}
          spellCheck={false}
          className="min-h-0 flex-1 resize-none bg-transparent px-5 py-4 font-mono text-[13px] leading-relaxed focus:outline-none"
        />
      </div>
    </div>
  );
}
