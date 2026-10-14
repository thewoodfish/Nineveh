import { useFlash, useLiveTable } from '@nineveh/react'

import { Panel, address, credits } from './shared'

/**
 * Every sale as it happens.
 *
 * `sales` is a log table: one row per `Sold` event, never updated, never deleted. So
 * ordering by `_version` descending is the feed, and the newest sale is simply the
 * first row — no separate subscription, no list to splice into.
 *
 * The tick is the point. `useLiveTable` holds the change feed open and re-reads when a
 * row lands, and because every row here is new, every row flashes once.
 */
export function Feed() {
  const result = useLiveTable('sales', { order: '_version.desc', limit: 20 })
  // A log's key is the transaction and the event's place in it, which is what makes
  // two sales in one transaction two rows.
  const flashing = useFlash(result.data, ['version', 'event_index'])

  return (
    <Panel
      title="Sales, newest first"
      note="A log table ordered by version. Rows only ever arrive, so each one flashes once."
      result={result}
      empty="Nothing has sold yet. Run ./play.sh and watch."
    >
      <ul className="feed">
        {result.data?.map((row) => {
          const id = `${String(row.version)}\u0000${String(row.event_index)}`
          return (
            <li key={id} className={flashing.has(id) ? 'flash' : undefined}>
              <span className="item">{String(row.item)}</span>
              <span className="price">{credits(row.price)}</span>
              <span className="muted">
                {address(row.seller)} → {address(row.buyer)}
              </span>
              <span className="muted mono at">@{String(row.version)}</span>
            </li>
          )
        })}
      </ul>
    </Panel>
  )
}
