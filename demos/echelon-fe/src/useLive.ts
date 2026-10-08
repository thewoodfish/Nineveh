import { useCallback, useEffect, useRef, useState } from "react";
import { changes, rows, type Change, type Row } from "./api";

/** The oldest version on screen, so the feed can replay how these rows got here. */
function earliest(...tables: Row[][]): string | undefined {
  const versions = tables.flat().map((r) => BigInt(String(r._version)));
  if (versions.length === 0) return undefined;
  const oldest = versions.reduce((a, b) => (b < a ? b : a));
  // One before it, because `after` is exclusive.
  return `${oldest - 1n}.0`;
}

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
  // What the last read saw, for the feed's starting position. State would be stale by
  // the time the promise resolves.
  const latest = useRef<{ markets: Row[]; vaults: Row[] }>({ markets: [], vaults: [] });

  const refresh = useCallback(async () => {
    try {
      const [m, v] = await Promise.all([
        rows("market", "limit=50&order=total_liability.desc"),
        rows("vault", "limit=100"),
      ]);
      latest.current = { markets: m, vaults: v };
      setMarkets(m);
      setVaults(v);
      setError(null);
    } catch (e) {
      setError(e instanceof Error ? e.message : String(e));
    }
  }, []);

  useEffect(() => {
    let source: EventSource | undefined;
    // Read the tables first, then open the feed just before the oldest row on screen,
    // so it replays the changes that produced what you are looking at rather than
    // starting blank and waiting for the chain to move. `after=beginning` would work
    // too and would grow without bound; this window is exactly what is on the page.
    void refresh().then(() => {
      source = changes(earliest(latest.current.markets, latest.current.vaults));
      source.addEventListener("open", () => setConnected(true));
      source.addEventListener("error", () => setConnected(false));
      source.addEventListener("change", (event) => {
        const change = JSON.parse((event as MessageEvent<string>).data) as Change;
        setFeed((f) => [change, ...f].slice(0, 60));
        window.clearTimeout(pending.current);
        pending.current = window.setTimeout(() => void refresh(), 400);
      });
    });
    const every = setInterval(() => void refresh(), 15000);
    return () => {
      clearInterval(every);
      window.clearTimeout(pending.current);
      source?.close();
    };
  }, [refresh]);

  return { markets, vaults, error, connected, feed };
}
