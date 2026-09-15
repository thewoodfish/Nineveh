// Nineveh's HTTP APIs, as `nineveh up` serves them: the control API at
// `/control/v1`, and each project's state API and change feed at `/projects/{name}`.
// `nineveh run --serve` and `nineveh serve` serve one project's API at the root, with no
// control API; Studio works against either.
//
// Wide integers (u64 and up, and versions) arrive as decimal strings so nothing loses
// precision in JavaScript (ADR 0008). Keep them as strings; format them with BigInt.

export const API_URL = (process.env.NEXT_PUBLIC_NINEVEH_API ?? "http://127.0.0.1:4000").replace(
  /\/$/,
  "",
);

export type Health = {
  phase: "starting" | "running" | "retrying" | "halted" | "stopped" | string;
  schema: string;
  cursor: string | null;
  start_version: string | null;
  chain_version: string | null;
  lag_secs: number | null;
  versions_per_sec: number | null;
  retries: number;
  last_error: string | null;
};

export type Build = {
  cursor: string | null;
  fingerprint: string;
  created_at: string;
  updated_at: string;
};

export type Status = {
  project: string;
  network: string;
  schema: string;
  build: Build | null;
  rebuild: (Build & { schema: string }) | null;
  pipeline: Health | null;
};

export type ColumnType =
  | "bool"
  | "u8"
  | "u16"
  | "u32"
  | "u64"
  | "u128"
  | "u256"
  | "i8"
  | "i16"
  | "i32"
  | "i64"
  | "i128"
  | "i256"
  | "address"
  | "string"
  | "bytes"
  | "json";

export type Column = { name: string; type: ColumnType; nullable: boolean };

export type Table = {
  name: string;
  kind: "reduce" | "mirror" | "log";
  key: string[];
  columns: Column[];
};

export type Row = Record<string, unknown> & { _version?: string };

export type RowsPage = {
  rows: Row[];
  count: number | null;
  limit: number;
  offset: number;
};

export type Change = {
  version: string;
  seq: number;
  table: string;
  op: "insert" | "update" | "delete";
  key: Record<string, unknown>;
  row: Row | null;
};

export class ApiError extends Error {
  constructor(
    message: string,
    readonly status: number,
    /** Located config diagnostics, for a config with problems. */
    readonly details?: string,
  ) {
    super(message);
  }
}

async function request<T>(url: string, init?: RequestInit): Promise<T> {
  let response: Response;
  try {
    response = await fetch(url, {
      cache: "no-store",
      ...init,
      headers: init?.body ? { "content-type": "application/json" } : undefined,
    });
  } catch {
    throw new ApiError(`Can't reach Nineveh at ${API_URL}`, 0);
  }
  if (!response.ok) {
    const body = (await response.json().catch(() => null)) as { error?: string; details?: string } | null;
    throw new ApiError(body?.error ?? response.statusText, response.status, body?.details);
  }
  if (response.status === 204) return undefined as T;
  return (await response.json()) as T;
}

// --- one project's API, under `base` --------------------------------------------------

export const getStatus = (base: string) => request<Status>(`${base}/v1/status`);
export const getTables = (base: string) => request<Table[]>(`${base}/v1/tables`);

export type RowsQuery = {
  limit: number;
  offset: number;
  order?: { column: string; desc: boolean };
  filters: Record<string, string>;
  count?: boolean;
};

export function rowsPath(table: string, query: RowsQuery): string {
  const params = new URLSearchParams();
  params.set("limit", String(query.limit));
  if (query.offset) params.set("offset", String(query.offset));
  if (query.order) params.set("order", `${query.order.column}${query.order.desc ? ".desc" : ""}`);
  for (const [column, value] of Object.entries(query.filters)) {
    if (value !== "") params.set(column, value);
  }
  if (query.count) params.set("count", "exact");
  return `/v1/tables/${encodeURIComponent(table)}?${params}`;
}

export const getRows = (base: string, table: string, query: RowsQuery) =>
  request<RowsPage>(`${base}${rowsPath(table, query)}`);

export const getPath = (base: string, path: string) => request<unknown>(`${base}${path}`);

/** The identity of a row: its key columns' values. */
export function rowKey(table: Table, row: Record<string, unknown>): string {
  return JSON.stringify(table.key.map((column) => row[column] ?? null));
}

// --- the control API ------------------------------------------------------------------

export type Network = "mainnet" | "testnet" | "devnet";

export type ProjectSummary = {
  name: string;
  network: Network;
  /** Whether it should run. */
  running: boolean;
  state: "starting" | "running" | "retrying" | "stopped" | "halted" | "failed" | string;
  error: string | null;
  pipeline: Health | null;
  /** The project's API, relative to the control plane. */
  api: string;
  created_at: string;
  updated_at: string;
};

export type ProjectDetail = ProjectSummary & { config: string };

export type CatalogItem = {
  kind: "event" | "resource" | "table";
  /** What the config names. */
  id: string;
  module: string;
  name: string;
  suggested_name: string;
  fields: { name: string; type: string }[];
  /** An enum's variants, oldest first. Its table has a column per field of any variant. */
  variants: string[];
  generic: boolean;
  /** Why it can't be followed yet. */
  unsupported: string | null;
};

export type Catalog = { address: string; modules: string[]; items: CatalogItem[] };

export type Start = "auto" | "now";

const CONTROL = `${API_URL}/control/v1`;
const json = (body: unknown) => ({ body: JSON.stringify(body) });

export const control = {
  projects: () => request<ProjectSummary[]>(`${CONTROL}/projects`),
  project: (name: string) => request<ProjectDetail>(`${CONTROL}/projects/${encodeURIComponent(name)}`),
  inspect: (network: Network, address: string) =>
    request<Catalog>(`${CONTROL}/inspect?${new URLSearchParams({ network, address })}`),
  scaffold: (draft: { name: string; network: Network; start: Start; picks: string[] }) =>
    request<{ config: string }>(`${CONTROL}/scaffold`, { method: "POST", ...json(draft) }),
  create: (config: string) =>
    request<ProjectDetail>(`${CONTROL}/projects`, { method: "POST", ...json({ config }) }),
  update: (name: string, config: string) =>
    request<ProjectDetail>(`${CONTROL}/projects/${encodeURIComponent(name)}`, {
      method: "PUT",
      ...json({ config }),
    }),
  start: (name: string) =>
    request<ProjectSummary>(`${CONTROL}/projects/${encodeURIComponent(name)}/start`, { method: "POST" }),
  stop: (name: string) =>
    request<ProjectSummary>(`${CONTROL}/projects/${encodeURIComponent(name)}/stop`, { method: "POST" }),
  remove: (name: string) =>
    request<void>(`${CONTROL}/projects/${encodeURIComponent(name)}`, { method: "DELETE" }),
};

/** A project's API base URL under the control plane. */
export const projectBase = (name: string) => `${API_URL}/projects/${encodeURIComponent(name)}`;
