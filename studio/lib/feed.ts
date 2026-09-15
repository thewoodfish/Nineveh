// One change feed per project, shared by everything on the page that listens to it,
// with changes delivered in batches.
//
// A busy contract commits hundreds of changes a second. Rendering each one as it lands
// would lock the page, and a stream per component would parse every change once each.
// So each project has one EventSource, each change is parsed once, and subscribers get
// what's arrived every FLUSH_MS.

import type { Change } from "./api";

const FLUSH_MS = 250;
/** Changes held per subscriber between flushes; a backgrounded tab flushes late. */
const MAX_PENDING = 1000;

export type Subscriber = {
  /** Only these tables' changes; all of them if absent. */
  tables?: string[];
  /** The changes since the last batch, oldest first, and how many were dropped. */
  onChanges?: (changes: Change[], dropped: number) => void;
  /** A rebuild was swapped in: reload what's listed. */
  onReset?: () => void;
  onConnected?: (connected: boolean) => void;
};

type Entry = { subscriber: Subscriber; tables: Set<string> | null; pending: Change[]; dropped: number };

type Shared = {
  source: EventSource;
  entries: Set<Entry>;
  connected: boolean;
  timer: ReturnType<typeof setTimeout> | null;
};

const feeds = new Map<string, Shared>();

/** Listen to the change feed of the project at `base`. Returns the unsubscribe. */
export function subscribe(base: string, subscriber: Subscriber): () => void {
  const feed = feeds.get(base) ?? open(base);
  const entry: Entry = {
    subscriber,
    tables: subscriber.tables ? new Set(subscriber.tables) : null,
    pending: [],
    dropped: 0,
  };
  feed.entries.add(entry);
  subscriber.onConnected?.(feed.connected);
  return () => {
    feed.entries.delete(entry);
    if (feed.entries.size === 0) {
      feed.source.close();
      if (feed.timer !== null) clearTimeout(feed.timer);
      feeds.delete(base);
    }
  };
}

function open(base: string): Shared {
  // The browser reconnects by itself and resumes with Last-Event-ID, so no change is
  // missed across a blip.
  const source = new EventSource(`${base}/v1/changes`);
  const feed: Shared = { source, entries: new Set(), connected: false, timer: null };
  const setConnected = (connected: boolean) => {
    feed.connected = connected;
    for (const e of feed.entries) e.subscriber.onConnected?.(connected);
  };
  source.onopen = () => setConnected(true);
  source.onerror = () => setConnected(false);
  source.addEventListener("change", (event) => {
    let change: Change | null = null;
    for (const e of feed.entries) {
      if (!e.subscriber.onChanges) continue;
      change ??= JSON.parse((event as MessageEvent<string>).data) as Change;
      if (e.tables && !e.tables.has(change.table)) continue;
      e.pending.push(change);
      if (e.pending.length > MAX_PENDING) {
        e.pending.shift();
        e.dropped += 1;
      }
    }
    if (change && feed.timer === null) feed.timer = setTimeout(() => flush(feed), FLUSH_MS);
  });
  source.addEventListener("reset", () => {
    for (const e of feed.entries) {
      e.pending = [];
      e.dropped = 0;
      e.subscriber.onReset?.();
    }
  });
  feeds.set(base, feed);
  return feed;
}

function flush(feed: Shared) {
  feed.timer = null;
  for (const e of feed.entries) {
    if (e.pending.length === 0) continue;
    const { pending, dropped } = e;
    e.pending = [];
    e.dropped = 0;
    e.subscriber.onChanges?.(pending, dropped);
  }
}
