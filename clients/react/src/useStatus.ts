import { useEffect, useReducer, useState } from 'react'

import { useNinevehClient } from './provider.ts'
import type { Status } from './types.ts'

export type StatusOptions = {
  /** How often to re-read, in milliseconds. Defaults to 5000. */
  interval?: number
}

export type StatusResult = {
  status: Status | undefined
  loading: boolean
  error: Error | null
  /** Versions between the committed cursor and the chain's head, as a decimal string. */
  behind: string | undefined
  /** Whether the cursor has caught up with the head. */
  synced: boolean
  /** A backfill's or rebuild's progress, 0 to 1, when there is one to measure. */
  progress: number | undefined
  refetch: () => void
}

/**
 * The project's build, any rebuild in progress, and how far behind the chain it is.
 *
 * The "synced / N behind" badge every dashboard wants. Polled rather than streamed:
 * the change feed carries row changes, not cursor movement.
 */
export function useStatus(options?: StatusOptions): StatusResult {
  const client = useNinevehClient()
  const interval = options?.interval ?? 5000
  const [nonce, refetch] = useReducer((n: number) => n + 1, 0)
  const [state, setState] = useState<{
    status: Status | undefined
    loading: boolean
    error: Error | null
  }>({ status: undefined, loading: true, error: null })

  useEffect(() => {
    const controller = new AbortController()
    const read = () => {
      client
        .status(controller.signal)
        .then((status: Status) => {
          if (controller.signal.aborted) return
          setState({ status, loading: false, error: null })
        })
        .catch((error: unknown) => {
          if (controller.signal.aborted) return
          setState((before) => ({
            ...before,
            loading: false,
            error: error instanceof Error ? error : new Error(String(error)),
          }))
        })
    }
    read()
    const every = interval > 0 ? setInterval(read, interval) : undefined
    return () => {
      clearInterval(every)
      controller.abort()
    }
  }, [client, interval, nonce])

  const pipeline = state.status?.pipeline ?? null
  const behind = gap(pipeline?.cursor, pipeline?.chain_head ?? pipeline?.chain_version)
  const result: StatusResult = {
    status: state.status,
    loading: state.loading,
    error: state.error,
    behind,
    synced: behind === '0',
    progress: fraction(pipeline?.start_version, pipeline?.cursor, pipeline?.chain_version),
    refetch,
  }
  return result
}

/** `head - cursor`, never negative, as a decimal string. Undefined if either is unknown. */
function gap(cursor: string | null | undefined, head: string | null | undefined): string | undefined {
  const from = big(cursor)
  const to = big(head)
  if (from === undefined || to === undefined) return undefined
  return (to > from ? to - from : 0n).toString()
}

/** How far a run has come from its start towards the head it is working to, 0 to 1. */
function fraction(
  start: string | null | undefined,
  cursor: string | null | undefined,
  target: string | null | undefined,
): number | undefined {
  const from = big(start)
  const at = big(cursor)
  const to = big(target)
  if (from === undefined || at === undefined || to === undefined || to <= from) return undefined
  const done = at <= from ? 0n : at >= to ? to - from : at - from
  // Scaled integer division, because the versions are far past what a double holds
  // exactly: parts per ten thousand is more resolution than a progress bar can show.
  return Number((done * 10_000n) / (to - from)) / 10_000
}

function big(value: string | null | undefined): bigint | undefined {
  if (value === null || value === undefined) return undefined
  try {
    return BigInt(value)
  } catch {
    return undefined
  }
}
