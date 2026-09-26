// Nineveh's HTTP APIs, as `nineveh up` serves them: the control API at
// `/control/v1`, and each project's state API and change feed at `/projects/{name}`.
// Hosted, every request carries the signed-in session (ADR 0018).
// `nineveh run --serve` and `nineveh serve` serve one project's API at the root, with no
// control API; Studio works against either.
//
// Wide integers (u64 and up, and versions) arrive as decimal strings so nothing loses
// precision in JavaScript (ADR 0008). Keep them as strings; format them with BigInt.

import { session, signedOut } from "./session";

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

/** Headers every request sends: the session, if signed in. */
export function authHeaders(): Record<string, string> {
  const token = session();
  return token ? { authorization: `Bearer ${token}` } : {};
}

async function request<T>(url: string, init?: RequestInit): Promise<T> {
  let response: Response;
  try {
    response = await fetch(url, {
      cache: "no-store",
      ...init,
      headers: {
        ...authHeaders(),
        ...(init?.body ? { "content-type": "application/json" } : {}),
      },
    });
  } catch {
    throw new ApiError(`Can't reach Nineveh at ${API_URL}`, 0);
  }
  if (response.status === 401 && session()) signedOut();
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
  state: "starting" | "running" | "retrying" | "stopped" | "halted" | "failed" | "idle" | string;
  error: string | null;
  pipeline: Health | null;
  /**
   * Whether it has stopped folding because nothing is reading it (ADR 0023). Not the
   * same as stopped: it is still running, still following the chain and still keeping
   * its records — only the computing of rows nobody asked for has paused, and the next
   * read starts it again.
   */
  idle: boolean;
  /** The project's API, relative to the control plane. */
  api: string;
  created_at: string;
  updated_at: string;
};

export type ProjectDetail = ProjectSummary & {
  config: string;
  /**
   * The project's `.nineveh.ts`, when its config names a `reducers:` file (ADR 0025).
   * Absent for a project written entirely in YAML.
   */
  reducers?: string;
};

/** What a rule on a source can read (ADR 0011). */
export type SourceInfo = {
  name: string;
  kind: "event" | "resource" | "table";
  /** The Move type it follows. */
  follows: string;
  /** Whether `<name>.deleted` rules are possible. */
  deletes: boolean;
  fields: FieldInfo[];
  /** What a `<name>.deleted` rule reads: only what identifies the row. */
  delete_fields: FieldInfo[];
  /** How many records this source has ever matched. Zero is worth asking about. */
  matched: number;
};

export type FieldInfo = { name: string; type: ColumnType; nullable: boolean };

/** What a table's rules would produce, folded over recent transactions (ADR 0017). */
export type Preview = {
  table: string;
  rows: Row[];
  /** How many rows the fold produced; `rows` holds at most the first 50. */
  row_count: number;
  /** Records that reached the project, and transactions read, over the window. */
  records: number;
  transactions: number;
  from: string;
  to: string;
  reached_tip: boolean;
};

/** A webhook endpoint: what the config says, plus how its deliveries are going. */
export type WebhookInfo = {
  name: string;
  url: string;
  /** The changes it asks for, as written: `balances.changed`. */
  on: string[];
  /** Whether deliveries carry the changed row, not only its key. */
  rows: boolean;
  /** The secret every delivery is signed with. */
  secret: string;
  /** The last change delivered, as `version.seq`. */
  delivered: string | null;
  failures: number;
  last_error: string | null;
};

/** A saved state table in the shape the editor edits. */
export type SavedTable = {
  name: string;
  kind: "reduce" | "mirror" | "log";
  columns: { name: string; type: ColumnType; default: string; nullable: boolean; key: boolean }[];
  rules: {
    on: string;
    deleted: boolean;
    when: string;
    keys: { column: string; expression: string }[];
    sets: { column: string; expression: string }[];
    removes: boolean;
  }[];
};

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

export type Me = {
  mode: "local" | "hosted";
  account: { login: string; name: string | null; avatar_url: string | null } | null;
  /**
   * The networks the control plane holds a Geomi key for, so it can actually stream
   * them. Not the same question as the tier's `networks`, which is what an account is
   * allowed to follow, and present in local mode where there is no tier at all.
   */
  networks: string[];
  /** Absent in local mode: there is no account there, so there is no tier. */
  limits?: Limits;
};

/** What an account's tier allows. The numbers live on the server; Studio quotes them. */
export type Limits = {
  name: string;
  projects: number;
  networks: string[];
  look_back_hours: number;
  log_bytes: number;
  history_days: number;
};

