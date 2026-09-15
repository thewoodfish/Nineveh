"use client";

// Which Nineveh Studio is talking to, who's signed in, and which project is open.
//
// Against `nineveh up` (the control plane), Studio lists the account's projects and the
// open one is `?project=` in the URL. Hosted, that needs signing in with GitHub first
// (ADR 0018). Against `nineveh run --serve`, there's no control API and one project at
// the root.

import { useSearchParams } from "next/navigation";
import { createContext, useCallback, useContext, useEffect, useMemo, useState, type ReactNode } from "react";

import { API_URL, ApiError, type Me, type ProjectSummary, control, projectBase } from "./api";
import { clearSession, onSignedOut } from "./session";

/** `signin`: hosted, and nobody's signed in. */
type Mode = "loading" | "control" | "single" | "offline" | "signin";

type ProjectState = {
  mode: Mode;
  /** Hosted: who's signed in. */
  account: Me["account"];
  /** Whether projects need sign-in and keys: hosted rather than local. */
  hosted: boolean;
  signOut: () => Promise<void>;
  /** Every project, under the control plane. */
  projects: ProjectSummary[] | null;
  /** The open project's name, under the control plane. */
  name: string | null;
  /** The open project's summary, under the control plane. */
  current: ProjectSummary | null;
  /** Where the open project's API is, if one is open. */
  base: string | null;
  error: string | null;
  /** Read the project list again now, after a change. */
  refresh: () => Promise<void>;
};

const Context = createContext<ProjectState>({
  mode: "loading",
  account: null,
  hosted: false,
  signOut: async () => {},
  projects: null,
  name: null,
  current: null,
  base: null,
  error: null,
  refresh: async () => {},
});

export function ProjectProvider({ children }: { children: ReactNode }) {
  const name = useSearchParams().get("project");
  const [mode, setMode] = useState<Mode>("loading");
  const [projects, setProjects] = useState<ProjectSummary[] | null>(null);
  const [me, setMe] = useState<Me | null>(null);
  const [error, setError] = useState<string | null>(null);

  const refresh = useCallback(async () => {
    try {
      const who = await control.me();
      setMe(who);
      setProjects(await control.projects());
      setMode("control");
      setError(null);
    } catch (e) {
      if (e instanceof ApiError && e.status === 401) {
        setMode("signin");
        setProjects(null);
        setError(null);
      } else if (e instanceof ApiError && e.status === 404) {
        // A single project's server: no control API.
        setMode("single");
        setError(null);
      } else {
        setMode((m) => (m === "loading" ? "offline" : m));
        setError(e instanceof Error ? e.message : String(e));
      }
    }
  }, []);

  useEffect(() => onSignedOut(() => void refresh()), [refresh]);

  const signOut = useCallback(async () => {
    await control.logout().catch(() => {});
    clearSession();
    await refresh();
  }, [refresh]);

  useEffect(() => {
    let alive = true;
    let timer: ReturnType<typeof setTimeout>;
    const tick = async () => {
      await refresh();
      if (alive) timer = setTimeout(tick, 2000);
    };
    void tick();
    return () => {
      alive = false;
      clearTimeout(timer);
    };
  }, [refresh]);

  const value = useMemo<ProjectState>(() => {
    const current = projects?.find((p) => p.name === name) ?? null;
    const base = mode === "single" ? API_URL : mode === "control" && name ? projectBase(name) : null;
    return {
      mode,
      account: me?.account ?? null,
      hosted: me?.mode === "hosted",
      signOut,
      projects,
      name: mode === "control" ? name : null,
      current,
      base,
      error,
      refresh,
    };
  }, [mode, me, signOut, projects, name, error, refresh]);

  return <Context.Provider value={value}>{children}</Context.Provider>;
}

export function useProject(): ProjectState {
  return useContext(Context);
}

/** Links within the open project: `path` with `?project=` carried along. */
export function useHref(): (path: string, params?: Record<string, string>) => string {
  const { name } = useProject();
  return useCallback(
    (path: string, params: Record<string, string> = {}) => {
      const query = new URLSearchParams(name ? { project: name, ...params } : params).toString();
      return query ? `${path}?${query}` : path;
    },
    [name],
  );
}
