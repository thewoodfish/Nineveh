import assert from 'node:assert/strict'
import { beforeEach, test } from 'node:test'

import { FakeFeed, feedSource, tick } from '../test/feed.ts'
import { NinevehClient } from './client.ts'
import type { Change } from './types.ts'

/** A client whose feed is a fake, and whose reads answer with `tables`. */
function client(tables: unknown = []) {
  let reads = 0
  const fetch = (() => {
    reads += 1
    return Promise.resolve(
      new Response(JSON.stringify(tables), {
        status: 200,
        headers: { 'content-type': 'application/json' },
      }),
    )
  }) as typeof globalThis.fetch
  return {
    reads: () => reads,
    client: new NinevehClient({
      project: 'https://api.example/projects/p',
      apiKey: 'k',
      fetch,
      eventSource: feedSource,
    }),
  }
}

function change(over: Partial<Change> = {}): string {
  return JSON.stringify({
    version: '1000',
    seq: 1,
    table: 'sellers',
    op: 'update',
    key: { seller: '0xa' },
    row: { _version: '1000', seller: '0xa' },
    ...over,
  })
}

beforeEach(() => FakeFeed.reset())

test('several hooks on a page share one connection, asking for every table between them', async () => {
  const { client: c } = client()
  c.subscribe({ tables: ['sellers'] })
  c.subscribe({ tables: ['buyers'] })
  c.subscribe({ tables: ['sellers', 'credits'] })
  await tick()

  const feed = FakeFeed.only()
  assert.deepEqual(feed.params.get('tables')?.split(',').sort(), ['buyers', 'credits', 'sellers'])
  // `EventSource` cannot set headers, so this is the one place the key has to ride on
  // the URL. Reads never touch the chain, which is what makes that acceptable.
  assert.equal(feed.params.get('apikey'), 'k')
  assert.equal(feed.params.get('after'), null)
})

test('a change reaches only the subscribers that asked for its table', async () => {
  const { client: c } = client()
  const heard: string[] = []
  c.subscribe({ tables: ['sellers'], onChange: () => heard.push('sellers') })
  c.subscribe({ tables: ['buyers'], onChange: () => heard.push('buyers') })
  c.subscribe({ onChange: () => heard.push('everything') })
  await tick()

  FakeFeed.only().emit('change', change({ table: 'sellers' }))
  assert.deepEqual(heard.sort(), ['everything', 'sellers'])
})

test('a hook for a table the open connection does not carry gets a connection that does', async () => {
  const { client: c } = client()
  c.subscribe({ tables: ['sellers'] })
  await tick()
  assert.equal(FakeFeed.opened.length, 1)

  c.subscribe({ tables: ['credits'] })
  await tick()
  assert.equal(FakeFeed.opened.length, 2)
  assert.equal(FakeFeed.opened[0]?.closed, true)
  assert.deepEqual(FakeFeed.opened[1]?.params.get('tables')?.split(',').sort(), [
    'credits',
    'sellers',
  ])
})

test('a connection already carrying what a new hook wants is kept', async () => {
  const { client: c } = client()
  c.subscribe({ tables: ['sellers', 'credits'] })
  await tick()
  c.subscribe({ tables: ['credits'] })
  await tick()
  assert.equal(FakeFeed.opened.length, 1)
  assert.equal(FakeFeed.only().closed, false)
})

test('the last hook to unmount closes the connection', async () => {
  const { client: c } = client()
  const first = c.subscribe({ tables: ['sellers'] })
  const second = c.subscribe({ tables: ['sellers'] })
  await tick()
  first()
  assert.equal(FakeFeed.only().closed, false)
  second()
  assert.equal(FakeFeed.only().closed, true)
})

test('a reopened connection resumes after the last change it saw', async () => {
  const { client: c } = client()
  c.subscribe({ tables: ['sellers'] })
  await tick()
  FakeFeed.only().emit('change', change({ version: '1041', seq: 3 }))

  // Reopening for a second table is the reachable way to make it reconnect; the
  // position is what a browser's own `EventSource` retry would send as `Last-Event-ID`.
  c.subscribe({ tables: ['credits'] })
  await tick()
  assert.equal(FakeFeed.opened[1]?.params.get('after'), '1041.3')
})

test('a rebuild resets the position and the remembered table shapes', async () => {
  const { client: c, reads } = client([{ name: 'sellers', kind: 'reduce', key: [], columns: [] }])
  await c.tables()
  await c.tables()
  assert.equal(reads(), 1, 'the table list is read once and kept')

  let told = 0
  c.subscribe({ tables: ['sellers'], onReset: () => (told += 1) })
  await tick()
  FakeFeed.only().emit('change', change({ version: '1041', seq: 3 }))
  FakeFeed.only().emit('reset', JSON.stringify({ fingerprint: 'abc' }))

  assert.equal(told, 1)
  // The new build has its own feed and may have its own columns, so neither the
  // position nor the shapes carry over: both were read from the build that just went.
  await c.tables()
  assert.equal(reads(), 2)
  c.subscribe({ tables: ['credits'] })
  await tick()
  assert.equal(FakeFeed.opened[1]?.params.get('after'), null)
})

test('open and error are reported to every subscriber', async () => {
  const { client: c } = client()
  const seen: string[] = []
  c.subscribe({ tables: ['sellers'], onOpen: () => seen.push('open'), onError: () => seen.push('error') })
  c.subscribe({ tables: ['sellers'], onOpen: () => seen.push('open') })
  await tick()

  FakeFeed.only().emit('open')
  FakeFeed.only().emit('error')
  assert.deepEqual(seen, ['open', 'open', 'error'])
})

test('a change that is not JSON is dropped rather than throwing out of the listener', async () => {
  const { client: c } = client()
  let heard = 0
  c.subscribe({ tables: ['sellers'], onChange: () => (heard += 1) })
  await tick()

  FakeFeed.only().emit('change', 'not json')
  FakeFeed.only().emit('change', change())
  assert.equal(heard, 1)
})

test('subscribing with no EventSource to subscribe with says how to fix it', () => {
  const had = globalThis.EventSource
  Reflect.deleteProperty(globalThis, 'EventSource')
  try {
    const c = new NinevehClient({ project: 'https://api.example/projects/p' })
    // Thrown where the caller is, rather than swallowed into a feed that never opens:
    // this is a misconfigured environment, not a connection having a bad minute.
    assert.throws(() => c.subscribe({}), /pass one as `eventSource`/)
  } finally {
    globalThis.EventSource = had
  }
})
