"use client";

import Link from "next/link";
import { usePathname, useSearchParams } from "next/navigation";
import { useEffect, useRef, useState } from "react";

import { useStatus, useTables } from "@/lib/hooks";
import { useHref, useProject } from "@/lib/project";

import { PhaseDot } from "./ui";

export function Sidebar() {
  const { mode, base } = useProject();
  const pathname = usePathname();
  return (
    <aside className="flex w-60 shrink-0 flex-col border-r border-line bg-black/25">
      <div className="px-4 pt-4 pb-3">
        <Link href="/" className="flex items-center gap-2 px-1">
          <Logo />
          <span className="text-[15px] font-semibold tracking-tight text-white">Nineveh</span>
          <span className="text-[15px] text-faint">Studio</span>
        </Link>
        <div className="mt-4">{mode === "control" ? <Switcher /> : <SingleProject />}</div>
      </div>
      {base ? (
        <ProjectNav />
      ) : (
        mode === "control" && (
          <nav className="flex flex-1 flex-col gap-0.5 px-2 text-sm">
            <NavLink href="/" active={pathname === "/"}>
              Projects
            </NavLink>
            <NavLink href="/new" active={pathname === "/new"}>
              New project
            </NavLink>
          </nav>
        )
      )}
      <AccountMenu />
    </aside>
  );
}

/** Hosted: who's signed in, and signing out. */
function AccountMenu() {
  const { account, signOut } = useProject();
  if (!account) return null;
  return (
    <div className="mt-auto flex items-center gap-2 border-t border-line px-4 py-3">
      {account.avatar_url ? (
        <img src={account.avatar_url} alt="" className="size-6 rounded-full" />
      ) : (
        <span className="size-6 rounded-full bg-white/12" />
      )}
      <span className="min-w-0 flex-1 truncate text-sm">{account.login}</span>
      <button
        type="button"
        onClick={() => void signOut()}
        className="text-xs text-dim hover:text-white"
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
        className="w-full rounded-lg border border-line bg-card px-3 py-2 text-left shadow-card transition-colors hover:border-edge"
      >
        <div className="flex items-center justify-between gap-2">
          <span className="truncate text-sm font-medium text-white">{name ?? "All projects"}</span>
          <span className="flex shrink-0 items-center gap-1.5">
            {current && <PhaseDot phase={current.state} />}
            <Chevron />
          </span>
        </div>
        <div className="mt-0.5 text-xs text-dim">
          {current
            ? current.network
            : `${projects?.length ?? 0} project${projects?.length === 1 ? "" : "s"}`}
        </div>
      </button>
      {open && (
        <div className="absolute inset-x-0 top-full z-20 mt-1 overflow-hidden rounded-lg border border-line bg-card py-1 shadow-lg">
          {projects?.map((p) => (
            <Link
              key={p.name}
              href={`/?project=${encodeURIComponent(p.name)}`}
              onClick={() => setOpen(false)}
              className="flex items-center justify-between gap-2 px-3 py-1.5 text-sm hover:bg-white/[0.06]"
            >
              <span className="truncate">{p.name}</span>
              <PhaseDot phase={p.state} />
            </Link>
          ))}
          <div className="my-1 border-t border-line" />
          <Link
            href="/"
            onClick={() => setOpen(false)}
            className="block px-3 py-1.5 text-sm text-dim hover:bg-white/[0.06]"
          >
            All projects
          </Link>
          <Link
            href="/new"
            onClick={() => setOpen(false)}
            className="block px-3 py-1.5 text-sm font-medium text-blue-300 hover:bg-white/[0.06]"
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
    <div className="rounded-lg border border-line bg-card px-3 py-2 shadow-card">
      <div className="flex items-center justify-between gap-2">
        <span className="truncate text-sm font-medium text-white">
          {status?.project ?? "No project"}
        </span>
        <PhaseDot phase={phase} />
      </div>
      <div className="mt-0.5 text-xs text-dim">
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
      <nav className="flex flex-col gap-0.5 px-2 text-sm">
        <NavLink href={href("/")} active={pathname === "/"}>
          Overview
        </NavLink>
        <NavLink href={href("/changes")} active={pathname === "/changes"}>
          Change feed
        </NavLink>
        <NavLink href={href("/playground")} active={pathname === "/playground"}>
          API playground
        </NavLink>
        <NavLink href={href("/state")} active={pathname === "/state"}>
          New state table
        </NavLink>
      </nav>

      <div className="mt-6 px-3 text-[11px] font-medium tracking-[0.1em] text-faint uppercase">
        Tables
      </div>
      <nav className="mt-1 flex min-h-0 flex-1 flex-col gap-0.5 overflow-y-auto px-2 pb-4 text-sm">
        {tables?.map((table) => (
          <NavLink
            key={table.name}
            href={href("/tables", { name: table.name })}
            active={pathname === "/tables" && selected === table.name}
          >
            <span className="truncate font-mono text-[12.5px]">{table.name}</span>
            <span className="ml-auto shrink-0 text-[11px] text-faint">{table.kind}</span>
          </NavLink>
        ))}
        {tables?.length === 0 && <p className="px-2 text-xs text-faint">No state tables.</p>}
      </nav>
    </>
  );
}

function NavLink({
  href,
  active,
  children,
}: {
  href: string;
  active: boolean;
  children: React.ReactNode;
}) {
  return (
    <Link
      href={href}
      className={`relative flex items-center gap-2 rounded-lg px-2.5 py-1.5 text-[13px] transition-colors ${
        active
          ? "bg-blue-500/15 font-medium text-blue-200"
          : "text-dim hover:bg-white/[0.06] hover:text-white"
      }`}
    >
      {active && (
        <span className="absolute top-1/2 -left-2 h-4 w-0.5 -translate-y-1/2 rounded-full bg-blue-400" />
      )}
      {children}
    </Link>
  );
}

function Chevron() {
  return (
    <svg viewBox="0 0 16 16" className="size-3.5 text-faint" aria-hidden>
      <path fill="none" stroke="currentColor" strokeWidth="1.5" d="M4.5 6.5 8 10l3.5-3.5" />
    </svg>
  );
}

function Logo() {
  // A stepped ziggurat: Nineveh's skyline, and state built up layer on layer.
  return (
    <svg viewBox="0 0 20 20" className="size-[18px] text-blue-400" aria-hidden>
      <path fill="currentColor" d="M8 3h4v3H8zM5 7h10v4H5zM2 12h16v5H2z" />
    </svg>
  );
}
