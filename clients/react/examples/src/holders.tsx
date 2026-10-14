import { useState } from 'react'
import { useFlash, useLiveTable } from '@nineveh/react'
import type { Columns, RowOf } from '@nineveh/react'

import { Panel, address, credits } from './shared'

/** A column of `balances`, so the sort can only ever be set to a real one. */
type Column = Columns<RowOf<'balances'>>

/**
 * Everyone's credit balance, sorted however you ask.
 *
 * `balances` mirrors a Move `Table<address, u64>`. A table value that isn't a struct
 * has nowhere to put a name, so the columns are the generic ones: `key` is the account,
 * `value` is the balance.
 *
 * The sort is two pieces of state rather than one `"value.desc"` string, which is what
 * the generated types push you towards and is better anyway: the column is one of the
 * table's own, the direction is a boolean, and the term handed to `order` is built from
 * both instead of being parsed back out of itself. `useFlash` lights the rows whose
 * `_version` moved, so a balance changing is something you see.
 */
export function Holders() {
  const [column, setColumn] = useState<Column>('value')
  const [descending, setDescending] = useState(true)
  const result = useLiveTable('balances', {
    where: { value: { gt: 0 } },
    // `key` last, so equal balances hold a stable order instead of swapping about.
    order: [`${column}.${descending ? 'desc' : 'asc'}`, 'key'],
    limit: 25,
  })
  const flashing = useFlash(result.data, 'key')

  const heading = (name: Column, label: string, extra = '') => (
    <th
      className={`sortable ${extra}`}
      onClick={() => {
        setDescending(column === name ? !descending : true)
        setColumn(name)
      }}
    >
      {label}
      {column === name && <i className="caret">{descending ? '▾' : '▴'}</i>}
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
            <tr key={row.key} className={flashing.has(row.key) ? 'flash' : undefined}>
              <td className="mono">{address(row.key)}</td>
              <td className="num strong">{credits(row.value)}</td>
            </tr>
          ))}
        </tbody>
      </table>
    </Panel>
  )
}
