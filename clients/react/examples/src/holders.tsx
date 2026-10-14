import { useState } from 'react'
import { useFlash, useLiveTable } from '@nineveh/react'

import { Panel, address, credits } from './shared'

/**
 * Everyone's credit balance, sorted however you ask.
 *
 * `balances` mirrors a Move `Table<address, u64>`. A table value that isn't a struct
 * has nowhere to put a name, so the columns are the generic ones: `key` is the account,
 * `value` is the balance.
 *
 * Sorting is React state and nothing else — the query is re-read when it changes,
 * because the hook keys on what the query says rather than on the object. `useFlash`
 * lights the rows whose `_version` moved, so a balance changing is something you see.
 */
export function Holders() {
  const [order, setOrder] = useState('value.desc')
  const [column, direction] = order.split('.')
  const result = useLiveTable('balances', {
    where: { value: { gt: 0 } },
    // `key` last, so equal balances hold a stable order instead of swapping about.
    order: [order, 'key'],
    limit: 25,
  })
  const flashing = useFlash(result.data, 'key')

  const heading = (name: string, label: string, extra = '') => (
    <th
      className={`sortable ${extra}`}
      onClick={() => setOrder(`${name}.${column === name && direction === 'desc' ? 'asc' : 'desc'}`)}
    >
      {label}
      {column === name && <i className="caret">{direction === 'desc' ? '▾' : '▴'}</i>}
    </th>
  )

  return (
    <Panel
      title="Credit holders"
      note="Click a heading to sort. A balance that changes flashes as it arrives."
      result={result}
      empty="No credits claimed yet."
    >
      <table>
        <thead>
          <tr>
            {heading('key', 'Account')}
            {heading('value', 'Credits', 'num')}
          </tr>
        </thead>
        <tbody>
          {result.data?.map((row) => (
            <tr key={String(row.key)} className={flashing.has(String(row.key)) ? 'flash' : undefined}>
              <td className="mono">{address(row.key)}</td>
              <td className="num strong">{credits(row.value)}</td>
            </tr>
          ))}
        </tbody>
      </table>
    </Panel>
  )
}
