import { queryString } from './query.ts'
import type { AnyRow, RowOf, TableName } from './schema.ts'
import type { Change, Page, Row, Status, TableInfo, TableQuery } from './types.ts'

export type ClientOptions = {
  /** A project's base URL, e.g. `https://api.nineveh.dev/projects/myapp`. */
  project: string
  /**
   * A project API key. Required against a hosted plane; a locally-run plane in local
   * mode serves loopback without one.
   *
   * It reaches the browser, and that is intended: reads only ever touch the state
   * tables in Postgres, never the chain, so an exposed key costs stream credit
   * nothing. Give a frontend its own key and revoke it when it's done.
   */
  apiKey?: string
  /** For tests and server rendering. Defaults to the global `fetch`. */
  fetch?: typeof globalThis.fetch
  /**
   * The change feed's transport. Defaults to the global `EventSource`.
   *
   * Injectable for the same reason `fetch` is, plus one of its own: `EventSource`
   * cannot send headers, so a caller who needs the key in one — behind a proxy that
   * rejects it on the URL — can pass a polyfill that does.
   */
  eventSource?: typeof globalThis.EventSource
}

/** A refused or failed read. `status` is the HTTP status, 0 if the request never landed. */
export class NinevehError extends Error {
  readonly status: number

  constructor(message: string, status: number) {
    super(message)
    this.name = 'NinevehError'
    this.status = status
  }

  /**
   * Worth trying again.
   *
   * 503 is the one that matters: `"the served build doesn't match the config yet"` is
   * what a config change looks like while the rebuild is swapped in. It is
   * expected and brief, and pairs with `reset` on the change feed.
   */
  get retryable(): boolean {
    return this.status === 503 || this.status === 429 || this.status >= 500 || this.status === 0
  }
}

export type Subscription = {
  /** Which tables to hear about. Every table when absent. */
  tables?: readonly string[]
  /**
   * Where to start: a `version.seq` position, or `'beginning'` for the whole stored
   * feed. The newest change when absent, which is what a live table wants.
   *
   * Changes are stored as they commit rather than only broadcast, so a feed can open
   * in the past. That is what makes an activity view arrive full instead of empty —
   * and `'beginning'` grows without bound, so a position is usually the better ask.
   */
  after?: string
  onChange?: (change: Change) => void
  /**
   * The tables were rebuilt and a new build was swapped in. Everything listed before
   * this came from the old build: re-read it. A client that ignores `reset` shows
   * stale rows forever, with no error.
   */
  onReset?: () => void
  onOpen?: () => void
  onError?: () => void
}

/** How many times a retryable read is tried, and the backoff between tries. */
const ATTEMPTS = 4
const BACKOFF_MS = 250

/**
 * One project's API: reads over its state tables, and one shared change feed.
 *
 * The feed is shared because the hooks are not: a page with four live tables on it
 * should hold one connection, and the server already takes `?tables=a,b`.
 */
export class NinevehClient {
  readonly project: string
  private readonly apiKey: string | undefined
  private readonly fetch: typeof globalThis.fetch
  private readonly eventSource: typeof globalThis.EventSource | undefined
  /** Memoized: table shapes change only when a rebuild swaps in, which resets us. */
  private schema: Promise<TableInfo[]> | undefined
  private readonly subscribers = new Set<Subscription>()
  private source: EventSource | undefined
  /** The tables the open connection asked for; undefined means all of them. */
  private open: ReadonlySet<string> | undefined
  /** Where the open connection started, so a subscriber wanting more history reopens. */
  private openedAt: string | undefined
  /** The last change delivered, as `version.seq`: where to resume from on a reopen. */
  private position: string | undefined
  private reopening = false

  constructor(options: ClientOptions) {
    this.project = options.project.replace(/\/+$/, '')
    this.apiKey = options.apiKey
    this.fetch = options.fetch ?? globalThis.fetch.bind(globalThis)
    this.eventSource = options.eventSource ?? globalThis.EventSource
  }

  /** `GET /v1/tables/:table` — one page of rows. */
  rows<T extends TableName>(
    table: T,
    query?: TableQuery<RowOf<T>>,
    signal?: AbortSignal,
  ): Promise<Page<RowOf<T>>> {
    return this.page<RowOf<T>>(table, queryString(query), signal)
  }

