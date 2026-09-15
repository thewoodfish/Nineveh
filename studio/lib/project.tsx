"use client";

// Which Nineveh Studio is talking to, and which project is open.
//
// Against `nineveh up` (the control plane), Studio lists every project and the open one
// is `?project=` in the URL. Against `nineveh run --serve`, there's no control API and
// one project at the root.

import { useSearchParams } from "next/navigation";
import { createContext, useCallback, useContext, useEffect, useMemo, useState, type ReactNode } from "react";

import { API_URL, ApiError, type ProjectSummary, control, projectBase } from "./api";

type Mode = "loading" | "control" | "single" | "offline";

type ProjectState = {
  mode: Mode;
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
  const [error, setError] = useState<string | null>(null);

  const refresh = useCallback(async () => {
    try {
      setProjects(await control.projects());
      setMode("control");
      setError(null);
    } catch (e) {
      if (e instanceof ApiError && e.status === 404) {
        // A single project's server: no control API.
        setMode("single");
        setError(null);
      } else {
        setMode((m) => (m === "loading" ? "offline" : m));
        setError(e instanceof Error ? e.message : String(e));
      }
    }
  }, []);

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
    return { mode, projects, name: mode === "control" ? name : null, current, base, error, refresh };
  }, [mode, projects, name, error, refresh]);

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
