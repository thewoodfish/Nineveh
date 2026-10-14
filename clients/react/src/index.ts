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
export { MAX_LIMIT, queryString } from './query.ts'
export { useTable } from './useTable.ts'
export type { TableResult } from './useTable.ts'
export { useLiveTable } from './useLiveTable.ts'
export type { LiveOptions, LiveResult } from './useLiveTable.ts'
export { useChanges } from './useChanges.ts'
export type { ChangesOptions, ChangesResult } from './useChanges.ts'
export { useStatus } from './useStatus.ts'
export type { StatusOptions, StatusResult } from './useStatus.ts'
export { rowKey, useFlash } from './useFlash.ts'
export type {
  Build,
  Change,
  ChangeOp,
  ColumnInfo,
  Page,
  Pipeline,
  Row,
  Status,
  TableInfo,
  TableKind,
  TableQuery,
} from './types.ts'
