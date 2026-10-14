/**
 * A stand-in for `EventSource`, because the real one needs a server.
 *
 * Only what the client uses: a URL, listeners, and `close`. The cast at the bottom is
 * the same one the fetch fakes make — the client's option is typed as the real
 * constructor so that passing the real one stays the obvious thing to do.
 */
export class FakeFeed {
  /** Every connection opened, in order, so a test can assert there was only one. */
  static readonly opened: FakeFeed[] = []

  readonly url: string
  closed = false
  private readonly listeners = new Map<string, ((event: MessageEvent<string>) => void)[]>()

  constructor(url: string) {
    this.url = url
    FakeFeed.opened.push(this)
  }

  addEventListener(type: string, listener: (event: MessageEvent<string>) => void): void {
    const existing = this.listeners.get(type)
    if (existing === undefined) this.listeners.set(type, [listener])
    else existing.push(listener)
  }

  close(): void {
    this.closed = true
  }

  /** What the server sends: an event name, and for a change its JSON. */
  emit(type: string, data = ''): void {
    for (const listener of this.listeners.get(type) ?? []) {
      listener({ data } as MessageEvent<string>)
    }
  }

  /** The query the connection was opened with. */
  get params(): URLSearchParams {
    return new URL(this.url).searchParams
  }

  static reset(): void {
    FakeFeed.opened.length = 0
  }

  /** The only connection open, or a failure naming how many there were. */
  static only(): FakeFeed {
    if (FakeFeed.opened.length !== 1) {
      throw new Error(`expected one connection, got ${FakeFeed.opened.length}`)
    }
    return FakeFeed.opened[0] as FakeFeed
  }
}

export const feedSource = FakeFeed as unknown as typeof globalThis.EventSource

/**
 * Let the client's deferred connect run.
 *
 * `reconcile` opens on a microtask so that a page mounting four hooks opens one
 * connection; a test has to cross that boundary before asserting anything about it.
 */
export function tick(): Promise<void> {
  return new Promise((resolve) => setTimeout(resolve, 0))
}
