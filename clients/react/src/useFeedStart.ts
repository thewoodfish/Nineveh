import { useEffect, useState } from 'react'

import type { AnyRow } from './schema.ts'

/**
 * A feed position just before the oldest of these rows, fixed once they arrive.
 *
 * For the activity view beside a table: pass it the rows on screen and hand the result
 * to `useChanges`, and the feed opens far enough back to replay how what you are
 * looking at got there, rather than starting blank and waiting for the chain to move.
 * `'beginning'` would do it too, and would grow without bound; this window is exactly
 * what the page is showing.
 *
 * `null` until the first rows land, which is `useChanges`' signal to hold off — a feed
 * opened at the newest change in the meantime would have to be reopened, and a page
 * would hold two connections in a row to show one list.
 *
 * Fixed once deliberately. The oldest row on screen moves as rows change, and a
 * position recomputed from live rows would reopen the feed every time they did.
 */
export function useFeedStart(...rows: (readonly AnyRow[] | undefined)[]): string | null {
  const [start, setStart] = useState<string | null>(null)
  const oldest = before(rows)

  useEffect(() => {
    if (start === null && oldest !== undefined) setStart(oldest)
  }, [start, oldest])

  return start
}

/** The position one before the oldest `_version` across every row given. */
function before(tables: (readonly AnyRow[] | undefined)[]): string | undefined {
  let min: bigint | undefined
  for (const rows of tables) {
    for (const row of rows ?? []) {
      let version: bigint
      try {
        version = BigInt(row._version)
      } catch {
        continue
      }
      if (min === undefined || version < min) min = version
    }
  }
  if (min === undefined) return undefined
  // `after` is exclusive, so a step back is what includes the change that wrote the
  // oldest row itself — the one that explains the row rather than the next one along.
  return `${min > 0n ? min - 1n : 0n}.0`
}
