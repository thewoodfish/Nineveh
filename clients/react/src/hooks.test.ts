import assert from 'node:assert/strict'
import { beforeEach, test } from 'node:test'
import { createElement } from 'react'
import type { ReactNode } from 'react'

import { api } from '../test/api.ts'
import type { Server } from '../test/api.ts'
import { FakeFeed, feedSource, tick } from '../test/feed.ts'
import { renderHook, quietly } from '../test/render.ts'
import { NinevehClient } from './client.ts'
import { NinevehProvider, useNinevehClient } from './provider.ts'
import type { Row } from './types.ts'
import { useFlash } from './useFlash.ts'
import { useLiveTable } from './useLiveTable.ts'
import { useStatus } from './useStatus.ts'
import { useTable } from './useTable.ts'

/** A provider over a canned server, which is what every hook here needs around it. */
function project(server: Server) {
  const fake = api(server)
  const client = new NinevehClient({
    project: 'https://api.example/projects/p',
    apiKey: 'k',
    fetch: fake.fetch,
    eventSource: feedSource,
  })
  const wrap = (children: ReactNode) => createElement(NinevehProvider, { client }, children)
  return { ...fake, client, wrap }
}

function change(over: Record<string, unknown> = {}): string {
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

test('useTable reads a table, and reports the wait before it has', async () => {
  const { wrap } = project({ rows: { sellers: [{ _version: '7', seller: '0xa' }] }, count: 1 })
  const hook = await renderHook(() => useTable('sellers', { count: true }), wrap)

  assert.deepEqual(hook.current.data, [{ _version: '7', seller: '0xa' }])
  assert.equal(hook.current.loading, false)
  assert.equal(hook.current.error, null)
  assert.equal(hook.current.count, 1)
  await hook.unmount()
})

test('useTable re-reads when the query changes, and not when it is written differently', async () => {
  const { wrap, readsOf } = project({ rows: { sellers: [] } })
  let query: Record<string, string> = { seller: '0xa' }
  const hook = await renderHook(() => useTable('sellers', { where: query }), wrap)
  assert.equal(readsOf('sellers'), 1)

  // The same filters, a new object: the serialized query is the key, so this is a
  // re-render and not a re-read. Without that, every parent render refetches.
  query = { seller: '0xa' }
  await hook.rerender()
  assert.equal(readsOf('sellers'), 1)

  query = { seller: '0xb' }
  await hook.rerender()
  assert.equal(readsOf('sellers'), 2)
  await hook.unmount()
})

test('a refused read becomes an error, not an empty table', async () => {
  const { wrap } = project({ fail: 404 })
  const hook = await renderHook(() => useTable('nope'), wrap)

  assert.equal(hook.current.data, undefined)
  assert.equal(hook.current.loading, false)
  assert.match(String(hook.current.error?.message), /nope/)
  await hook.unmount()
})

test('useLiveTable opens the feed, and a change re-reads the rows once the changes stop', async () => {
  const server: Server = { rows: { sellers: [{ _version: '7', seller: '0xa' }] } }
  const { wrap, readsOf } = project(server)
  const hook = await renderHook(() => useLiveTable('sellers', undefined, { debounceMs: 20 }), wrap)
  await hook.settle(tick)

  const feed = FakeFeed.only()
  await hook.settle(() => feed.emit('open'))
  assert.equal(hook.current.connected, true)

  server.rows = { sellers: [{ _version: '9', seller: '0xa' }] }
  await hook.settle(() => {
    feed.emit('change', change({ version: '1000' }))
    feed.emit('change', change({ version: '1001', seq: 2 }))
    feed.emit('change', change({ version: '1002', seq: 3 }))
  })
  // Three changes in a burst, one read: the feed says something moved, the debounce
  // decides when to go and ask what it is.
  assert.equal(readsOf('sellers'), 1)
  assert.equal(hook.current.lastChange?.version, '1002')

  await hook.after(40)
  assert.equal(readsOf('sellers'), 2)
  assert.deepEqual(hook.current.data, [{ _version: '9', seller: '0xa' }])
  await hook.unmount()
})

test('a rebuild re-reads at once, without waiting out the debounce', async () => {
  const server: Server = { rows: { sellers: [{ _version: '7', seller: '0xa' }] } }
  const { wrap, readsOf } = project(server)
  const hook = await renderHook(
    () => useLiveTable('sellers', undefined, { debounceMs: 10_000 }),
    wrap,
  )
  await hook.settle(tick)

  server.rows = { sellers: [{ _version: '9', seller: '0xb' }] }
  await hook.settle(() => FakeFeed.only().emit('reset'))
  // A debounce this long is the point: rows from a build that no longer exists are
  // wrong now, so this read cannot be the one that was already waiting its turn.
  assert.equal(readsOf('sellers'), 2)
  assert.deepEqual(hook.current.data, [{ _version: '9', seller: '0xb' }])
  await hook.unmount()
})

test('a change to another table does not re-read this one', async () => {
  const { wrap, readsOf } = project({ rows: { sellers: [] } })
  const hook = await renderHook(() => useLiveTable('sellers', undefined, { debounceMs: 5 }), wrap)
  await hook.settle(tick)

  await hook.settle(() => FakeFeed.only().emit('change', change({ table: 'buyers' })))
  await hook.after(20)
  assert.equal(readsOf('sellers'), 1)
  await hook.unmount()
})

test('useFlash lights the row whose version moved, then lets it go', async () => {
  const first: Row[] = [
    { _version: '7', seller: '0xa' },
    { _version: '7', seller: '0xb' },
  ]
  const later: Row[] = [
    { _version: '9', seller: '0xa' },
    { _version: '7', seller: '0xb' },
  ]
  let rows = first
  // An inline array for the key, which is how it reads at a call site — and which used
  // to be the bug: a new array each render re-ran the effect, whose cleanup cancelled
  // the timer that clears the flash, so a row that moved once stayed lit for good.
  const hook = await renderHook(() => useFlash(rows, ['seller'], 30))

  assert.equal(hook.current.size, 0, 'the first read is not a change')
  rows = later
  await hook.rerender()
  assert.deepEqual([...hook.current], ['0xa'])

  await hook.after(60)
  assert.equal(hook.current.size, 0)
  await hook.unmount()
})

test('useStatus measures the gap in versions far past what a double holds', async () => {
  const { wrap } = project({
    status: {
      project: 'p',
      network: 'testnet',
      schema: 's',
      build: null,
      rebuild: null,
      pipeline: {
        phase: 'running',
        schema: 's',
        start_version: '9007199254740990',
        cursor: '9007199254741990',
        chain_version: '9007199254742990',
        chain_head: '9007199254743990',
        lag_secs: 2,
        versions_per_sec: 100,
        retries: 0,
        last_error: null,
      },
    },
  })
  const hook = await renderHook(() => useStatus({ interval: 0 }), wrap)

  // 2^53 + a little: as numbers these subtract to the wrong answer, so the arithmetic
  // is BigInt and the result stays a string, exactly as it arrived (ADR 0008).
  assert.equal(hook.current.behind, '2000')
  assert.equal(hook.current.synced, false)
  assert.equal(hook.current.progress, 0.5)
  assert.equal(hook.current.status?.pipeline?.phase, 'running')
  await hook.unmount()
})

test('useStatus calls a caught-up cursor synced, and has no progress to show', async () => {
  const { wrap } = project({
    status: {
      project: 'p',
      network: 'testnet',
      schema: 's',
      build: null,
      rebuild: null,
      pipeline: {
        phase: 'running',
        schema: 's',
        start_version: '100',
        cursor: '500',
        chain_version: '500',
        chain_head: '500',
        lag_secs: 0,
        versions_per_sec: 1,
        retries: 0,
        last_error: null,
      },
    },
  })
  const hook = await renderHook(() => useStatus({ interval: 0 }), wrap)

  assert.equal(hook.current.behind, '0')
  assert.equal(hook.current.synced, true)
  assert.equal(hook.current.progress, 1)
  await hook.unmount()
})

test('a hook outside a provider says which thing is missing', async () => {
  await quietly(async () => {
    await assert.rejects(
      () => renderHook(() => useNinevehClient()),
      /inside a <NinevehProvider>/,
    )
  })
})

test('a connection that drops and comes back re-reads, because the gap may be unreplayable', async () => {
  const server: Server = { rows: { sellers: [{ _version: '7', seller: '0xa' }] } }
  const { wrap, readsOf } = project(server)
  const hook = await renderHook(() => useLiveTable('sellers', undefined, { debounceMs: 5 }), wrap)
  await hook.settle(tick)

  const feed = FakeFeed.only()
  await hook.settle(() => feed.emit('open'))
  // The first open is not a reconnection: the read that mounted the hook is newer.
  assert.equal(readsOf('sellers'), 1)

  server.rows = { sellers: [{ _version: '9', seller: '0xa' }] }
  await hook.settle(() => feed.emit('error'))
  assert.equal(hook.current.connected, false)

  // Back again. The server replays what it still holds, but a gap longer than the
  // feed's retention has been pruned and nothing says so — so the hook asks.
  await hook.settle(() => feed.emit('open'))
  assert.equal(hook.current.connected, true)
  assert.equal(readsOf('sellers'), 2)
  assert.deepEqual(hook.current.data, [{ _version: '9', seller: '0xa' }])
  await hook.unmount()
})
