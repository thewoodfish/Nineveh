"use client";

import { useEffect, useRef, useState } from "react";

import { control } from "@/lib/api";

import { Button, Icon, IconButton } from "./ui";

/**
 * A project's `nineveh.yaml`, to read and to copy into your repo. Not to edit: a project
 * is configured through Studio's own surfaces — sources, tables, webhooks — and this is
 * the file they produce, shown so you can see what they did and keep it in version
 * control. Hand-editing it was a second way to say the same things, in a format nobody
 * should have to hold in their head.
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
  const ref = useRef<HTMLDialogElement>(null);
  const [text, setText] = useState("");
  const [error, setError] = useState<string | null>(null);
  const [copied, setCopied] = useState(false);
  // Separate from `open`: the element has to be in the top layer for a frame before the
  // slide can animate, and has to finish sliding out before it leaves.
  const [slid, setSlid] = useState(false);

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
      .then((p) => setText(p.config))
      .catch((e: unknown) => setError(e instanceof Error ? e.message : String(e)));
  }, [name, open]);

  const copy = async () => {
    await navigator.clipboard.writeText(text);
    setCopied(true);
    setTimeout(() => setCopied(false), 1500);
  };

  return (
    <dialog
      ref={ref}
      className={`sheet w-[min(46rem,100vw)] rounded-none bg-surface-container-low p-0 text-on-surface shadow-e4 transition-transform duration-300 ease-[cubic-bezier(0.2,0,0,1)] motion-reduce:transition-none ${
        slid ? "translate-x-0" : "translate-x-full"
      }`}
      onCancel={(e) => {
        e.preventDefault();
        onClose();
      }}
      onClick={(e) => {
        if (e.target === ref.current) onClose();
      }}
    >
      <div className="flex h-full flex-col">
        <header className="flex items-start justify-between gap-3 px-5 py-4">
          <div className="min-w-0">
            <h2 className="font-mono text-base text-on-surface">nineveh.yaml</h2>
            <p className="mt-0.5 text-xs text-on-surface-variant">
              What this project follows and what it builds, as Studio has configured it.
              The same file the CLI reads — keep a copy in your repo.
            </p>
          </div>
          <IconButton name="close" aria-label="Close" onClick={onClose} />
        </header>

        {error && (
          <div className="mx-5 mb-3 flex gap-3 rounded-md bg-error-container px-4 py-3 text-sm text-on-error-container">
            <Icon name="error" className="mt-px shrink-0 text-[20px]" />
            <div className="min-w-0 font-medium">{error}</div>
          </div>
        )}

        {/* No soft wrap: a file that reflows mid-identifier is unreadable, so it
            scrolls sideways the way an editor does. `tabIndex` so it can be scrolled
            from the keyboard, which a focusable textarea used to give for free. */}
        <pre
          tabIndex={0}
          aria-label="nineveh.yaml"
          className="mx-5 min-h-0 flex-1 overflow-auto rounded-md bg-surface-container px-4 py-3 font-mono text-[13px] leading-relaxed whitespace-pre text-on-surface outline-none focus:ring-1 focus:ring-primary"
        >
          {text}
        </pre>

        <footer className="flex items-center justify-end gap-2 px-5 py-4">
          <Button tone="text" onClick={() => void copy()}>
            <Icon name={copied ? "check" : "content_copy"} className="text-[18px]" />
            {copied ? "Copied" : "Copy"}
          </Button>
        </footer>
      </div>
    </dialog>
  );
}
