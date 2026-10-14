import { useLiveTable } from '@nineveh/react'

import { Panel, Rank, address, credits } from './shared'

/**
 * Who sells the most — a number the contract never stores.
 *
 * The market emits `Listed`, `Sold` and `Cancelled`; a running total per account would
 * cost gas on every trade, so it keeps none. `sellers` is a reducer's fold of those
 * events, and this is the query that makes it a leaderboard.
 *
 * Two things worth copying. The order has a second column, because revenue ties and
 * without `seller` to break them the rows on page two are whatever Postgres felt like.
 * And the filter is a comparison rather than an equality: `sold.gte 1` leaves out the
 * accounts that listed something and never sold it, who do not belong here.
 */
export function Leaderboard() {
  const result = useLiveTable('sellers', {
    where: { sold: { gte: 1 } },
    order: ['revenue.desc', 'seller'],
    limit: 10,
  })

  return (
    <Panel
      title="Top sellers"
      note="Revenue is the price less the market's cut, summed per account by a reducer."
      result={result}
      empty="Nobody has sold anything yet."
    >
      <table>
        <thead>
          <tr>
            <th className="rank" />
            <th>Seller</th>
            <th className="num">Sold</th>
            <th className="num">Listed</th>
            <th className="num">Revenue</th>
          </tr>
        </thead>
        <tbody>
          {result.data?.map((row, i) => (
            <tr key={String(row.seller)}>
              <td className="rank">
                <Rank place={i + 1} />
              </td>
              <td className="mono">{address(row.seller)}</td>
              <td className="num">{String(row.sold)}</td>
              <td className="num muted">{String(row.listed)}</td>
              <td className="num strong">{credits(row.revenue)}</td>
            </tr>
          ))}
        </tbody>
      </table>
    </Panel>
  )
}
