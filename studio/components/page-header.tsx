"use client";

// The one bar at the top of the working area. It answers *which project* and *which
// page* on a single line: the picker on the left, where a console keeps it, then the
// page's own title, then whatever that page lets you do.

import Link from "next/link";
import { useEffect, useRef, useState, type ReactNode } from "react";

import { useRouter } from "next/navigation";

import { control, type ProjectSummary } from "@/lib/api";
import { useStatus } from "@/lib/hooks";
import { useProject } from "@/lib/project";

import { ConfigPanel } from "./config-panel";
import { ConfirmDialog } from "./dialog";

import { Icon, IconButton, PhaseDot } from "./ui";

export function PageHeader({
  title,
  hint,
  children,
}: {
  title: ReactNode;
  hint?: ReactNode;
  children?: ReactNode;
}) {
  const { mode, current } = useProject();
  const showPicker = (mode === "control" && current !== null) || mode === "single";
  return (
    /* Chrome, not canvas. `bg-surface` put the header on exactly the tone the page
       content sits on, so the bar read as the top of the page rather than as part of the
       frame, and the sidebar was the only thing that looked like furniture. Sharing the
       sidebar's surface makes the two one band along the top and down the left, with the
       content recessed below it — one step in both themes, 0.992 over 0.953 in light and
       0.19 over 0.145 in dark. */
    /* `z-40`, not `z-10`: sticky with a z-index makes this a stacking context, so the
       menus inside it can never climb higher than the header itself does. At `z-10` that
       tied with the data grid's sticky `thead`, and a tie is settled by document order —
       the grid comes later, so an open menu went under it. The header now outranks
       anything the page can stack. */
    <header className="sticky top-0 z-40 flex flex-wrap items-center gap-x-4 gap-y-2 border-b border-outline-variant bg-surface-container-low px-6 py-3">
      <div className="min-w-0 flex-1">
        <h1 className="truncate text-[22px] leading-7 text-on-surface">{title}</h1>
        {hint && <p className="mt-0.5 truncate text-sm text-on-surface-variant">{hint}</p>}
      </div>
      {showPicker && (
        <>
          {mode === "control" ? <ProjectPicker /> : <SingleProject />}
          {current && <ProjectMenu project={current} />}
        </>
      )}
      {/* The rule only earns its place when there is something on both sides of it. */}
      {showPicker && children && (
        <span className="h-6 w-px shrink-0 bg-outline-variant" aria-hidden />
      )}
      {children && <div className="flex items-center gap-2">{children}</div>}
    </header>
  );
}

/** The open project, and every other one a click away. */
function ProjectPicker() {
  const { projects, current, name } = useProject();
  const [open, setOpen] = useState(false);
  const ref = useRef<HTMLDivElement>(null);

  useEffect(() => {
    if (!open) return;
    const close = (e: MouseEvent) => {
      if (!ref.current?.contains(e.target as Node)) setOpen(false);
    };
    const escape = (e: KeyboardEvent) => e.key === "Escape" && setOpen(false);
    document.addEventListener("mousedown", close);
    document.addEventListener("keydown", escape);
    return () => {
      document.removeEventListener("mousedown", close);
      document.removeEventListener("keydown", escape);
    };
  }, [open]);

  return (
    <div ref={ref} className="relative">
      <button
        type="button"
        onClick={() => setOpen(!open)}
        aria-haspopup="menu"
        aria-expanded={open}
        className="state flex h-9 items-center gap-2 rounded-xl bg-surface-container-high pr-2 pl-3 text-on-surface"
      >
        {current ? (
          <PhaseDot phase={current.state} />
        ) : (
          <Icon name="folder" className="text-[18px] text-on-surface-variant" />
        )}
        <span className="max-w-56 truncate text-sm font-medium">{name ?? "All projects"}</span>
        {current && (
          <span className="rounded-xs bg-surface-container-highest px-1.5 py-0.5 font-mono text-[11px] text-on-surface-variant">
            {current.network}
          </span>
        )}
        <Icon name="arrow_drop_down" className="text-[20px] text-on-surface-variant" />
      </button>

      {open && (
        <div
          role="menu"
          className="absolute top-full right-0 z-50 mt-1 min-w-72 overflow-hidden rounded-sm bg-menu py-2 shadow-e2"
        >
          {projects?.map((p) => (
            <Link
              key={p.name}
              href={`/?project=${encodeURIComponent(p.name)}`}
              onClick={() => setOpen(false)}
              className="state flex items-center gap-3 px-4 py-2 text-sm text-on-surface"
            >
              <PhaseDot phase={p.state} />
              <span className="min-w-0 flex-1 truncate">{p.name}</span>
              <span className="shrink-0 font-mono text-[11px] text-on-surface-variant">
                {p.network}
              </span>
              {p.name === name && <Icon name="check" className="text-[16px] text-primary" />}
            </Link>
          ))}
          {projects?.length === 0 && (
            <p className="px-4 py-2 text-sm text-on-surface-variant">No projects yet.</p>
          )}
          <div className="my-2 border-t border-outline-variant" />
          <Link
            href="/"
            onClick={() => setOpen(false)}
            className="state flex items-center gap-3 px-4 py-2 text-sm text-on-surface-variant"
          >
            <Icon name="grid_view" className="text-[18px]" />
            All projects
          </Link>
          <Link
            href="/new"
            onClick={() => setOpen(false)}
            className="state flex items-center gap-3 px-4 py-2 text-sm font-medium text-primary"
          >
            <Icon name="add" className="text-[18px]" />
            New project
          </Link>
        </div>
      )}
    </div>
  );
}

