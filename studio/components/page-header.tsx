"use client";

// The one bar at the top of the working area. It answers *which project* and *which
// page* on a single line: the picker on the left, where a console keeps it, then the
// page's own title, then whatever that page lets you do.

import Link from "next/link";
import { useEffect, useRef, useState, type ReactNode } from "react";

import { useStatus } from "@/lib/hooks";
import { useProject } from "@/lib/project";

import { Icon, PhaseDot } from "./ui";

export function PageHeader({
  title,
  hint,
  children,
}: {
  title: ReactNode;
  hint?: ReactNode;
  children?: ReactNode;
}) {
  const { mode } = useProject();
  const showPicker = mode === "control" || mode === "single";
  return (
    <header className="sticky top-0 z-10 flex flex-wrap items-center gap-x-4 gap-y-2 border-b border-outline-variant bg-surface px-6 py-3">
      {showPicker && (
        <>
          {mode === "control" ? <ProjectPicker /> : <SingleProject />}
          <span className="h-6 w-px shrink-0 bg-outline-variant" aria-hidden />
        </>
      )}
      <div className="min-w-0 flex-1">
        <h1 className="truncate text-[22px] leading-7 text-on-surface">{title}</h1>
        {hint && <p className="mt-0.5 truncate text-sm text-on-surface-variant">{hint}</p>}
      </div>
      <div className="flex items-center gap-2">{children}</div>
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
          className="absolute top-full left-0 z-30 mt-1 min-w-72 overflow-hidden rounded-sm bg-surface-container-high py-2 shadow-e2"
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
