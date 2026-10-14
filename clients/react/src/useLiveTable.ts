import { useEffect, useRef, useState } from 'react'

import { queryString } from './query.ts'
import { useNinevehClient } from './provider.ts'
import type { TableResult } from './useTable.ts'
import { useRows } from './useTable.ts'
import type { Change, Row, TableQuery } from './types.ts'

export type LiveOptions = {
  /** How long to gather changes before re-reading. Defaults to 400ms. */
  debounceMs?: number
  /**
   * Re-read on a timer as well, in milliseconds. Off by default: the feed reports a
   * rebuild as `reset` and a dropped connection as a reopen, and this hook reads again
   * on both, so there is nothing left for a poll to catch.
   */
  poll?: number
}

export type LiveResult<T extends Row = Row> = TableResult<T> & {
  /** Whether the change feed is open, so a UI can show a "live" dot honestly. */
  connected: boolean
  /** The last change seen for this table, for an activity view. */
  lastChange: Change | null
}

/**
 * One table's rows, kept current — "tell me when they change".
 *
 * The feed is used as the signal that something moved, not as the data: on a change
 * the rows are read again. That is simpler and always exactly right, where merging the
 * change into the rows locally is not — a change can reorder rows or push one out of a
 * `limit: 10` window, and only the server knows which. Merging is an optimization to
 * add behind an option later, not the baseline.
 */
export function useLiveTable<T extends Row = Row>(
  table: string,
  query?: TableQuery,
  options?: LiveOptions,
): LiveResult<T> {
  const client = useNinevehClient()
  const { data, loading, error, count, refetch, reload } = useRows<T>(
    client,
    table,
    queryString(query),
  )
  const [connected, setConnected] = useState(false)
  const [lastChange, setLastChange] = useState<Change | null>(null)
  const opened = useRef(false)
  const debounceMs = options?.debounceMs ?? 400
  const poll = options?.poll ?? 0

  useEffect(() => {
    let timer: ReturnType<typeof setTimeout> | undefined
    // Whether this subscription has had a connection before, which makes the next one
    // a reconnection rather than the first.
    opened.current = false
    const unsubscribe = client.subscribe({
      tables: [table],
      onOpen: () => {
        setConnected(true)
        // A connection that dropped and came back may have missed changes while it was
        // away. Inside the feed's retention the server replays them from the position
        // the browser resumes with; past it — a tab asleep for longer than a week —
        // they have been pruned and no replay is coming, and nothing in the feed says
        // so. Reading the table again is the one answer that is right either way, and
        // it costs one query per reconnection.
        //
        // Not on the first open: the read that mounted this hook is newer than it.
        if (opened.current) reload()
        opened.current = true
      },
      onError: () => setConnected(false),
      onChange: (change: Change) => {
        setLastChange(change)
        clearTimeout(timer)
        timer = setTimeout(reload, debounceMs)
      },
      // A rebuild was swapped in, so every row listed came from the old build. Read
      // again now rather than on the debounce: this is a correctness reload, not a
      // refresh, and without it the rows go quietly stale forever.
      onReset: () => {
        clearTimeout(timer)
        reload()
      },
    })
    return () => {
      clearTimeout(timer)
      unsubscribe()
    }
  }, [client, table, debounceMs, reload])

  useEffect(() => {
    if (poll <= 0) return
    const every = setInterval(reload, poll)
    return () => clearInterval(every)
  }, [poll, reload])

  return { data, loading, error, count, refetch, connected, lastChange }
}
