"use client";

import { useCallback, useEffect, useRef, useState } from "react";

import { API_URL, type Change, type Status, type Table, getStatus, getTables } from "./api";

/** Poll `load` every `ms`, keeping the last good value and the last error. */
export function usePoll<T>(load: () => Promise<T>, ms: number) {
  const [data, setData] = useState<T | null>(null);
  const [error, setError] = useState<string | null>(null);
  useEffect(() => {
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

export function useStatus() {
  return usePoll(getStatus, 1000);
}

/** The project's tables, reloaded when the feed resets (a rebuild swapped in). */
export function useTables() {
  const [tables, setTables] = useState<Table[] | null>(null);
  const [error, setError] = useState<string | null>(null);
  const reload = useCallback(() => {
    getTables()
      .then((t) => {
        setTables(t);
        setError(null);
      })
      .catch((e: unknown) => setError(e instanceof Error ? e.message : String(e)));
  }, []);
  useEffect(reload, [reload]);
  useFeed({ onReset: reload });
  return { tables, error, reload };
}

type FeedOptions = {
  tables?: string[];
  onChange?: (change: Change) => void;
  onReset?: () => void;
};

/**
 * The change feed, from the newest change on. The browser reconnects by itself and
 * resumes with Last-Event-ID, so no change is missed across a blip.
 */
export function useFeed({ tables, onChange, onReset }: FeedOptions) {
  const [connected, setConnected] = useState(false);
  const handlers = useRef({ onChange, onReset });
  handlers.current = { onChange, onReset };
  const filter = tables?.join(",") ?? "";
  useEffect(() => {
    const url = `${API_URL}/v1/changes${filter ? `?tables=${encodeURIComponent(filter)}` : ""}`;
    const source = new EventSource(url);
    source.onopen = () => setConnected(true);
    source.onerror = () => setConnected(false);
    source.addEventListener("change", (event) => {
      handlers.current.onChange?.(JSON.parse((event as MessageEvent<string>).data) as Change);
    });
    source.addEventListener("reset", () => handlers.current.onReset?.());
    return () => source.close();
  }, [filter]);
  return connected;
}
