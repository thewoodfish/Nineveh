"use client";

import Link from "next/link";
import { usePathname, useSearchParams } from "next/navigation";
import { useEffect, useRef, useState } from "react";

import { useStatus, useTables } from "@/lib/hooks";
import { useHref, useProject } from "@/lib/project";

import { ThemeToggle } from "./theme";
import { Icon, PhaseDot } from "./ui";

export function Sidebar() {
  const { mode, base } = useProject();
  const pathname = usePathname();
  return (
    <aside className="flex w-60 shrink-0 flex-col bg-surface-container-low">
      <div className="px-4 pt-4 pb-3">
        <div className="flex items-center gap-2">
          <Link href="/" className="flex min-w-0 flex-1 items-center gap-2 px-1">
            <Logo />
            <span className="text-[15px] font-semibold tracking-tight text-on-surface">
              Nineveh
            </span>
            <span className="text-[15px] text-on-surface-variant">Studio</span>
          </Link>
          <ThemeToggle />
        </div>
        <div className="mt-4">{mode === "control" ? <Switcher /> : <SingleProject />}</div>
      </div>
      {base ? (
        <ProjectNav />
      ) : (
        mode === "control" && (
          <nav className="flex flex-1 flex-col gap-1 px-3 text-sm">
            <NavLink href="/" active={pathname === "/"} icon="folder">
              Projects
            </NavLink>
            <NavLink href="/new" active={pathname === "/new"} icon="add_circle">
              New project
            </NavLink>
          </nav>
        )
      )}
      <div className="mt-auto" />
      <AccountMenu />
    </aside>
  );
}

/** Hosted: who's signed in, and signing out. */
function AccountMenu() {
  const { account, signOut } = useProject();
  if (!account) return null;
  return (
    <div className="flex items-center gap-2 border-t border-outline-variant px-4 py-3">
      {account.avatar_url ? (
        <img src={account.avatar_url} alt="" className="size-6 rounded-full" />
      ) : (
        <span className="size-6 rounded-full bg-outline-variant" />
      )}
      <span className="min-w-0 flex-1 truncate text-sm">{account.login}</span>
      <button
        type="button"
        onClick={() => void signOut()}
        className="text-xs text-on-surface-variant hover:text-on-surface"
      >
        Sign out
      </button>
    </div>
  );
}

/** The open project, and every other one a click away. */
function Switcher() {
  const { projects, current, name } = useProject();
  const [open, setOpen] = useState(false);
  const ref = useRef<HTMLDivElement>(null);
  useEffect(() => {
    if (!open) return;
    const close = (e: MouseEvent) => {
      if (!ref.current?.contains(e.target as Node)) setOpen(false);
    };
    document.addEventListener("mousedown", close);
    return () => document.removeEventListener("mousedown", close);
  }, [open]);

  return (
    <div ref={ref} className="relative">
      <button
        type="button"
        onClick={() => setOpen(!open)}
        className="state w-full rounded-md bg-surface-container-high px-3 py-2 text-left text-on-surface"
      >
        <div className="flex items-center justify-between gap-2">
          <span className="truncate text-sm font-medium text-on-surface">
            {name ?? "All projects"}
          </span>
          <span className="flex shrink-0 items-center gap-1.5">
            {current && <PhaseDot phase={current.state} />}
            <Chevron />
          </span>
        </div>
        <div className="mt-0.5 text-xs text-on-surface-variant">
          {current
            ? current.network
            : `${projects?.length ?? 0} project${projects?.length === 1 ? "" : "s"}`}
        </div>
      </button>
      {open && (
        <div className="absolute inset-x-0 top-full z-20 mt-1 overflow-hidden rounded-sm bg-surface-container-high py-2 shadow-e2">
          {projects?.map((p) => (
            <Link
              key={p.name}
              href={`/?project=${encodeURIComponent(p.name)}`}
              onClick={() => setOpen(false)}
              className="state flex items-center justify-between gap-2 px-3 py-2 text-sm text-on-surface"
            >
              <span className="truncate">{p.name}</span>
              <PhaseDot phase={p.state} />
            </Link>
          ))}
          <div className="my-2 border-t border-outline-variant" />
          <Link
            href="/"
            onClick={() => setOpen(false)}
            className="state block px-3 py-2 text-sm text-on-surface-variant"
          >
            All projects
          </Link>
          <Link
            href="/new"
            onClick={() => setOpen(false)}
            className="state block px-3 py-2 text-sm font-medium text-primary"
          >
            + New project
          </Link>
        </div>
      )}
    </div>
  );
}