/** `nineveh run --serve`: one project, no control plane, so nothing to switch to. */
function SingleProject() {
  const { data: status, error } = useStatus();
  const phase = error ? "offline" : (status?.pipeline?.phase ?? (status ? "serving" : "offline"));
  return (
    <div className="flex h-9 items-center gap-2 rounded-xl bg-surface-container-high pr-3 pl-3">
      <PhaseDot phase={phase} />
      <span className="max-w-56 truncate text-sm font-medium text-on-surface">
        {status?.project ?? "No project"}
      </span>
      {status && (
        <span className="rounded-xs bg-surface-container-highest px-1.5 py-0.5 font-mono text-[11px] text-on-surface-variant">
          {status.network}
        </span>
      )}
    </div>
  );
}

/**
 * What you can do to the project, rather than to the page: its config, running or not,
 * and deleting it. They live behind an overflow menu beside the picker because they
 * follow the project onto every screen, and because a Delete button sitting in the
 * corner of every page is an accident waiting for a tired afternoon.
 */
function ProjectMenu({ project }: { project: ProjectSummary }) {
  const { refresh } = useProject();
  const router = useRouter();
  const [open, setOpen] = useState(false);
  const [busy, setBusy] = useState(false);
  const [confirming, setConfirming] = useState(false);
  const [showConfig, setShowConfig] = useState(false);
  const [error, setError] = useState<string | null>(null);
  const ref = useRef<HTMLDivElement>(null);

  useEffect(() => {
    if (!open) return;
    const close = (e: MouseEvent) => {
      if (!ref.current?.contains(e.target as Node)) setOpen(false);
    };
    const escape = (e: KeyboardEvent) => e.key === "Escape" && setOpen(false);
    document.addEventListener("mousedown", close);
    document.addEventListener("keydown", escape);
    return () => {
      document.removeEventListener("mousedown", close);
      document.removeEventListener("keydown", escape);
    };
  }, [open]);

  const act = async (action: () => Promise<unknown>) => {
    setBusy(true);
    setError(null);
    setOpen(false);
    try {
      await action();
      await refresh();
    } catch (e) {
      setError(e instanceof Error ? e.message : String(e));
    } finally {
      setBusy(false);
    }
  };

  const remove = async () => {
    await act(() => control.remove(project.name));
    setConfirming(false);
    router.push("/");
  };

  const item = "state flex w-full items-center gap-3 px-4 py-2 text-left text-sm";

  return (
    <div ref={ref} className="relative">
      {error && <span className="mr-2 text-xs text-error">{error}</span>}
      <IconButton
        name="more_vert"
        aria-label={`Actions for ${project.name}`}
        aria-haspopup="menu"
        aria-expanded={open}
        disabled={busy}
        onClick={() => setOpen(!open)}
      />
      {open && (
        <div
          role="menu"
          className="absolute top-full right-0 z-50 mt-1 min-w-56 overflow-hidden rounded-sm bg-menu py-2 shadow-e2"
        >
          <Link
            href={`/settings?project=${encodeURIComponent(project.name)}`}
            onClick={() => setOpen(false)}
            className={`${item} text-on-surface`}
          >
            <Icon name="settings" className="text-[18px] text-on-surface-variant" />
            Project settings
          </Link>
          <button
            type="button"
            className={`${item} text-on-surface`}
            onClick={() => {
              setShowConfig(true);
              setOpen(false);
            }}
          >
            <Icon name="description" className="text-[18px] text-on-surface-variant" />
            Edit nineveh.yaml
          </button>
          <button
            type="button"
            className={`${item} text-on-surface`}
            onClick={() =>
              void act(() =>
                project.running ? control.stop(project.name) : control.start(project.name),
              )
            }
          >
            <Icon
              name={project.running ? "pause" : "play_arrow"}
              className="text-[18px] text-on-surface-variant"
            />
            {project.running ? "Stop project" : "Start project"}
          </button>
          <div className="my-2 border-t border-outline-variant" />
          <button
            type="button"
            className={`${item} text-error`}
            onClick={() => {
              setConfirming(true);
              setOpen(false);
            }}
          >
            <Icon name="delete" className="text-[18px]" />
            Delete project
          </button>
        </div>
      )}

      <ConfigPanel name={project.name} open={showConfig} onClose={() => setShowConfig(false)} />
      <ConfirmDialog
        danger
        open={confirming}
        busy={busy}
        onClose={() => setConfirming(false)}
        onConfirm={() => void remove()}
        title={`Delete ${project.name}?`}
        confirmLabel="Delete project"
      >
        This removes the project and every table Nineveh folded for it. The contract is untouched —
        but the backfill starts from nothing if you create it again.
      </ConfirmDialog>
    </div>
  );
}
