import { useEffect, useState } from 'react'

import { useNinevehClient } from './provider.ts'
import type { Change } from './types.ts'

export type ChangesOptions = {
  /** Which tables to hear about. Every table when absent. */
  tables?: readonly string[]
  /**
   * Where to start: a `version.seq` position, or `'beginning'` for the whole stored
   * feed. The newest change when absent, so the list starts empty and fills as things
   * happen.
   *
   * Pass a position to open in the past and have the list arrive full. It is read once,
   * when the subscription opens — a value derived from rows that are themselves moving
   * would otherwise reopen the feed every time they did.
   */
  after?: string
  /** How many to keep, newest first. Defaults to 50. */
  keep?: number
}

export type ChangesResult = {
  /** Newest first, at most `keep` of them. */
  changes: Change[]
  /** Whether the feed is open, so a UI can show a "live" dot honestly. */
  connected: boolean
}

/**
 * The change feed itself, as a list — "what has been happening".
 *
 * The other hooks use the feed as a signal and throw the change away; this one is for
 * the view that shows the changes as the content. Changes are stored as they commit
 * rather than only broadcast, so `after` can open this in the past and the list arrives
 * full instead of empty.
 *
 * Deduplicated by position, because the same change can arrive twice: a dropped
 * connection resumes from what the browser last saw, and a reopen for an unrelated
 * reason may replay from further back than that.
 */
export function useChanges(options?: ChangesOptions): ChangesResult {
  const client = useNinevehClient()
  const [changes, setChanges] = useState<Change[]>([])
  const [connected, setConnected] = useState(false)
  const keep = options?.keep ?? 50
  const after = options?.after
  const tables = options?.tables?.join(',')

  useEffect(() => {
    const names = tables === undefined || tables === '' ? undefined : tables.split(',')
    const unsubscribe = client.subscribe({
      ...(names === undefined ? {} : { tables: names }),
      ...(after === undefined ? {} : { after }),
      onOpen: () => setConnected(true),
      onError: () => setConnected(false),
      onChange: (change: Change) => {
        setChanges((before) => {
          const at = `${change.version}.${change.seq}`
          if (before.some((seen) => `${seen.version}.${seen.seq}` === at)) return before
          return [change, ...before].slice(0, keep)
        })
      },
      // The rows these changes describe were rebuilt, so the changes that produced the
      // old ones are not the history of anything on screen any more.
      onReset: () => setChanges([]),
    })
    return unsubscribe
  }, [client, tables, after, keep])

  return { changes, connected }
}
