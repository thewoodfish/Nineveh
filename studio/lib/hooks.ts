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

/**
 * Poll `load` every `ms`. `null` waits.
 *
 * `pending` is the first load for the current `load`, still in flight — which is a
 * different thing from `data` being null, and the difference is the whole point. Both
 * are null before the first answer arrives, so a caller that only looked at `data`
 * could not tell "reading this now" from "there is nothing here", and every one of them
 * rendered nothing for both. That is why a cold Studio showed an empty page.
 *
 * The previous value is dropped when `load` changes, deliberately: that happens when the
 * open project changes, and the last project's numbers under this project's heading
 * would be worse than a skeleton. `pending` is what lets the caller draw the skeleton.
 */
export function usePoll<T>(load: (() => Promise<T>) | null, ms: number) {
  const [data, setData] = useState<T | null>(null);
  const [error, setError] = useState<string | null>(null);
  const [pending, setPending] = useState(load !== null);
  const [nonce, setNonce] = useState(0);
  // Two reasons to go again, and they want opposite things on screen. `load` changing
  // is a different project, so the last one's numbers go (see above). `reload` is the
  // same project asked again, right after something changed it — and a list that
  // blanked to a skeleton the moment you added to it would be a worse answer than the
  // one-second-stale one it already had.
  useEffect(() => {
    setData(null);
    setError(null);
    setPending(load !== null);
  }, [load]);
  useEffect(() => {
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
      // Settled either way: an error is an answer, and a caller showing a skeleton
      // until one arrives would otherwise shimmer forever against a dead plane.
      if (alive) setPending(false);
      if (alive) timer = setTimeout(tick, ms);
    };
    void tick();
    return () => {
      alive = false;
      clearTimeout(timer);
    };
  }, [load, ms, nonce]);
  const reload = useCallback(() => setNonce((n) => n + 1), []);
  return { data, error, pending, reload };
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
 * What a table is worth showing first. A `reduce` table is the one its author designed —
 * the state their app actually reads — while a `mirror` or a `log` is raw contract
 * output that Nineveh made because a source was followed. Config order is kept inside
 * each group, because that order is the author's too.
 */
const PRECEDENCE: Record<Table["kind"], number> = { reduce: 0, mirror: 1, log: 2 };

/**
 * The open project's tables, reloaded when the feed resets (a rebuild swapped in).
 *
 * Sorted here rather than at each call site, so the sidebar, the Tables page and the
 * playground's picker can't disagree about what comes first.
 *
 * `withCounts` costs a query per table, so it is off unless the caller is going to put
 * the numbers on screen.
 */
export function useTables(withCounts = false) {
  const { base } = useProject();
  const [tables, setTables] = useState<Table[] | null>(null);
  const [error, setError] = useState<string | null>(null);
  // As in `usePoll`: "no tables" and "haven't asked yet" are different answers, and the
  // Tables page, the sidebar and the playground picker all have to tell them apart.
  const [pending, setPending] = useState(true);
  const reload = useCallback(() => {
    if (!base) {
      setTables(null);
      setPending(false);
      return;
    }
    setPending(true);
    getTables(base, withCounts)
      .then((t) => {
        // Stable: `toSorted` keeps equal ranks in the order the config listed them.
        setTables(t.toSorted((a, b) => PRECEDENCE[a.kind] - PRECEDENCE[b.kind]));
        setError(null);
      })
      .catch((e: unknown) => setError(e instanceof Error ? e.message : String(e)))
      .finally(() => setPending(false));
  }, [base, withCounts]);
  useEffect(reload, [reload]);
  useFeed({ onReset: reload });
  return { tables, error, reload, pending };
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
