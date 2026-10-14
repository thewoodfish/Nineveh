import assert from 'node:assert/strict'
import { test } from 'node:test'

import { queryString } from './query.ts'
import { parseQuery, snippet } from './snippet.ts'

test('a request with nothing in it is the shortest call that makes it', () => {
  assert.equal(snippet('sellers'), "const { data, loading, error } = useTable('sellers')")
  assert.equal(snippet('sellers', ''), "const { data, loading, error } = useTable('sellers')")
  assert.equal(snippet('sellers', '?'), "const { data, loading, error } = useTable('sellers')")
})

test('the leaderboard request becomes the leaderboard call', () => {
  assert.equal(
    snippet('sellers', 'sold.gte=1&order=revenue.desc,seller&limit=10'),
    `const { data, loading, error } = useTable('sellers', {
  where: { sold: { gte: '1' } },
  order: ['revenue.desc', 'seller'],
  limit: 10,
})`,
  )
})

test('a range is one column with two comparisons on it', () => {
  assert.match(
    snippet('balances', 'value.gte=100&value.lt=500'),
    /where: \{ value: \{ gte: '100', lt: '500' \} \}/,
  )
})

test('values keep their quotes, because a wide integer is a string', () => {
  // The one mistake the package exists to prevent: a `u64` arrives as a decimal string,
  // so a snippet that dropped the quotes would be teaching it.
  assert.match(snippet('b', 'value=18446744073709551615'), /value: '18446744073709551615'/)
  // Not quoted: `limit` and `offset` really are numbers.
  assert.match(snippet('b', 'limit=20&offset=40'), /limit: 20,\n {2}offset: 40,/)
})

test('the live hook is the same call with a different name and result', () => {
  assert.equal(
    snippet('sales', 'order=_version.desc', { hook: 'useLiveTable' }),
    `const { data, connected } = useLiveTable('sales', {
  order: '_version.desc',
})`,
  )
})

test('a count is a boolean, not the word the API takes', () => {
  assert.match(snippet('b', 'count=exact'), /count: true,/)
  // Anything else under `count` is not a count, and is dropped rather than guessed at.
  assert.equal(snippet('b', 'count=yes'), "const { data, loading, error } = useTable('b')")
})

test('parsing is the inverse of serializing, for anything the API accepts', () => {
  // The property that keeps the two directions honest. `queryString` sorts its keys, so
  // the fixed point is reached after one pass through it rather than on the raw input.
  const searches = [
    '',
    'limit=10',
    'seller=0xabc',
    'seller=0xabc&sold=3',
    'revenue.gte=100&revenue.lt=500',
    'order=revenue.desc',
    'order=revenue.desc%2Cseller',
    'count=exact&limit=0',
    'live.neq=false&order=key.asc&limit=25&offset=50',
  ]
  for (const search of searches) {
    const once = queryString(parseQuery(search))
    const twice = queryString(parseQuery(once))
    assert.equal(twice, once, search)
  }
})

test('an unknown operator is a column name, not an operator', () => {
  // `where` keys are whatever the server reports, and a column cannot contain a dot —
  // so a suffix that is not an operator is part of a name this package does not know.
  assert.match(snippet('b', 'weird.thing=1'), /'weird\.thing': '1'/)
})

test('a limit past the cap is clamped, the way a read would be', () => {
  assert.match(snippet('b', 'limit=99999'), /limit: 1000,/)
})