/** `nineveh run --serve`: one project, no control plane. */
function SingleProject() {
  const { data: status, error } = useStatus();
  const phase = error ? "offline" : (status?.pipeline?.phase ?? (status ? "serving" : "offline"));
  return (
    <div className="rounded-md bg-surface-container-high px-3 py-2">
      <div className="flex items-center justify-between gap-2">
        <span className="truncate text-sm font-medium text-on-surface">
          {status?.project ?? "No project"}
        </span>
        <PhaseDot phase={phase} />
      </div>
      <div className="mt-0.5 text-xs text-on-surface-variant">
        {status ? `${status.network} · ${status.schema}` : "API not reachable"}
      </div>
    </div>
  );
}

function ProjectNav() {
  const { tables } = useTables();
  const pathname = usePathname();
  const selected = useSearchParams().get("name");
  const href = useHref();
  return (
    <>
      <nav className="flex flex-col gap-1 px-3 text-sm">
        <NavLink href={href("/")} active={pathname === "/"} icon="dashboard">
          Overview
        </NavLink>
        <NavLink href={href("/changes")} active={pathname === "/changes"} icon="bolt">
          Change feed
        </NavLink>
        <NavLink href={href("/playground")} active={pathname === "/playground"} icon="terminal">
          API playground
        </NavLink>
        <NavLink href={href("/state")} active={pathname === "/state"} icon="add_circle">
          New state table
        </NavLink>
      </nav>

      <div className="mt-6 px-3 text-[11px] font-medium tracking-[0.08em] text-on-surface-variant uppercase">
        Tables
      </div>
      <nav className="mt-1 flex min-h-0 flex-1 flex-col gap-1 overflow-y-auto px-3 pb-4 text-sm">
        {tables?.map((table) => (
          <NavLink
            key={table.name}
            href={href("/tables", { name: table.name })}
            active={pathname === "/tables" && selected === table.name}
          >
            <span className="truncate font-mono text-[12.5px]">{table.name}</span>
            <span className="ml-auto shrink-0 text-[11px] text-on-surface-variant">
              {table.kind}
            </span>
          </NavLink>
        ))}
        {tables?.length === 0 && (
          <p className="px-4 text-xs text-on-surface-variant">No state tables.</p>
        )}
      </nav>
    </>
  );
}

function NavLink({
  href,
  active,
  icon,
  children,
}: {
  href: string;
  active: boolean;
  icon?: string;
  children: React.ReactNode;
}) {
  return (
    <Link
      href={href}
      className={`state flex h-10 items-center gap-3 rounded-xl px-4 text-sm font-medium ${
        active ? "bg-secondary-container text-on-secondary-container" : "text-on-surface-variant"
      }`}
    >
      {icon && <Icon name={icon} filled={active} className="shrink-0 text-[20px]" />}
      {children}
    </Link>
  );
}

function Chevron() {
  return (
    <svg viewBox="0 0 16 16" className="size-3.5 text-on-surface-variant" aria-hidden>
      <path fill="none" stroke="currentColor" strokeWidth="1.5" d="M4.5 6.5 8 10l3.5-3.5" />
    </svg>
  );
}

function Logo() {
  // A stepped ziggurat: Nineveh's skyline, and state built up layer on layer.
  return (
    <svg viewBox="0 0 20 20" className="size-[18px] text-primary" aria-hidden>
      <path fill="currentColor" d="M8 3h4v3H8zM5 7h10v4H5zM2 12h16v5H2z" />
    </svg>
  );
}
