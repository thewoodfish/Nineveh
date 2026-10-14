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
  for (const [column, filter] of Object.entries(query.where ?? {}).sort(byKey)) {
    if (filter === null || typeof filter !== 'object') {
      // A bare value is equality, and goes as the plain parameter the API started with.
      params.set(column, String(filter))
      continue
    }
    // The operator rides on the parameter's name, never in front of the value, so a
    // string value that happens to read like `gt.5` is still just that string.
    for (const [op, value] of Object.entries(filter).sort(byKey)) {
      if (value !== undefined) params.set(`${column}.${op}`, String(value))
    }
  }
  if (query.order !== undefined) {
    params.set('order', typeof query.order === 'string' ? query.order : query.order.join(','))
  }
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