/** What a project is using of what it is allowed. */
export type Usage = {
  records: number;
  bytes: number;
  limit_bytes: number;
  /** The earliest version still logged: how far back a rebuild reaches for free. */
  earliest_version: string | null;
  history_days: number;
};

/**
 * A network's shared Transaction Stream reader (ADR 0021). One per network, whatever
 * the number of projects — Geomi caps concurrent streams at 7 on testnet, so this is
 * the plane's scarcest resource.
 */
export type ReaderInfo = {
  network: string;
  position: string | null;
  projects: number;
  slots_free: number;
};

export type ApiKey = {
  id: number;
  label: string;
  /** The key's first characters. */
  prefix: string;
  created_at: string;
  last_used_at: string | null;
  /** The key itself: only in the answer that creates it. */
  key?: string;
};

/** Where signing in starts: the control plane sends the browser on to GitHub. */
export const SIGN_IN_URL = `${API_URL}/auth/github`;

export const control = {
  me: () => request<Me>(`${CONTROL}/me`),
  /** What each network's shared reader is doing. */
  readers: () => request<ReaderInfo[]>(`${CONTROL}/readers`),
  /** What one project is using of its allowance. */
  usage: (name: string) =>
    request<Usage>(`${CONTROL}/projects/${encodeURIComponent(name)}/usage`),
  logout: () => request<void>(`${CONTROL}/logout`, { method: "POST" }),
  sources: (name: string) =>
    request<SourceInfo[]>(`${CONTROL}/projects/${encodeURIComponent(name)}/sources`),
  /** Check a config against the project's pinned layouts, without saving it. */
  check: (name: string, config: string, reducers?: string) =>
    request<{ ok: boolean }>(`${CONTROL}/projects/${encodeURIComponent(name)}/check`, {
      method: "POST",
      ...json({ config, reducers }),
    }),
  /** The rows a table's rules would produce, without saving anything. */
  preview: (name: string, config: string, table: string, reducers?: string) =>
    request<Preview>(`${CONTROL}/projects/${encodeURIComponent(name)}/preview`, {
      method: "POST",
      ...json({ config, table, reducers }),
    }),
  /** A saved state table, to open in the editor. */
  stateTable: (name: string, table: string) =>
    request<SavedTable>(
      `${CONTROL}/projects/${encodeURIComponent(name)}/state/${encodeURIComponent(table)}`,
    ),
  /** This project's webhook endpoints, with their secrets and delivery health. */
  webhooks: (name: string) =>
    request<WebhookInfo[]>(`${CONTROL}/projects/${encodeURIComponent(name)}/webhooks`),
  rotateWebhook: (name: string, endpoint: string) =>
    request<{ secret: string }>(
      `${CONTROL}/projects/${encodeURIComponent(name)}/webhooks/${encodeURIComponent(endpoint)}/rotate`,
      { method: "POST" },
    ),
  keys: (name: string) => request<ApiKey[]>(`${CONTROL}/projects/${encodeURIComponent(name)}/keys`),
  createKey: (name: string, label: string) =>
    request<ApiKey>(`${CONTROL}/projects/${encodeURIComponent(name)}/keys`, {
      method: "POST",
      ...json({ label }),
    }),
  revokeKey: (name: string, id: number) =>
    request<void>(`${CONTROL}/projects/${encodeURIComponent(name)}/keys/${id}`, { method: "DELETE" }),
  projects: () => request<ProjectSummary[]>(`${CONTROL}/projects`),
  project: (name: string) => request<ProjectDetail>(`${CONTROL}/projects/${encodeURIComponent(name)}`),
  inspect: (network: Network, address: string) =>
    request<Catalog>(`${CONTROL}/inspect?${new URLSearchParams({ network, address })}`),
  scaffold: (draft: { name: string; network: Network; start: Start; picks: string[] }) =>
    request<{ config: string; reducers?: string }>(`${CONTROL}/scaffold`, {
      method: "POST",
      ...json(draft),
    }),
  create: (config: string, reducers?: string) =>
    request<ProjectDetail>(`${CONTROL}/projects`, {
      method: "POST",
      ...json({ config, reducers }),
    }),
  update: (name: string, config: string, reducers?: string) =>
    request<ProjectDetail>(`${CONTROL}/projects/${encodeURIComponent(name)}`, {
      method: "PUT",
      ...json({ config, reducers }),
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
