/**
 * The project's REST API and change feed.
 *
 * The key reaches the browser — the page calls the API directly — so it is a build-time
 * variable rather than a secret. Reads never touch the chain (`nineveh-api`: "It reads
 * the tables the store writes and never the chain"), so an exposed key costs stream
 * credit nothing; give the demo its own and revoke it when you're done.
 */
const KEY = import.meta.env.VITE_ECHELON_KEY ?? "";
const BASE = import.meta.env.VITE_ECHELON_BASE ?? "https://api.nineveh.dev/projects/echelon";

export const configured = KEY !== "";

export type Row = Record<string, unknown>;

export async function rows(table: string, query = ""): Promise<Row[]> {
  const res = await fetch(`${BASE}/v1/tables/${table}?${query}`, {
    headers: { Authorization: `Bearer ${KEY}` },
  });
  if (!res.ok) throw new Error(`${table}: ${res.status} ${res.statusText}`);
  const body: { rows?: Row[] } = await res.json();
  return body.rows ?? [];
}

/** A row change, as the feed sends it. */
export type Change = { version: string; seq: number; table: string; op: string };

export function changes(): EventSource {
  return new EventSource(`${BASE}/v1/changes?apikey=${encodeURIComponent(KEY)}`);
}
