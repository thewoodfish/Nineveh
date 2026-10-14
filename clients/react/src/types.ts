/**
 * The shapes a project's API serves.
 *
 * Values arrive exactly as the API renders them, and nothing here
 * converts them: integers up to 32 bits are JSON numbers, wider ones are decimal
 * strings. `Number()` over a `u64` loses precision silently above 2^53, which is why
 * the wide ones are strings in the first place — parse them with `BigInt` if you need
 * arithmetic, and keep them as strings if you only need to display them.
 */

/** A state row: its declared columns, plus `_version`. */
export type Row = {
  /** The transaction that last wrote this row, as a decimal string. */
  _version: string
  [column: string]: unknown
}

/** A page of rows, as `GET /v1/tables/:name` returns it. */
export type Page<T extends Row = Row> = {
  rows: T[]
  /** Only when `count: true` was asked for. */
  count?: number
  limit: number
  offset: number
}

/** How a table is written: a reducer's fold, a mirror of chain state, or a log. */
export type TableKind = 'reduce' | 'mirror' | 'log'

export type ColumnInfo = {
  name: string
  /** The config's type name: `u64`, `address`, `json`, … */
  type: string
  nullable: boolean
}

/** A table's shape, from `GET /v1/tables`. */
export type TableInfo = {
  name: string
  kind: TableKind
  /** The key columns, in key order. A listed row does not carry its key; this is it. */
  key: string[]
  columns: ColumnInfo[]
  /** Only when `?counts=true` was asked for. */
  rows?: { count: number; exact: boolean }
}

export type ChangeOp = 'insert' | 'update' | 'delete'

/** One row change, as the feed sends it. `row` is null for a delete. */
export type Change<T extends Row = Row> = {
  /** Decimal string. */
  version: string
  seq: number
  table: string
  op: ChangeOp
  key: Record<string, unknown>
  row: T | null
}

/** The pipeline's health, as `GET /v1/status` reports it. */
export type Pipeline = {
  /** `starting`, `running`, `retrying`, `halted` or `stopped`. */
  phase: string
  schema: string
  /** The last committed version, as a decimal string. */
  cursor: string | null
  start_version: string | null
  /** The chain's version when this run started: a backfill's denominator. */
  chain_version: string | null
  /** Where the chain is now. */
  chain_head: string | null
  /** Seconds between now and the last committed transaction's block time. */
  lag_secs: number | null
  versions_per_sec: number | null
  retries: number
  last_error: string | null
}

export type Build = {
  cursor: string | null
  fingerprint: string
  created_at: string
  updated_at: string
  /** Present on a rebuild: the schema it is being built in. */
  schema?: string
}

export type Status = {
  project: string
  network: string
  schema: string
  build: Build | null
  /** A rebuild in progress, or null. */
  rebuild: Build | null
  /** Null when the API serves a project whose pipeline runs elsewhere. */
  pipeline: Pipeline | null
}

/** What a read asks of a table. */
export type TableQuery = {
  /**
   * Equality filters, AND-ed: `{ seller: '0xabc…' }`.
   *
   * The API has no `gt`/`lt`/`in` — it builds `WHERE col = val AND …` and nothing
   * else. Anything richer is a change to `nineveh-api`, not something to fake here by
   * over-fetching.
   */
  where?: Record<string, string | number | boolean>
  /** One column: `col`, `col.asc` or `col.desc`. `_version` works too. */
  order?: string
  /** Defaults to 50 server-side, clamped to 1000. */
  limit?: number
  offset?: number
  /** Ask for an exact `count(*)` over the filtered set. A second query; opt in. */
  count?: boolean
  /**
   * Appended to the query string verbatim, so the SDK can never be the reason
   * something the API supports is unreachable.
   */
  raw?: string
}
