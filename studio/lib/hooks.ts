"use client";

import { useCallback, useEffect, useRef, useState } from "react";

import {
  type Change,
  type ReaderInfo,
  type SourceInfo,
  type Status,
  type Table,
  type Usage,
  control,
  getStatus,
  getTables,
} from "./api";
import { subscribe } from "./feed";
import { useProject } from "./project";

/** Poll `load` every `ms`, keeping the last good value and the last error. `null` waits. */
export function usePoll<T>(load: (() => Promise<T>) | null, ms: number) {
  const [data, setData] = useState<T | null>(null);
  const [error, setError] = useState<string | null>(null);
  useEffect(() => {
    setData(null);
    setError(null);
    if (!load) return;
    let alive = true;
    let timer: ReturnType<typeof setTimeout>;
    const tick = async () => {
      try {
        const value = await load();
        if (!alive) return;
        setData(value);
        setError(null);
      } catch (e) {
        if (alive) setError(e instanceof Error ? e.message : String(e));
      }
      if (alive) timer = setTimeout(tick, ms);
    };
    void tick();
    return () => {
      alive = false;
      clearTimeout(timer);
    };
  }, [load, ms]);
  return { data, error };
}

/**
 * What the open project is using of its allowance. Polled slowly: it moves in
 * megabytes over hours, and there is nothing to watch tick.
 */
export function useUsage() {
  const { mode, name } = useProject();
  const load = useCallback(() => control.usage(name ?? ""), [name]);
  return usePoll<Usage>(mode === "control" && name ? load : null, 30_000);
}

/** What each network's shared reader is doing (ADR 0021). */
export function useReaders() {
  const { mode } = useProject();
  const load = useCallback(() => control.readers(), []);
  return usePoll<ReaderInfo[]>(mode === "control" ? load : null, 5_000);
}

/** The open project's status. */
export function useStatus() {
  const { base } = useProject();
  const load = useCallback(() => getStatus(base ?? ""), [base]);
  return usePoll<Status>(base ? load : null, 1000);
}

/** The project's sources, with how many records each has ever matched. */
export function useSources() {
  const { mode, name } = useProject();
  const load = useCallback(() => control.sources(name ?? ""), [name]);
  return usePoll<SourceInfo[]>(mode === "control" && name ? load : null, 30_000);
}

/**
 * The open project's tables, reloaded when the feed resets (a rebuild swapped in).
 *
 * `withCounts` costs a query per table, so it is off unless the caller is going to put
 * the numbers on screen.
 */
export function useTables(withCounts = false) {
  const { base } = useProject();
  const [tables, setTables] = useState<Table[] | null>(null);
  const [error, setError] = useState<string | null>(null);
  const reload = useCallback(() => {
    if (!base) {
      setTables(null);
      return;
    }
    getTables(base, withCounts)
      .then((t) => {
        setTables(t);
        setError(null);
      })
      .catch((e: unknown) => setError(e instanceof Error ? e.message : String(e)));
  }, [base, withCounts]);
  useEffect(reload, [reload]);
  useFeed({ onReset: reload });
  return { tables, error, reload };
}

type FeedOptions = {
  tables?: string[];
  /** Changes since the last batch, oldest first, and how many were dropped. */
  onChanges?: (changes: Change[], dropped: number) => void;
  onReset?: () => void;
};

/**
 * The open project's change feed, from the newest change on, in batches a few times a
 * second (see `feed.ts`). Returns whether it's connected.
 */
export function useFeed({ tables, onChanges, onReset }: FeedOptions) {
  const { base } = useProject();
  const [connected, setConnected] = useState(false);
  const handlers = useRef({ onChanges, onReset });
  handlers.current = { onChanges, onReset };
  const listens = onChanges !== undefined;
  const filter = tables?.join(",") ?? "";
  useEffect(() => {
    if (!base) return;
    const unsubscribe = subscribe(base, {
      tables: filter ? filter.split(",") : undefined,
      onChanges: listens ? (changes, dropped) => handlers.current.onChanges?.(changes, dropped) : undefined,
      onReset: () => handlers.current.onReset?.(),
      onConnected: setConnected,
    });
    return () => {
      unsubscribe();
      setConnected(false);
    };
  }, [base, filter, listens]);
  return connected;
}