  /**
   * A read with the query already serialized, which is what the hooks hold: the string
   * is both the request and the key they re-read on.
   */
  async page<T extends AnyRow = Row>(
    table: string,
    search: string,
    signal?: AbortSignal,
  ): Promise<Page<T>> {
    const path = `/v1/tables/${encodeURIComponent(table)}${search === '' ? '' : `?${search}`}`
    const body = await this.read<{ rows?: T[]; count: number | null; limit: number; offset: number }>(
      path,
      signal,
    )
    const page: Page<T> = { rows: body.rows ?? [], limit: body.limit, offset: body.offset }
    if (body.count !== null) page.count = body.count
    return page
  }

  /** `GET /v1/tables` — every table's kind, key and columns. Read once and kept. */
  tables(signal?: AbortSignal): Promise<TableInfo[]> {
    this.schema ??= this.read<TableInfo[]>('/v1/tables', signal).catch((error: unknown) => {
      // A failed read must not be remembered as the answer.
      this.schema = undefined
      throw error
    })
    return this.schema
  }

  /** `GET /v1/status` — the build, any rebuild, and the pipeline's health. */
  status(signal?: AbortSignal): Promise<Status> {
    return this.read<Status>('/v1/status', signal)
  }

  /** Listen to the change feed. Returns the unsubscribe. */
  subscribe(subscription: Subscription): () => void {
    if (this.eventSource === undefined) {
      // Checked here, where the caller is: the connection itself opens a microtask
      // later, and a throw from there would be an unhandled rejection with no stack
      // worth reading. An environment with no `EventSource` is a misconfiguration to
      // fix, not a transient failure to report — so it reads like the provider's.
      throw new Error('no `EventSource` in this environment; pass one as `eventSource`')
    }
    this.subscribers.add(subscription)
    this.reconcile()
    return () => {
      this.subscribers.delete(subscription)
      this.reconcile()
    }
  }

  /** Close the shared connection. Hooks do this for themselves; tests want it too. */
  close(): void {
    this.source?.close()
    this.source = undefined
    this.open = undefined
    this.openedAt = undefined
  }

  private async read<T>(path: string, signal?: AbortSignal): Promise<T> {
    const headers: Record<string, string> = { accept: 'application/json' }
    if (this.apiKey !== undefined) headers.authorization = `Bearer ${this.apiKey}`
    for (let attempt = 1; ; attempt += 1) {
      const error = await this.attempt<T>(`${this.project}${path}`, headers, signal)
      if (!(error instanceof NinevehError)) return error.value
      if (!error.retryable || attempt === ATTEMPTS) throw error
      await delay(BACKOFF_MS * 2 ** (attempt - 1), signal)
    }
  }

  /** One try: the value, or the error to decide about. Aborts are thrown straight out. */
  private async attempt<T>(
    url: string,
    headers: Record<string, string>,
    signal?: AbortSignal,
  ): Promise<{ value: T } | NinevehError> {
    let response: Response
    try {
      response = await this.fetch(url, signal === undefined ? { headers } : { headers, signal })
    } catch (cause: unknown) {
      if (signal?.aborted === true) throw cause
      return new NinevehError(cause instanceof Error ? cause.message : String(cause), 0)
    }
    if (!response.ok) {
      const said = await response
        .json()
        .then((body: unknown) =>
          typeof body === 'object' && body !== null && 'error' in body ? String(body.error) : '',
        )
        .catch(() => '')
      const what = said === '' ? `${response.status} ${response.statusText}` : said
      return new NinevehError(what, response.status)
    }
    return { value: (await response.json()) as T }
  }

  /**
   * Hold the connection the current subscribers need.
   *
   * Opening is deferred a tick so a page that mounts four hooks opens one connection
   * rather than four. A connection is kept while it covers what is wanted: a narrower
   * set of tables than it carries is fine, since each change is dispatched only to the
   * subscribers that asked for its table.
   */
  private reconcile(): void {
    if (this.subscribers.size === 0) {
      this.close()
      return
    }
    if (this.source !== undefined && this.covers()) return
    if (this.reopening) return
    this.reopening = true
    queueMicrotask(() => {
      this.reopening = false
      if (this.subscribers.size === 0 || (this.source !== undefined && this.covers())) {
        return
      }
      this.connect()
    })
  }

  /** Whether the open connection hears everything the subscribers want. */
  private covers(): boolean {
    for (const subscriber of this.subscribers) {
      // A connection that started later than a subscriber wants has already passed the
      // history it is asking for, and a feed cannot be rewound — only reopened.
      if (earlier(this.openedAt, subscriber.after) !== this.openedAt) return false
      if (this.open === undefined) continue
      if (subscriber.tables === undefined) return false
      if (subscriber.tables.some((table) => !this.open?.has(table))) return false
    }
    return true
  }

