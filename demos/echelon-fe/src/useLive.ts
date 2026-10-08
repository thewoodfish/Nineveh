import { useCallback, useEffect, useRef, useState } from "react";
import { changes, rows, type Change, type Row } from "./api";

export type Live = {
  markets: Row[];
  vaults: Row[];
  error: string | null;
  /** Null until the feed is open, so the page can say "connecting" honestly. */
  connected: boolean;
  feed: Change[];
};

/**
 * Everything on the page, kept current two ways.
 *
 * The feed says *when* something changed, which is the cheap signal; the tables are
 * re-read when it does. Polling on a timer as well, because a table can also be built
 * by a rebuild that the feed resets rather than streams.
 */
export function useLive(): Live {
  const [markets, setMarkets] = useState<Row[]>([]);
  const [vaults, setVaults] = useState<Row[]>([]);
  const [error, setError] = useState<string | null>(null);
  const [connected, setConnected] = useState(false);
  const [feed, setFeed] = useState<Change[]>([]);
  const pending = useRef<number | undefined>(undefined);

  const refresh = useCallback(async () => {
    try {
      const [m, v] = await Promise.all([
        rows("market", "limit=50&order=total_liability.desc"),
        rows("vault", "limit=100"),
      ]);
      setMarkets(m);
      setVaults(v);
      setError(null);
    } catch (e) {
      setError(e instanceof Error ? e.message : String(e));
    }
  }, []);

  useEffect(() => {
    void refresh();
    const every = setInterval(() => void refresh(), 15000);
    const source = changes();
    source.addEventListener("open", () => setConnected(true));
    source.addEventListener("error", () => setConnected(false));
    source.addEventListener("change", (event) => {
      const change = JSON.parse((event as MessageEvent<string>).data) as Change;
      setFeed((f) => [change, ...f].slice(0, 60));
      // A burst of changes is one transaction's worth; read the tables once after it.
      window.clearTimeout(pending.current);
      pending.current = window.setTimeout(() => void refresh(), 400);
    });
    return () => {
      clearInterval(every);
      window.clearTimeout(pending.current);
      source.close();
    };
  }, [refresh]);

  return { markets, vaults, error, connected, feed };
}
