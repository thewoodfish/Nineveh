import assert from 'node:assert/strict'
import { test } from 'node:test'

import { MAX_LIMIT, queryString } from './query.ts'

test('an empty query asks for nothing', () => {
  assert.equal(queryString(), '')
  assert.equal(queryString({}), '')
})

test('filters are equality, and the same query is the same string', () => {
  const one = queryString({ where: { seller: '0xabc', sold: 3 } })
  const other = queryString({ where: { sold: 3, seller: '0xabc' } })
  assert.equal(one, 'seller=0xabc&sold=3')
  // The string is also the key a hook re-reads on, so key order must not change it.
  assert.equal(one, other)
})

test('order, limit, offset and count go through as the API takes them', () => {
  assert.equal(
    queryString({ order: 'revenue.desc', limit: 10, offset: 20, count: true }),
    'order=revenue.desc&limit=10&offset=20&count=exact',
  )
  assert.equal(queryString({ count: false }), '')
})

test('a limit past the server cap is clamped, not sent and silently ignored', () => {
  assert.equal(queryString({ limit: 5000 }), `limit=${MAX_LIMIT}`)
  assert.equal(queryString({ limit: 1 }), 'limit=1')
})

test('raw is appended so nothing the API supports is unreachable', () => {
  assert.equal(queryString({ raw: 'weird=1' }), 'weird=1')
  assert.equal(queryString({ order: 'a', raw: '?weird=1' }), 'order=a&weird=1')
  assert.equal(queryString({ order: 'a', raw: '&weird=1' }), 'order=a&weird=1')
})

test('values are encoded', () => {
  assert.equal(queryString({ where: { name: 'a b&c' } }), 'name=a+b%26c')
})

test('a bare value is equality, and an object says how to compare', () => {
  assert.equal(queryString({ where: { seller: '0xabc' } }), 'seller=0xabc')
  assert.equal(queryString({ where: { revenue: { gte: 100 } } }), 'revenue.gte=100')
  // The operator is on the name, so a value that reads like one is still a value.
  assert.equal(queryString({ where: { memo: 'gt.5' } }), 'memo=gt.5')
  assert.equal(queryString({ where: { live: { neq: false } } }), 'live.neq=false')
})

test('a range is two comparisons on one column', () => {
  assert.equal(
    queryString({ where: { revenue: { gte: 100, lt: 500 } } }),
    'revenue.gte=100&revenue.lt=500',
  )
})

test('the same filters are the same string, however they were written', () => {
  const one = queryString({ where: { revenue: { lt: 500, gte: 100 }, seller: '0xa' } })
  const other = queryString({ where: { seller: '0xa', revenue: { gte: 100, lt: 500 } } })
  assert.equal(one, other)
  // Still the cache key a hook re-reads on, so operator order must not change it.
  assert.equal(one, 'revenue.gte=100&revenue.lt=500&seller=0xa')
})

test('an operator left undefined is not sent as the word undefined', () => {
  assert.equal(queryString({ where: { revenue: { gte: 100, lt: undefined } } }), 'revenue.gte=100')
  assert.equal(queryString({ where: { revenue: {} } }), '')
})

test('order takes a list, to break the ties in its first column', () => {
  assert.equal(queryString({ order: 'revenue.desc' }), 'order=revenue.desc')
  assert.equal(
    queryString({ order: ['revenue.desc', 'seller'] }),
    'order=revenue.desc%2Cseller',
  )
})
