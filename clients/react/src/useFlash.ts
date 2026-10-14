import { useEffect, useMemo, useRef, useState } from 'react'

import type { AnyRow } from './schema.ts'

/**
 * The rows whose `_version` just moved, for a moment.
 *
 * A page that claims to be live should look it. `_version` is on every row and says
 * which transaction last wrote it, so a row that has moved since the previous read is
 * the row to light up — no diffing of values, and nothing to get wrong when a number
 * happens to repeat.
 *
 * `key` is the table's key columns, which `GET /v1/tables` reports in key order.
 */
export function useFlash(
  rows: readonly AnyRow[] | undefined,
  key: string | readonly string[],
  ms = 1100,
): Set<string> {
  const seen = useRef(new Map<string, string>())
  const [flashing, setFlashing] = useState<Set<string>>(new Set())
  // The key columns as one string, so the effect below depends on what they are rather
  // than on the identity of the array holding them. `useFlash(rows, ['seller'])` builds
  // a fresh array every render; depending on it, the effect re-ran on the render that
  // `setFlashing` itself caused, and its cleanup cancelled the timer that would have
  // cleared the flash — so every row that ever moved stayed lit for good.
  const columns = useMemo(
    () => (typeof key === 'string' ? [key] : [...key]),
    [typeof key === 'string' ? key : key.join('\u0000')],
  )

  useEffect(() => {
    if (rows === undefined) return
    const moved = new Set<string>()
    for (const row of rows) {
      const id = rowKey(row, columns)
      const version = String(row._version)
      const before = seen.current.get(id)
      if (before !== undefined && before !== version) moved.add(id)
      seen.current.set(id, version)
    }
    if (moved.size === 0) return
    setFlashing(moved)
    const done = setTimeout(() => setFlashing(new Set()), ms)
    return () => clearTimeout(done)
  }, [rows, columns, ms])

  return flashing
}

/** A row's identity, the same way `rowKey` builds it for a change. */
export function rowKey(row: Record<string, unknown>, key: string | readonly string[]): string {
  const columns = typeof key === 'string' ? [key] : key
  return columns.map((column) => String(row[column])).join('\u0000')
}
