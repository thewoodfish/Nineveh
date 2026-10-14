import { useEffect, useReducer, useState } from 'react'

import type { NinevehClient } from './client.ts'
import { queryString } from './query.ts'
import { useNinevehClient } from './provider.ts'
import type { Page, Row, TableQuery } from './types.ts'

export type TableResult<T extends Row = Row> = {
  /** Undefined until the first read lands, then the rows. */
  data: T[] | undefined
  loading: boolean
  error: Error | null
  /** Only when `count: true` was asked for. */
  count: number | undefined
  refetch: () => void
}

/**
 * One table's rows — "how are things".
 *
 * Re-reads whenever the table or the query changes, and never lets a slower earlier
 * read overwrite a later one.
 */
export function useTable<T extends Row = Row>(table: string, query?: TableQuery): TableResult<T> {
  const client = useNinevehClient()
  const rows = useRows<T>(client, table, queryString(query))
  return rows
}

/** The read half of both `useTable` and `useLiveTable`, keyed on the serialized query. */
export function useRows<T extends Row = Row>(
  client: NinevehClient,
  table: string,
  search: string,
): TableResult<T> & { reload: () => void } {
  const [nonce, reload] = useReducer((n: number) => n + 1, 0)
  const [state, setState] = useState<{
    data: T[] | undefined
    count: number | undefined
    loading: boolean
    error: Error | null
  }>({ data: undefined, count: undefined, loading: true, error: null })

  useEffect(() => {
    const controller = new AbortController()
    setState((before) => ({ ...before, loading: true }))
    client
      .page<T>(table, search, controller.signal)
      .then((page: Page<T>) => {
        if (controller.signal.aborted) return
        setState({ data: page.rows, count: page.count, loading: false, error: null })
      })
      .catch((error: unknown) => {
        if (controller.signal.aborted) return
        setState((before) => ({
          ...before,
          loading: false,
          error: error instanceof Error ? error : new Error(String(error)),
        }))
      })
    return () => controller.abort()
  }, [client, table, search, nonce])

  return { ...state, refetch: reload, reload }
}
