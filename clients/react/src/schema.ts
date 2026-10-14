import type { NinevehTables } from './index.ts'
import type { Row } from './types.ts'

/**
 * A table's name: one of yours once the declarations exist, any string before that.
 *
 * `keyof` an un-augmented interface is `never`, which would make every call an error,
 * so that case widens back to `string`.
 */
export type TableName = keyof NinevehTables extends never ? string : Extract<keyof NinevehTables, string>

/** The row shape of the table called `T`, or the open `Row` if it isn't a known one. */
export type RowOf<T extends string> = T extends keyof NinevehTables
  ? NinevehTables[T] extends AnyRow
    ? NinevehTables[T]
    : Row
  : Row

/**
 * The least a row is: the version that last wrote it.
 *
 * Generated rows list their columns exactly and have no index signature, so they are
 * not assignable to `Row`. This is what the generic hooks are bounded by instead.
 */
export type AnyRow = { _version: string }

/** A column name of `R`. For the open `Row` this is `string`, as it was before. */
export type Columns<R> = Extract<keyof R, string>

/** A column, optionally with a direction: `revenue`, `revenue.asc`, `revenue.desc`. */
export type OrderBy<R> = Columns<R> | `${Columns<R>}.asc` | `${Columns<R>}.desc`
