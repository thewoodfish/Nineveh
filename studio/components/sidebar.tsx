"use client";

import Link from "next/link";
import { usePathname, useSearchParams } from "next/navigation";
import { Suspense } from "react";

import { useStatus, useTables } from "@/lib/hooks";

import { PhaseDot } from "./ui";

export function Sidebar() {
  return (
    <aside className="flex w-60 shrink-0 flex-col border-r border-zinc-200 bg-zinc-50/60 dark:border-zinc-800 dark:bg-zinc-900/40">
      <Suspense>
        <SidebarContent />
      </Suspense>
    </aside>
  );
}

function SidebarContent() {
  const { data: status, error } = useStatus();
  const { tables } = useTables();
  const pathname = usePathname();
  const selected = useSearchParams().get("name");
  const phase = error ? "offline" : (status?.pipeline?.phase ?? (status ? "serving" : "offline"));

  return (
    <>
      <div className="px-4 pt-4 pb-3">
        <div className="flex items-center gap-2">
          <Logo />
          <span className="text-sm font-semibold tracking-tight">Nineveh</span>
          <span className="text-sm text-zinc-400">Studio</span>
        </div>
        <div className="mt-4 rounded-lg border border-zinc-200 bg-white px-3 py-2 dark:border-zinc-800 dark:bg-zinc-900">
          <div className="flex items-center justify-between gap-2">
            <span className="truncate text-sm font-medium">{status?.project ?? "No project"}</span>
            <PhaseDot phase={phase} />
          </div>
          <div className="mt-0.5 text-xs text-zinc-500">
            {status ? `${status.network} · ${status.schema}` : "API not reachable"}
          </div>
        </div>
      </div>

      <nav className="flex flex-col gap-0.5 px-2 text-sm">
        <NavLink href="/" active={pathname === "/"}>
          Overview
        </NavLink>
        <NavLink href="/changes" active={pathname === "/changes"}>
          Change feed
        </NavLink>
        <NavLink href="/playground" active={pathname === "/playground"}>
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
            href={`/tables?name=${encodeURIComponent(table.name)}`}
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

function Logo() {
  // A stepped ziggurat: Nineveh's skyline, and state built up layer on layer.
  return (
    <svg viewBox="0 0 20 20" className="size-5 text-lapis-500" aria-hidden>
      <path fill="currentColor" d="M8 3h4v3H8zM5 7h10v4H5zM2 12h16v5H2z" />
    </svg>
  );
}
