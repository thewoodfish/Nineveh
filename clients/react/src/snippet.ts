import { MAX_LIMIT } from './query.ts'
import type { Comparison, Filter, Row, Scalar, TableQuery } from './types.ts'

/**
 * A query string, back as a `TableQuery` — the inverse of `queryString`.
 *
 * For turning a request someone built somewhere else, in Studio's API console or a
 * `curl` they already have, into the call that makes it from an app. Keeping both
 * directions in one place is the point: the query shape has changed twice since this
 * package existed, and anything that writes a `useTable` call from the outside drifts
 * the moment it does.
 */
export function parseQuery(search: string): TableQuery<Row> {
  const params = new URLSearchParams(search.replace(/^[?]/, ''))
  const query: TableQuery<Row> = {}
  const where: Record<string, Filter> = {}

  for (const [key, value] of params) {
    if (key === 'limit' || key === 'offset') {
      const n = Number(value)
      if (Number.isInteger(n) && n >= 0) query[key] = Math.min(n, MAX_LIMIT)
      continue
    }
    if (key === 'order') {
      const terms = value.split(',').map((term) => term.trim()).filter((term) => term !== '')
      const [only] = terms
      if (terms.length === 1 && only !== undefined) query.order = only
      else if (terms.length > 1) query.order = terms
      continue
    }
    if (key === 'count') {
      if (value === 'exact') query.count = true
      continue
    }
    // Anything else is a column, with `.op` on the name saying how to compare.
    const dot = key.lastIndexOf('.')
    const op = dot > 0 ? key.slice(dot + 1) : ''
    if (OPERATORS.includes(op)) {
      const column = key.slice(0, dot)
      const existing = where[column]
      const comparison: Comparison =
        existing !== undefined && typeof existing === 'object' ? existing : {}
      where[column] = { ...comparison, [op]: value }
    } else {
      where[key] = value
    }
  }

  if (Object.keys(where).length > 0) query.where = where
  return query
}

const OPERATORS = ['eq', 'neq', 'gt', 'gte', 'lt', 'lte']

export type SnippetOptions = {
  /** `useTable` by default; `useLiveTable` for the version that keeps itself current. */
  hook?: 'useTable' | 'useLiveTable'
  /** Spaces per level. Two, like the rest of the ecosystem. */
  indent?: number
}

/**
 * The hook call that makes this request, as source to paste.
 *
 * Values stay strings even when they look like numbers, which is deliberate rather than
 * lazy: a `u64` column's values arrive as decimal strings, and a snippet that quietly
 * dropped the quotes would be teaching the one mistake this package exists to prevent.
 */
export function snippet(table: string, search = '', options?: SnippetOptions): string {
  const hook = options?.hook ?? 'useTable'
  const pad = ' '.repeat(options?.indent ?? 2)
  const query = parseQuery(search)
  const result = hook === 'useTable' ? '{ data, loading, error }' : '{ data, connected }'
  const lines: string[] = []

  if (query.where !== undefined) lines.push(`where: ${object(query.where)},`)
  if (query.order !== undefined) {
    const order = query.order
    lines.push(`order: ${typeof order === 'string' ? quote(order) : list(order)},`)
  }
  if (query.limit !== undefined) lines.push(`limit: ${query.limit},`)
  if (query.offset !== undefined) lines.push(`offset: ${query.offset},`)
  if (query.count === true) lines.push('count: true,')

  const call =
    lines.length === 0
      ? `${hook}(${quote(table)})`
      : `${hook}(${quote(table)}, {\n${lines.map((line) => pad + line).join('\n')}\n})`
  return `const ${result} = ${call}`
}

/** `{ a: 1, b: { gte: '2' } }` on one line: these are short, and a wrapped one reads worse. */
function object(where: Record<string, Filter | undefined>): string {
  const parts = Object.entries(where).map(([column, filter]) => {
    if (filter === undefined) return `${member(column)}: undefined`
    if (filter === null || typeof filter !== 'object') return `${member(column)}: ${value(filter)}`
    const comparisons = Object.entries(filter)
      .filter(([, against]) => against !== undefined)
      .map(([op, against]) => `${op}: ${value(against as Scalar)}`)
    return `${member(column)}: { ${comparisons.join(', ')} }`
  })
  return `{ ${parts.join(', ')} }`
}

function list(terms: readonly string[]): string {
  return `[${terms.map(quote).join(', ')}]`
}

function value(scalar: Scalar): string {
  return typeof scalar === 'string' ? quote(scalar) : String(scalar)
}

/** Single quotes, which is what the examples and most code around this use. */
function quote(text: string): string {
  return `'${text.replace(/\\/g, '\\\\').replace(/'/g, "\\'")}'`
}

/** A key that needs quoting, quoted. Column names never do, but this is given a string. */
function member(name: string): string {
  return /^[A-Za-z_$][A-Za-z0-9_$]*$/.test(name) ? name : quote(name)
}
