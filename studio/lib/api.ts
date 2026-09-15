// The project's API, as `nineveh run --serve` or `nineveh serve` serves it.
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
  ) {
    super(message);
  }
}

async function get<T>(path: string): Promise<T> {
  let response: Response;
  try {
    response = await fetch(`${API_URL}${path}`, { cache: "no-store" });
  } catch {
    throw new ApiError(`Can't reach the Nineveh API at ${API_URL}`, 0);
  }
  if (!response.ok) {
    const body = (await response.json().catch(() => null)) as { error?: string } | null;
    throw new ApiError(body?.error ?? response.statusText, response.status);
  }
  return (await response.json()) as T;
}

export const getStatus = () => get<Status>("/v1/status");
export const getTables = () => get<Table[]>("/v1/tables");

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

export const getRows = (table: string, query: RowsQuery) => get<RowsPage>(rowsPath(table, query));

export const getPath = (path: string) => get<unknown>(path);

/** The identity of a row: its key columns' values. */
export function rowKey(table: Table, row: Record<string, unknown>): string {
  return JSON.stringify(table.key.map((column) => row[column] ?? null));
}
