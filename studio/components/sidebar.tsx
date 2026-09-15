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
    <aside className="flex w-60 shrink-0 flex-col border-r border-zinc-200 bg-zinc-50/60 dark:border-zinc-800 dark:bg-zinc-900/40">
      <div className="px-4 pt-4 pb-3">
        <Link href="/" className="flex items-center gap-2">
          <Logo />
          <span className="text-sm font-semibold tracking-tight">Nineveh</span>
          <span className="text-sm text-zinc-400">Studio</span>
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
    <div className="mt-auto flex items-center gap-2 border-t border-zinc-200 px-4 py-3 dark:border-zinc-800">
      {account.avatar_url ? (
        <img src={account.avatar_url} alt="" className="size-6 rounded-full" />
      ) : (
        <span className="size-6 rounded-full bg-zinc-200 dark:bg-zinc-700" />
      )}
      <span className="min-w-0 flex-1 truncate text-sm">{account.login}</span>
      <button
        type="button"
        onClick={() => void signOut()}
        className="text-xs text-zinc-500 hover:text-zinc-900 dark:hover:text-zinc-100"
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
        className="w-full rounded-lg border border-zinc-200 bg-white px-3 py-2 text-left hover:border-zinc-300 dark:border-zinc-800 dark:bg-zinc-900 dark:hover:border-zinc-700"
      >
        <div className="flex items-center justify-between gap-2">
          <span className="truncate text-sm font-medium">{name ?? "All projects"}</span>
          {current ? <PhaseDot phase={current.state} /> : <Chevron />}
        </div>
        <div className="mt-0.5 text-xs text-zinc-500">
          {current
            ? current.network
            : `${projects?.length ?? 0} project${projects?.length === 1 ? "" : "s"}`}
        </div>
      </button>
      {open && (
        <div className="absolute inset-x-0 top-full z-20 mt-1 overflow-hidden rounded-lg border border-zinc-200 bg-white py-1 shadow-lg dark:border-zinc-800 dark:bg-zinc-900">
          {projects?.map((p) => (
            <Link
              key={p.name}
              href={`/?project=${encodeURIComponent(p.name)}`}
              onClick={() => setOpen(false)}
              className="flex items-center justify-between gap-2 px-3 py-1.5 text-sm hover:bg-zinc-50 dark:hover:bg-zinc-800"
            >
              <span className="truncate">{p.name}</span>
              <PhaseDot phase={p.state} />
            </Link>
          ))}
          <div className="my-1 border-t border-zinc-100 dark:border-zinc-800" />
          <Link
            href="/"
            onClick={() => setOpen(false)}
            className="block px-3 py-1.5 text-sm text-zinc-600 hover:bg-zinc-50 dark:text-zinc-400 dark:hover:bg-zinc-800"
          >
            All projects
          </Link>
          <Link
            href="/new"
            onClick={() => setOpen(false)}
            className="block px-3 py-1.5 text-sm font-medium text-lapis-600 hover:bg-zinc-50 dark:text-lapis-400 dark:hover:bg-zinc-800"
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
    <div className="rounded-lg border border-zinc-200 bg-white px-3 py-2 dark:border-zinc-800 dark:bg-zinc-900">
      <div className="flex items-center justify-between gap-2">
        <span className="truncate text-sm font-medium">{status?.project ?? "No project"}</span>
        <PhaseDot phase={phase} />
      </div>
      <div className="mt-0.5 text-xs text-zinc-500">
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
      </nav>

      <div className="mt-5 px-4 text-[11px] font-medium tracking-wider text-zinc-400 uppercase">
        Tables
      </div>
      <nav className="mt-1 flex min-h-0 flex-1 flex-col gap-0.5 overflow-y-auto px-2 pb-4 text-sm">
        {tables?.map((table) => (
          <NavLink
            key={table.name}
            href={href("/tables", { name: table.name })}
            active={pathname === "/tables" && selected === table.name}
          >
            <span className="truncate font-mono text-[13px]">{table.name}</span>
            <span className="ml-auto text-[11px] text-zinc-400">{table.kind}</span>
          </NavLink>
        ))}
        {tables?.length === 0 && <p className="px-2 text-xs text-zinc-400">No state tables.</p>}
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
      className={`flex items-center gap-2 rounded-md px-2 py-1.5 transition-colors ${
        active
          ? "bg-white font-medium text-zinc-900 shadow-sm ring-1 ring-zinc-200 dark:bg-zinc-800 dark:text-white dark:ring-zinc-700"
          : "text-zinc-600 hover:bg-zinc-100 hover:text-zinc-900 dark:text-zinc-400 dark:hover:bg-zinc-800/60 dark:hover:text-zinc-100"
      }`}
    >
      {children}
    </Link>
  );
}

function Chevron() {
  return (
    <svg viewBox="0 0 16 16" className="size-3.5 text-zinc-400" aria-hidden>
      <path fill="none" stroke="currentColor" strokeWidth="1.5" d="M4.5 6.5 8 10l3.5-3.5" />
    </svg>
  );
}

function Logo() {
  // A stepped ziggurat: Nineveh's skyline, and state built up layer on layer.
  return (
    <svg viewBox="0 0 20 20" className="size-5 text-lapis-500" aria-hidden>
      <path fill="currentColor" d="M8 3h4v3H8zM5 7h10v4H5zM2 12h16v5H2z" />
    </svg>
  );
}
