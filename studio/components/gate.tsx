"use client";

import { usePathname } from "next/navigation";
import type { ReactNode } from "react";

import { SIGN_IN_URL } from "@/lib/api";
import { useProject } from "@/lib/project";

import { Sidebar } from "./sidebar";

/** Studio's frame, or the sign-in screen when a hosted Nineveh needs one. */
export function Gate({ children }: { children: ReactNode }) {
  const { mode } = useProject();
  const pathname = usePathname();
  // `/auth` is where signing in lands: it has to render before there's a session.
  if (pathname === "/auth") return <main className="min-w-0 flex-1">{children}</main>;
  if (mode === "signin") return <SignIn />;
  return (
    <>
      <Sidebar />
      <main className="min-w-0 flex-1 overflow-y-auto">{children}</main>
    </>
  );
}

function SignIn() {
  return (
    <main className="flex min-w-0 flex-1 items-center justify-center px-6">
      <div className="w-full max-w-sm text-center">
        <svg viewBox="0 0 20 20" className="mx-auto size-9 text-primary" aria-hidden>
          <path fill="currentColor" d="M8 3h4v3H8zM5 7h10v4H5zM2 12h16v5H2z" />
        </svg>
        <h1 className="mt-5 text-xl font-semibold tracking-tight">Sign in to Nineveh</h1>
        <p className="mt-2 text-sm text-on-surface-variant">
          A live backend for your Aptos contract, from its address. Nothing to run.
        </p>
        <a
          href={SIGN_IN_URL}
          className="mt-8 inline-flex w-full items-center justify-center gap-2 rounded-sm bg-primary px-4 py-2.5 text-sm font-medium text-on-surface shadow-e1 transition-colors hover:bg-primary"
        >
          <GitHubMark />
          Continue with GitHub
        </a>
        <p className="mt-4 text-xs text-on-surface-variant">
          Nineveh reads only your public GitHub profile.
        </p>
      </div>
    </main>
  );
}

function GitHubMark() {
  return (
    <svg viewBox="0 0 16 16" className="size-4" aria-hidden>
      <path
        fill="currentColor"
        d="M8 0C3.58 0 0 3.58 0 8c0 3.54 2.29 6.53 5.47 7.59.4.07.55-.17.55-.38 0-.19-.01-.82-.01-1.49-2.01.37-2.53-.49-2.69-.94-.09-.23-.48-.94-.82-1.13-.28-.15-.68-.52-.01-.53.63-.01 1.08.58 1.23.82.72 1.21 1.87.87 2.33.66.07-.52.28-.87.51-1.07-1.78-.2-3.64-.89-3.64-3.95 0-.87.31-1.59.82-2.15-.08-.2-.36-1.02.08-2.12 0 0 .67-.21 2.2.82.64-.18 1.32-.27 2-.27.68 0 1.36.09 2 .27 1.53-1.04 2.2-.82 2.2-.82.44 1.1.16 1.92.08 2.12.51.56.82 1.27.82 2.15 0 3.07-1.87 3.75-3.65 3.95.29.25.54.73.54 1.48 0 1.07-.01 1.93-.01 2.2 0 .21.15.46.55.38A8.013 8.013 0 0016 8c0-4.42-3.58-8-8-8z"
      />
    </svg>
  );
}
