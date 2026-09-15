// One change feed per project, shared by everything on the page that listens to it,
// with changes delivered in batches.
//
// A busy contract commits hundreds of changes a second. Rendering each one as it lands
// would lock the page, and a stream per component would parse every change once each.
// So each project has one stream, each change is parsed once, and subscribers get
// what's arrived every FLUSH_MS.
//
// The stream is read with fetch rather than EventSource, which can't send headers: the
// session goes in `Authorization`, never in the URL (ADR 0018). Like EventSource, it
// reconnects by itself and resumes with Last-Event-ID, so no change is missed.

import { type Change, authHeaders } from "./api";

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
  abort: AbortController;
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
      feed.abort.abort();
      if (feed.timer !== null) clearTimeout(feed.timer);
      feeds.delete(base);
    }
  };
}

function open(base: string): Shared {
  const feed: Shared = { abort: new AbortController(), entries: new Set(), connected: false, timer: null };
  feeds.set(base, feed);
  void follow(base, feed);
  return feed;
}

function setConnected(feed: Shared, connected: boolean) {
  if (feed.connected === connected) return;
  feed.connected = connected;
  for (const e of feed.entries) e.subscriber.onConnected?.(connected);
}

/** Read the feed until it's closed, reconnecting after a failure. */
async function follow(base: string, feed: Shared) {
  let lastId: string | null = null;
  let delay = 1000;
  while (!feed.abort.signal.aborted) {
    try {
      const response = await fetch(`${base}/v1/changes`, {
        headers: {
          accept: "text/event-stream",
          ...authHeaders(),
          ...(lastId ? { "last-event-id": lastId } : {}),
        },
        cache: "no-store",
        signal: feed.abort.signal,
      });
      if (!response.ok || !response.body) throw new Error(`the feed answered ${response.status}`);
      setConnected(feed, true);
      delay = 1000;
      const reader = response.body.pipeThrough(new TextDecoderStream()).getReader();
      let buffer = "";
      for (;;) {
        const { value, done } = await reader.read();
        if (done) break;
        buffer += value.replace(/\r\n?/g, "\n");
        let end: number;
        while ((end = buffer.indexOf("\n\n")) >= 0) {
          const id = dispatch(feed, buffer.slice(0, end));
          if (id !== null) lastId = id;
          buffer = buffer.slice(end + 2);
        }
      }
    } catch {
      if (feed.abort.signal.aborted) return;
    }
    setConnected(feed, false);
    await new Promise((resolve) => setTimeout(resolve, delay));
    delay = Math.min(delay * 2, 15000);
  }
}

/** Handle one server-sent event, returning its id. */
function dispatch(feed: Shared, block: string): string | null {
  let event = "message";
  let id: string | null = null;
  const data: string[] = [];
  for (const line of block.split("\n")) {
    if (line === "" || line.startsWith(":")) continue;
    const colon = line.indexOf(":");
    const field = colon < 0 ? line : line.slice(0, colon);
    const value = colon < 0 ? "" : line.slice(colon + 1).replace(/^ /, "");
    if (field === "event") event = value;
    else if (field === "id") id = value;
    else if (field === "data") data.push(value);
  }
  if (event === "change") receive(feed, data.join("\n"));
  if (event === "reset") {
    for (const e of feed.entries) {
      e.pending = [];
      e.dropped = 0;
      e.subscriber.onReset?.();
    }
  }
  return id;
}

function receive(feed: Shared, text: string) {
  let change: Change | null = null;
  for (const e of feed.entries) {
    if (!e.subscriber.onChanges) continue;
    change ??= JSON.parse(text) as Change;
    if (e.tables && !e.tables.has(change.table)) continue;
    e.pending.push(change);
    if (e.pending.length > MAX_PENDING) {
      e.pending.shift();
      e.dropped += 1;
    }
  }
  if (change && feed.timer === null) feed.timer = setTimeout(() => flush(feed), FLUSH_MS);
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
