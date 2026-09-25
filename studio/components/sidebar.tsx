"use client";

import Link from "next/link";
import { usePathname, useSearchParams } from "next/navigation";

import { behind, formatDuration, formatInteger } from "@/lib/format";
import { useStatus, useTables } from "@/lib/hooks";
import { useHref, useProject } from "@/lib/project";

import { ThemeToggle } from "./theme";
import { Icon } from "./ui";

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
      <ChainLag />
      <AccountMenu />
    </aside>
  );
}

/**
 * How far behind the chain this project is, kept in the drawer so it is answered on
 * every screen rather than only on Overview.
 */
function ChainLag() {
  const { base } = useProject();
  const { data: status } = useStatus();
  const pipeline = status?.pipeline;
  if (!base || !pipeline) return null;
  const behindBy = behind(pipeline.cursor, pipeline.chain_version);
  const caughtUp = behindBy === "0" || behindBy === null;
  return (
    <div className="mx-3 mb-2 rounded-md bg-surface-container px-3 py-2.5">
      <div className="flex items-center justify-between gap-2">
        <span className="flex items-center gap-1.5 text-xs font-medium text-on-surface">
          <span
            className={`size-1.5 rounded-full ${caughtUp ? "bg-tertiary" : "bg-primary"}`}
            aria-hidden
          />
          {caughtUp ? "Following the chain" : "Catching up"}
        </span>
        <span className="font-mono text-[11px] text-on-surface-variant tnum">
          {formatDuration(pipeline.lag_secs)}
        </span>
      </div>
      <dl className="mt-2 space-y-1 font-mono text-[11px] tnum">
        <div className="flex justify-between gap-2">
          <dt className="text-on-surface-variant">behind</dt>
          <dd className={caughtUp ? "text-on-surface-variant" : "text-on-surface"}>
            {formatInteger(behindBy)}
          </dd>
        </div>
        <div className="flex justify-between gap-2">
          <dt className="text-on-surface-variant">cursor</dt>
          <dd className="truncate text-on-surface-variant">{formatInteger(pipeline.cursor)}</dd>
        </div>
        <div className="flex justify-between gap-2">
          <dt className="text-on-surface-variant">chain head</dt>
          <dd className="truncate text-on-surface-variant">
            {formatInteger(pipeline.chain_version)}
          </dd>
        </div>
      </dl>
    </div>
  );
}

/**
 * Who is signed in, and how to stop being. Hosted shows the GitHub account; local mode
 * has nobody to sign out, and says so rather than leaving the drawer to end in nothing.
 */
function AccountMenu() {
  const { account, signOut, hosted } = useProject();
  if (!account) {
    return (
      <div className="flex items-center gap-2.5 border-t border-outline-variant px-4 py-3">
        <span className="grid size-7 shrink-0 place-items-center rounded-full bg-surface-container-high">
          <Icon name="computer" className="text-[16px] text-on-surface-variant" />
        </span>
        <span className="min-w-0 flex-1">
          <span className="block truncate text-sm text-on-surface">Local mode</span>
          <span className="block truncate text-[11px] text-on-surface-variant">
            {hosted ? "not signed in" : "loopback only, no sign-in"}
          </span>
        </span>
      </div>
    );
  }
  return (
    <div className="flex items-center gap-2.5 border-t border-outline-variant px-4 py-3">
      {account.avatar_url ? (
        <img src={account.avatar_url} alt="" className="size-7 shrink-0 rounded-full" />
      ) : (
        <span className="grid size-7 shrink-0 place-items-center rounded-full bg-surface-container-high">
          <Icon name="person" className="text-[16px] text-on-surface-variant" />
        </span>
      )}
      <span className="min-w-0 flex-1">
        <span className="block truncate text-sm text-on-surface">{account.login}</span>
        <span className="block truncate text-[11px] text-on-surface-variant">Signed in</span>
      </span>
      <button
        type="button"
        onClick={() => void signOut()}
        title="Sign out"
        aria-label="Sign out"
        className="state grid size-9 shrink-0 place-items-center rounded-full text-on-surface-variant"
      >
        <Icon name="logout" className="text-[18px]" />
      </button>
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
        <NavLink href={href("/settings")} active={pathname === "/settings"} icon="settings">
          Settings
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

function Logo() {
  // The gate: two pillars and the arch between them. Same mark as the site and the
  // favicon, drawn so it still reads at this size.
  return (
    <svg viewBox="0 0 24 18" className="h-[15px] w-5 text-primary" aria-hidden>
      <path fill="currentColor" d="M0 18 V7.74 L4.3 4.27 V18 Z M5 18 V7.2 A7 7 0 0 1 19 7.2 V18 H15.75 V7.2 A3.75 3.75 0 0 0 8.25 7.2 V18 Z M19.7 18 V4.27 L24 7.74 V18 Z" />
    </svg>
  );
}
