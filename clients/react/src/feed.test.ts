import assert from 'node:assert/strict'
import { beforeEach, test } from 'node:test'

import { FakeFeed, feedSource, tick } from '../test/feed.ts'
import { earlier, NinevehClient } from './client.ts'
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

test('positions order with beginning first and the newest change last', () => {
  // `undefined` is the newest change, so it is the identity: whatever it is paired
  // with is earlier than it.
  assert.equal(earlier(undefined, undefined), undefined)
  assert.equal(earlier(undefined, '10.1'), '10.1')
  assert.equal(earlier('10.1', undefined), '10.1')
  assert.equal(earlier('beginning', '10.1'), 'beginning')
  assert.equal(earlier('beginning', undefined), 'beginning')
  assert.equal(earlier('10.1', '10.4'), '10.1')
  assert.equal(earlier('10.4', '10.1'), '10.1')
  assert.equal(earlier('9.1', '10.1'), '9.1')
  // Versions run past what a double holds exactly, so they compare as BigInt: as
  // numbers these two are the same value.
  assert.equal(earlier('9007199254740993.0', '9007199254740992.0'), '9007199254740992.0')
  // Nonsense is passed through for the server to reject rather than reordered here.
  assert.equal(earlier('nonsense', '10.1'), 'nonsense')
})

test('one connection opens early enough for every subscriber on the page', async () => {
  const { client: c } = client()
  c.subscribe({ tables: ['sellers'] })
  c.subscribe({ tables: ['sellers'], after: '900.0' })
  await tick()

  // The live table asked for nothing and gets history it does not need; the activity
  // view asked for history and gets it. One connection, and the earlier ask wins.
  assert.equal(FakeFeed.only().params.get('after'), '900.0')
})

test('a subscriber wanting more history than the open connection carries reopens it', async () => {
  const { client: c } = client()
  c.subscribe({ tables: ['sellers'], after: '900.0' })
  await tick()
  assert.equal(FakeFeed.opened.length, 1)

  // Later than what is open: already covered, since the feed runs forward from 900.0.
  c.subscribe({ tables: ['sellers'], after: '950.0' })
  await tick()
  assert.equal(FakeFeed.opened.length, 1, 'a later position needs no reopen')

  // Earlier: the open connection has already gone past it, and a feed cannot be
  // rewound — only opened again further back.
  c.subscribe({ tables: ['sellers'], after: 'beginning' })
  await tick()
  assert.equal(FakeFeed.opened.length, 2)
  assert.equal(FakeFeed.opened[1]?.params.get('after'), 'beginning')
})

test('a live subscriber alone still opens at the newest change', async () => {
  const { client: c } = client()
  c.subscribe({ tables: ['sellers'] })
  c.subscribe({ tables: ['buyers'] })
  await tick()
  assert.equal(FakeFeed.only().params.get('after'), null)
})
