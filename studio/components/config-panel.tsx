"use client";

import { useEffect, useRef, useState } from "react";

import { ApiError, control } from "@/lib/api";
import { useProject } from "@/lib/project";

import { ConfirmDialog } from "./dialog";
import { Button, Icon, IconButton } from "./ui";

/**
 * A project's `nineveh.yaml`. Read it, copy it into your repo, or change it. Saving a
 * change that alters what's built rebuilds the tables beside the served ones (ADR 0016).
 *
 * The reducers used to be the second tab here, and they don't belong to a project the
 * way its config does — they say what one table holds, so they are edited on that
 * table's own page. They are still loaded and sent back untouched, because a config with
 * a `reducers:` key is refused without them (`compose`): this panel changes one of the
 * project's two files and has to hand the other back as it found it.
 *
 * It is a side sheet on the platform's `<dialog>`, so it borrows the top layer, the
 * backdrop, focus trapping and Escape rather than re-implementing them — and it stays
 * mounted while closed so it has something to animate out.
 */
export function ConfigPanel({
  name,
  open,
  onClose,
}: {
  name: string;
  open: boolean;
  onClose: () => void;
}) {
  const { refresh } = useProject();
  const ref = useRef<HTMLDialogElement>(null);
  const [saved, setSaved] = useState<string | null>(null);
  const [text, setText] = useState("");
  /** Never edited here, and never dropped either. */
  const [reducers, setReducers] = useState<string | undefined>(undefined);
  const [error, setError] = useState<{ message: string; details?: string } | null>(null);
  const [checked, setChecked] = useState<{ ok: boolean; details?: string } | null>(null);
  const [saving, setSaving] = useState(false);
  const [copied, setCopied] = useState(false);
  const [discarding, setDiscarding] = useState(false);
  // Separate from `open`: the element has to be in the top layer for a frame before the
  // slide can animate, and has to finish sliding out before it leaves.
  const [slid, setSlid] = useState(false);

  const changed = saved !== null && text !== saved;

  useEffect(() => {
    const el = ref.current;
    if (!el) return;
    if (open) {
      if (!el.open) el.showModal();
      const frame = requestAnimationFrame(() => setSlid(true));
      return () => cancelAnimationFrame(frame);
    }
    setSlid(false);
    const done = setTimeout(() => el.open && el.close(), 300);
    return () => clearTimeout(done);
  }, [open]);

  // Read the config when the sheet opens rather than on mount: it stays mounted so it
  // can animate, and the project's config is only interesting once it is on screen.
  useEffect(() => {
    if (!open) return;
    setError(null);
    control
      .project(name)
      .then((p) => {
        setSaved(p.config);
        setText(p.config);
        setReducers(p.reducers);
      })
      .catch((e: unknown) => setError({ message: e instanceof Error ? e.message : String(e) }));
  }, [name, open]);

  // Check with the server as it's written, not only when it's saved: a located
  // diagnostic is worth far more while the mistake is still on screen.
  useEffect(() => {
    if (!open || !changed) {
      setChecked(null);
      return;
    }
    const timer = setTimeout(() => {
      control
        .check(name, text, reducers)
        .then(() => setChecked({ ok: true }))
        .catch((e: unknown) =>
          setChecked({
            ok: false,
            details: e instanceof ApiError ? (e.details ?? e.message) : String(e),
          }),
        );
    }, 400);
    return () => clearTimeout(timer);
  }, [open, changed, name, text, reducers]);

  /** Closing with edits in the box would throw them away silently. */
  const tryClose = () => (changed ? setDiscarding(true) : onClose());

  const save = async () => {
    setSaving(true);
    setError(null);
    try {
      const updated = await control.update(name, text, reducers);
      setSaved(updated.config);
      setReducers(updated.reducers);
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

  return (
    <>
      <dialog
        ref={ref}
        className={`sheet w-[min(46rem,100vw)] rounded-none bg-surface-container-low p-0 text-on-surface shadow-e4 transition-transform duration-300 ease-[cubic-bezier(0.2,0,0,1)] motion-reduce:transition-none ${
          slid ? "translate-x-0" : "translate-x-full"
        }`}
        onCancel={(e) => {
          e.preventDefault();
          tryClose();
        }}
        onClick={(e) => {
          if (e.target === ref.current) tryClose();
        }}
      >
        <div className="flex h-full flex-col">
          <header className="flex items-start justify-between gap-3 px-5 py-4">
            <div className="min-w-0">
              <h2 className="font-mono text-base text-on-surface">nineveh.yaml</h2>
              <p className="mt-0.5 text-xs text-on-surface-variant">
                What this project follows and what it builds. The same file the CLI reads —
                keep a copy in your repo. A table&apos;s rules are on its own page.
              </p>
            </div>
            <IconButton name="close" aria-label="Close" onClick={tryClose} />
          </header>

          {error && (
            <div className="mx-5 mb-3 flex gap-3 rounded-md bg-error-container px-4 py-3 text-sm text-on-error-container">
              <Icon name="error" className="mt-px shrink-0 text-[20px]" />
              <div className="min-w-0">
                <div className="font-medium">{error.message}</div>
                {error.details && (
                  <pre className="mt-2 overflow-x-auto font-mono text-xs whitespace-pre">
                    {error.details}
                  </pre>
                )}
              </div>
            </div>
          )}

          {checked && !checked.ok && !error && (
            <div className="mx-5 mb-3 flex gap-3 rounded-md bg-error-container px-4 py-3 text-sm text-on-error-container">
              <Icon name="error" className="mt-px shrink-0 text-[20px]" />
              <div className="min-w-0">
                <div className="font-medium">Nineveh can&apos;t build that</div>
                <pre className="mt-2 overflow-x-auto font-mono text-xs whitespace-pre">
                  {checked.details}
                </pre>
              </div>
            </div>
          )}

          {changed && !error && (
            <p className="mx-5 mb-3 rounded-md bg-surface-container-high px-4 py-3 text-xs leading-relaxed text-on-surface-variant">
              {checked?.ok && (
                <span className="font-medium text-on-tertiary-container">Checks out. </span>
              )}
              Saving pins the layouts again and restarts the project. If the change alters
              what&apos;s built, the tables are rebuilt beside the served ones and swapped in once
              caught up.
            </p>
          )}

          {/* `wrap="off"`: a file that reflows mid-identifier is unreadable, so it
              scrolls sideways the way an editor does. */}
          <textarea
            value={text}
            onChange={(e) => setText(e.target.value)}
            spellCheck={false}
            wrap="off"
            aria-label="nineveh.yaml"
            className="mx-5 min-h-0 flex-1 resize-none overflow-auto rounded-md bg-surface-container px-4 py-3 font-mono text-[13px] leading-relaxed text-on-surface outline-none focus:ring-1 focus:ring-primary"
          />

          <footer className="flex items-center justify-end gap-2 px-5 py-4">
            <Button tone="text" onClick={() => void copy()}>
              <Icon name={copied ? "check" : "content_copy"} className="text-[18px]" />
              {copied ? "Copied" : "Copy"}
            </Button>
            <Button tone="primary" disabled={!changed || saving} onClick={() => void save()}>
              {saving ? "Saving…" : "Save changes"}
            </Button>
          </footer>
        </div>
      </dialog>

      <ConfirmDialog
        danger
        open={discarding}
        onClose={() => setDiscarding(false)}
        onConfirm={() => {
          setDiscarding(false);
          setText(saved ?? "");
          onClose();
        }}
        title="Discard your changes?"
        confirmLabel="Discard"
      >
        The config in the box hasn&apos;t been saved. Closing now leaves the project running the
        version it already had.
      </ConfirmDialog>
    </>
  );
}