  private connect(): void {
    this.close()
    const wanted = new Set<string>()
    let all = false
    // Where to open: early enough for every subscriber. One connection serves them
    // all, so it starts at the earliest point any of them asked for, and the ones that
    // only wanted what happens next are handed some history they did not need — which
    // for a live table is a burst of changes that debounces into one re-read.
    let start = this.position
    for (const subscriber of this.subscribers) {
      if (subscriber.tables === undefined) all = true
      else for (const table of subscriber.tables) wanted.add(table)
      start = earlier(start, subscriber.after)
    }
    const params = new URLSearchParams()
    // `EventSource` cannot set headers, so the key goes on the URL — the one place it
    // has to. Reads never touch the chain, which is what makes that acceptable.
    if (this.apiKey !== undefined) params.set('apikey', this.apiKey)
    if (!all && wanted.size > 0) params.set('tables', [...wanted].join(','))
    // Without a start, the newest change: a live table re-reads on any change, so a
    // hook that joins late never needs history — it needs to know that something moved.
    if (start !== undefined) params.set('after', start)

    // Non-null because `subscribe` refuses without it, and nothing connects unsubscribed.
    const Feed = this.eventSource ?? globalThis.EventSource
    const source = new Feed(`${this.project}/v1/changes?${params.toString()}`)
    this.source = source
    this.open = all ? undefined : wanted
    this.openedAt = start
    source.addEventListener('open', () => {
      for (const subscriber of [...this.subscribers]) subscriber.onOpen?.()
    })
    source.addEventListener('error', () => {
      // `EventSource` reconnects by itself, sending `Last-Event-ID`, so this is a
      // report rather than something to handle.
      for (const subscriber of [...this.subscribers]) subscriber.onError?.()
    })
    source.addEventListener('change', (event: MessageEvent<string>) => {
      let change: Change
      try {
        change = JSON.parse(event.data) as Change
      } catch {
        return
      }
      this.position = `${change.version}.${change.seq}`
      for (const subscriber of [...this.subscribers]) {
        if (subscriber.tables === undefined || subscriber.tables.includes(change.table)) {
          subscriber.onChange?.(change)
        }
      }
    })
    source.addEventListener('reset', () => {
      // A new build was swapped in. The old build's positions mean nothing in the new
      // feed, and the table shapes may have changed with the config.
      this.position = undefined
      this.schema = undefined
      for (const subscriber of [...this.subscribers]) subscriber.onReset?.()
    })
  }
}

/**
 * The earlier of two places to open the feed, so one connection can serve both.
 *
 * Three kinds of value, ordered: `'beginning'` is before everything, a `version.seq`
 * position sits where it says, and `undefined` — the newest change — is after
 * everything, which makes it the identity here rather than a missing value.
 */
export function earlier(a: string | undefined, b: string | undefined): string | undefined {
  if (a === undefined) return b
  if (b === undefined) return a
  if (a === BEGINNING || b === BEGINNING) return BEGINNING
  const left = position(a)
  const right = position(b)
  // An unparseable position is handed to the server as it is rather than silently
  // reordered: the server validates it and says what is wrong with it.
  if (left === undefined) return a
  if (right === undefined) return b
  if (left[0] !== right[0]) return left[0] < right[0] ? a : b
  return left[1] <= right[1] ? a : b
}

/** The whole stored feed, which is what the server calls this. */
const BEGINNING = 'beginning'

/** `version.seq` as a pair to compare. Versions are past what a number holds. */
function position(value: string): [bigint, number] | undefined {
  const dot = value.lastIndexOf('.')
  if (dot <= 0) return undefined
  const seq = Number(value.slice(dot + 1))
  if (!Number.isInteger(seq)) return undefined
  try {
    return [BigInt(value.slice(0, dot)), seq]
  } catch {
    return undefined
  }
}

function delay(ms: number, signal?: AbortSignal): Promise<void> {
  return new Promise((resolve, reject) => {
    const timer = setTimeout(() => {
      signal?.removeEventListener('abort', stop)
      resolve()
    }, ms)
    const stop = () => {
      clearTimeout(timer)
      reject(signal?.reason)
    }
    signal?.addEventListener('abort', stop, { once: true })
  })
}
