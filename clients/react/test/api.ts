/**
 * A project's API, answered from memory.
 *
 * Routes the three paths the hooks read, so a test states what the server holds rather
 * than what each request should return. `pages` is consulted per table on every read,
 * so a test can change the rows and then provoke a re-read.
 */
export type Server = {
  rows?: Record<string, Record<string, unknown>[]>
  count?: number
  status?: unknown
  tables?: unknown
  /** Fail every read with this status, for the error paths. */
  fail?: number
}

export type Api = {
  fetch: typeof globalThis.fetch
  /** Every URL read, in order. */
  readonly reads: string[]
  /** Reads of one table, which is what "did it re-read?" means. */
  readsOf: (table: string) => number
  server: Server
}

export function api(server: Server): Api {
  const reads: string[] = []
  const json = (body: unknown, status = 200) =>
    new Response(JSON.stringify(body), {
      status,
      headers: { 'content-type': 'application/json' },
    })

  const fetch = ((url: string | URL | Request) => {
    const path = new URL(String(url)).pathname
    reads.push(String(url))
    if (server.fail !== undefined) {
      return Promise.resolve(json({ error: 'nope' }, server.fail))
    }
    if (path.endsWith('/v1/status')) return Promise.resolve(json(server.status ?? {}))
    if (path.endsWith('/v1/tables')) return Promise.resolve(json(server.tables ?? []))
    const table = path.slice(path.lastIndexOf('/') + 1)
    const rows = server.rows?.[table] ?? []
    return Promise.resolve(
      json({ rows, count: server.count ?? null, limit: 50, offset: 0 }),
    )
  }) as typeof globalThis.fetch

  return {
    fetch,
    reads,
    readsOf: (table) => reads.filter((url) => url.includes(`/v1/tables/${table}`)).length,
    server,
  }
}
