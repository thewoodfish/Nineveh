"use client";

import { useEffect, useRef, useState } from "react";

import { ApiError, control } from "@/lib/api";
import { useProject } from "@/lib/project";
import { reducersFile, withReducersKey } from "@/lib/state-table";

import { ConfirmDialog } from "./dialog";
import { Button, Icon, IconButton } from "./ui";

function FileTab({
  active,
  onClick,
  children,
}: {
  active: boolean;
  onClick: () => void;
  children: React.ReactNode;
}) {
  return (
    <button
      type="button"
      onClick={onClick}
      aria-current={active ? "true" : undefined}
      className={`rounded-md px-3 py-1.5 font-mono text-xs transition-colors ${
        active
          ? "bg-surface-container-high text-on-surface"
          : "text-on-surface-variant hover:bg-surface-container"
      }`}
    >
      {children}
    </button>
  );
}

/** The reducers file a project gets when it doesn't have one yet. */
function starter(name: string) {
  return `// Reducers for ${name}. Each handler says what changes when a record arrives.
// See docs/dsl.md for the whole language — it is six statements.
//
// export const balances = table({
//   key:     { user: address },
//   columns: { balance: u128.default(0) },
// })
//
// on(<source>, (r) => {
//   balances.row(r.user).balance += u128(r.amount)
// })
`;
}

/**
 * A project's files: its `nineveh.yaml`, and its reducers when they're written in the
 * DSL (ADR 0025). Read them, copy them into your repo, or change them. Saving a change
 * that alters what's built rebuilds the tables beside the served ones (ADR 0016).
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
  const [savedReducers, setSavedReducers] = useState<string | null>(null);
  const [reducers, setReducers] = useState<string | null>(null);
  const [tab, setTab] = useState<"config" | "reducers">("config");
  const [error, setError] = useState<{ message: string; details?: string } | null>(null);
  const [saving, setSaving] = useState(false);
  const [copied, setCopied] = useState(false);
  const [discarding, setDiscarding] = useState(false);
  // Separate from `open`: the element has to be in the top layer for a frame before the
  // slide can animate, and has to finish sliding out before it leaves.
  const [slid, setSlid] = useState(false);

  const file = reducersFile(name);
  const changed = saved !== null && (text !== saved || reducers !== savedReducers);
  const editing = tab === "reducers" && reducers !== null;

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
        setSavedReducers(p.reducers ?? null);
        setReducers(p.reducers ?? null);
        setTab("config");
      })
      .catch((e: unknown) => setError({ message: e instanceof Error ? e.message : String(e) }));
  }, [name, open]);

  /** Closing with edits in the box would throw them away silently. */
  const tryClose = () => (changed ? setDiscarding(true) : onClose());

  const save = async () => {
    setSaving(true);
    setError(null);
    try {
      const updated = await control.update(name, text, reducers ?? undefined);
      setSaved(updated.config);
      setSavedReducers(updated.reducers ?? null);
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
    await navigator.clipboard.writeText(editing ? (reducers ?? "") : text);
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
              <h2 className="font-mono text-base text-on-surface">
                {editing ? file : "nineveh.yaml"}
              </h2>
              <p className="mt-0.5 text-xs text-on-surface-variant">
                {editing
                  ? "What changes when a record arrives. The same file the CLI reads."
                  : "The same file the CLI reads. Keep a copy in your repo."}
              </p>
            </div>
            <IconButton name="close" aria-label="Close" onClick={tryClose} />
          </header>

          {/* Two files, one at a time. A project with no reducers yet gets the key and
              a starter file here, which is the only step between YAML and the DSL. */}
          <div className="flex items-center gap-1 px-5 pb-3">
            <FileTab active={tab === "config"} onClick={() => setTab("config")}>
              nineveh.yaml
            </FileTab>
            {reducers === null ? (
              <Button
                tone="text"
                onClick={() => {
                  setText((yaml) => withReducersKey(yaml, file));
                  setReducers(starter(name));
                  setTab("reducers");
                }}
              >
                <Icon name="add" className="text-[18px]" />
                Add reducers
              </Button>
            ) : (
              <FileTab active={tab === "reducers"} onClick={() => setTab("reducers")}>
                {file}
              </FileTab>
            )}
          </div>

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

          {changed && !error && (
            <p className="mx-5 mb-3 rounded-md bg-surface-container-high px-4 py-3 text-xs leading-relaxed text-on-surface-variant">
              Saving pins the layouts again and restarts the project. If the change alters
              what&apos;s built, the tables are rebuilt beside the served ones and swapped in once
              caught up.
            </p>
          )}

          {/* `wrap="off"`: a file that reflows mid-identifier is unreadable, so it
              scrolls sideways the way an editor does. */}
          <textarea
            value={editing ? (reducers ?? "") : text}
            onChange={(e) =>
              editing ? setReducers(e.target.value) : setText(e.target.value)
            }
            spellCheck={false}
            wrap="off"
            aria-label={editing ? file : "nineveh.yaml"}
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
          setReducers(savedReducers);
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
