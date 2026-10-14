import { useLiveTable, useStatus } from '@nineveh/react'

import { Panel, address, credits } from './shared'

/**
 * Two tables and the health of the thing serving them.
 *
 * `useStatus` is the one hook that isn't about rows: it polls `/v1/status` for the
 * cursor, how far behind the chain it is, and a backfill's progress. The change feed
 * carries row changes, not cursor movement, so there is nothing here to subscribe to.
 *
 * Both tables share one connection with each other — the package opens a single feed
 * for every live hook on the page.
 */
export function Dashboard() {
  const sellers = useLiveTable('sellers', { order: ['revenue.desc', 'seller'], limit: 5 })
  const buyers = useLiveTable('buyers', { order: ['spent.desc', 'buyer'], limit: 5 })
  const { behind, synced, progress, status } = useStatus()

  return (
    <>
      <div className="tiles">
        <div className="tile">
          <div className="label">Cursor</div>
          <div className="value mono">{status?.pipeline?.cursor ?? status?.build?.cursor ?? '—'}</div>
        </div>
        <div className="tile">
          <div className="label">{synced ? 'Chain' : 'Behind by'}</div>
          {/* `behind` is undefined until there is both a cursor and a head to compare,
              which is the first second of a run and any project whose pipeline runs
              elsewhere. Saying nothing beats saying zero. */}
          <div className="value">
            {synced ? 'caught up' : behind === undefined ? '—' : `${credits(behind)} versions`}
          </div>
        </div>
        <div className="tile">
          <div className="label">Backfill</div>
          <div className="value">
            {progress === undefined ? '—' : `${Math.round(progress * 100)}%`}
          </div>
        </div>
      </div>
      <div className="pair">
        <Panel title="Sellers" note="By revenue." result={sellers} empty="No sales yet.">
          <ul className="plain">
            {sellers.data?.map((row) => (
              <li key={row.seller}>
                <span className="mono">{address(row.seller)}</span>
                <b>{credits(row.revenue)}</b>
              </li>
            ))}
          </ul>
        </Panel>
        <Panel title="Buyers" note="By credits spent." result={buyers} empty="No buyers yet.">
          <ul className="plain">
            {buyers.data?.map((row) => (
              <li key={row.buyer}>
                <span className="mono">{address(row.buyer)}</span>
                <b>{credits(row.spent)}</b>
              </li>
            ))}
          </ul>
        </Panel>
      </div>
    </>
  )
}
