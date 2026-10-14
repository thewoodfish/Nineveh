/**
 * React hooks for a Nineveh project.
 *
 * A thin, typed wrapper over the project's REST API and change feed — no new backend
 * capability, just the common things made trivial. `useTable` for "how are things",
 * `useLiveTable` for "tell me when they change".
 *
 * The API reference this wraps: https://nineveh.dev/docs/reading
 */

export { NinevehClient, NinevehError } from './client.ts'
export type { ClientOptions, Subscription } from './client.ts'
export { NinevehProvider, useNinevehClient } from './provider.ts'
export type { NinevehProviderProps } from './provider.ts'
export { declaration, tsType } from './declare.ts'
export { MAX_LIMIT, queryString } from './query.ts'
export { parseQuery, snippet } from './snippet.ts'
export type { SnippetOptions } from './snippet.ts'
export type { AnyRow, Columns, OrderBy, RowOf, TableName } from './schema.ts'

/**
 * Your project's tables, once you have generated them.
 *
 * Empty here, and filled in by the declaration file `nineveh-types` writes, which
 * augments it with one member per state table:
 *
 * ```ts
 * declare module '@nineveh/react' {
 *   interface NinevehTables {
 *     sellers: { seller: string; revenue: string; sold: string; _version: string }
 *   }
 * }
 * ```
 *
 * With that file anywhere in the project, `useTable('sellers')` knows its columns and
 * their types, and a name that is not a table is a type error. Without it everything
 * still works and rows are `Row` — generated types sharpen, they are never required.
 *
 * It is declared *here*, in the entry point, and not beside the types that use it: an
 * augmentation of `'@nineveh/react'` can only merge into an interface this module
 * declares itself. Re-exporting one from elsewhere would make `declare module` quietly
 * create a second, unrelated interface, and nothing would sharpen.
 */
// The empty interface is the point: it exists to be augmented.
export interface NinevehTables {}
export { useTable } from './useTable.ts'
export type { TableResult } from './useTable.ts'
export { useLiveTable } from './useLiveTable.ts'
export type { LiveOptions, LiveResult } from './useLiveTable.ts'
export { useChanges } from './useChanges.ts'
export type { ChangesOptions, ChangesResult } from './useChanges.ts'
export { useFeedStart } from './useFeedStart.ts'
export { useStatus } from './useStatus.ts'
export type { StatusOptions, StatusResult } from './useStatus.ts'
export { rowKey, useFlash } from './useFlash.ts'
export type {
  Build,
  Change,
  ChangeOp,
  ColumnInfo,
  Comparison,
  Filter,
  Page,
  Pipeline,
  Row,
  Scalar,
  Status,
  TableInfo,
  TableKind,
  TableQuery,
} from './types.ts'
