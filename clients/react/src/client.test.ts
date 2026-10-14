import assert from 'node:assert/strict'
import { test } from 'node:test'

import { NinevehClient, NinevehError } from './client.ts'

type Call = { url: string; headers: Record<string, string> }

/** A client over a canned sequence of responses, and the calls it made. */
function client(
  responses: ((call: Call) => Response)[],
  options: { apiKey?: string } = { apiKey: 'k' },
) {
  const calls: Call[] = []
  const fetch = ((url: string | URL | Request, init?: RequestInit) => {
    const call = {
      url: String(url),
      headers: (init?.headers ?? {}) as Record<string, string>,
    }
    calls.push(call)
    const next = responses.shift()
    assert.ok(next !== undefined, `unexpected request: ${call.url}`)
    return Promise.resolve(next(call))
  }) as typeof globalThis.fetch
  const base = { project: 'https://api.example/projects/p/', fetch }
  return {
    calls,
    client: new NinevehClient(
      options.apiKey === undefined ? base : { ...base, apiKey: options.apiKey },
    ),
  }
}

const json = (body: unknown, status = 200) =>
  new Response(JSON.stringify(body), { status, headers: { 'content-type': 'application/json' } })

const page = (rows: unknown[], count: number | null = null) =>
  json({ rows, count, limit: 50, offset: 0 })

test('rows reads a table, with the key as a bearer header', async () => {
  const { client: c, calls } = client([() => page([{ _version: '7', seller: '0xa' }])])
  const result = await c.rows('sellers', { order: 'revenue.desc', limit: 10 })
  assert.deepEqual(result.rows, [{ _version: '7', seller: '0xa' }])
  assert.equal(result.count, undefined)
  assert.equal(calls[0]?.url, 'https://api.example/projects/p/v1/tables/sellers?order=revenue.desc&limit=10')
  assert.equal(calls[0]?.headers.authorization, 'Bearer k')
})

test('a local plane needs no key', async () => {
  const { client: c, calls } = client([() => page([])], {})
  await c.rows('sellers')
  assert.equal(calls[0]?.headers.authorization, undefined)
})

test('a count only appears when the server returned one', async () => {
  const { client: c } = client([() => page([], 12)])
  assert.equal((await c.rows('sellers', { count: true })).count, 12)
})

test('wide integers are left exactly as they arrived', async () => {
  const { client: c } = client([() => page([{ _version: '9', revenue: '18446744073709551615' }])])
  const [row] = (await c.rows('sellers')).rows
  // Not a number: `Number()` would have landed on 18446744073709552000.
  assert.equal(row?.revenue, '18446744073709551615')
})

test("a rebuild's 503 is retried, and the error says what the server said", async () => {
  const { client: c, calls } = client([
    () => json({ error: "the served build doesn't match the config yet" }, 503),
    () => page([{ _version: '1' }]),
  ])
  assert.equal((await c.rows('sellers')).rows.length, 1)
  assert.equal(calls.length, 2)

  const { client: d } = client([() => json({ error: 'no state table `nope`' }, 404)])
  const error = await d.rows('nope').catch((e: unknown) => e)
  assert.ok(error instanceof NinevehError)
  assert.equal(error.status, 404)
  assert.equal(error.message, 'no state table `nope`')
  assert.equal(error.retryable, false)
})

test('the table list is read once and kept', async () => {
  const shape = [{ name: 'sellers', kind: 'reduce', key: ['seller'], columns: [] }]
  const { client: c, calls } = client([() => json(shape)])
  assert.deepEqual(await c.tables(), shape)
  assert.deepEqual(await c.tables(), shape)
  assert.equal(calls.length, 1)
})

test('a failed table list is not remembered as the answer', async () => {
  const shape = [{ name: 'sellers', kind: 'reduce', key: ['seller'], columns: [] }]
  const { client: c } = client([() => json({ error: 'nope' }, 401), () => json(shape)])
  await assert.rejects(c.tables())
  assert.deepEqual(await c.tables(), shape)
})
