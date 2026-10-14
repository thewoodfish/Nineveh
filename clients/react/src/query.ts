import type { TableQuery } from './types.ts'

/** The server's cap. A larger `limit` is clamped there rather than refused. */
export const MAX_LIMIT = 1000

/**
 * `TableQuery` as a query string, in a stable order.
 *
 * Stable because it is also the cache key a hook re-reads on: the same query has to
 * produce the same string, whatever order the object's keys were written in.
 */
export function queryString(query: TableQuery = {}): string {
  const params = new URLSearchParams()
  for (const [column, value] of Object.entries(query.where ?? {}).sort(byKey)) {
    params.set(column, String(value))
  }
  if (query.order !== undefined) params.set('order', query.order)
  if (query.limit !== undefined) params.set('limit', String(Math.min(query.limit, MAX_LIMIT)))
  if (query.offset !== undefined) params.set('offset', String(query.offset))
  if (query.count === true) params.set('count', 'exact')
  const text = params.toString()
  if (query.raw === undefined || query.raw === '') return text
  const raw = query.raw.replace(/^[?&]/, '')
  return text === '' ? raw : `${text}&${raw}`
}

function byKey(a: [string, unknown], b: [string, unknown]): number {
  return a[0] < b[0] ? -1 : a[0] > b[0] ? 1 : 0
}
