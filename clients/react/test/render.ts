/**
 * The smallest thing that can render a hook.
 *
 * A hook is only a hook while React is calling it, so testing one means mounting a
 * component whose whole job is to call it and hand back what it returned. React's own
 * `act` is what makes this deterministic: inside it, effects run and state updates
 * flush before the call returns, so a test can read the result on the next line
 * instead of waiting on a timer and hoping.
 */
import { act, createElement } from 'react'
import type { ReactNode } from 'react'
import { createRoot } from 'react-dom/client'

export type Harness<T> = {
  /** What the hook returned on its most recent render. */
  readonly current: T
  /** How many times the component rendered, for asserting a hook is not thrashing. */
  readonly renders: number
  /** Run `body`, then let React settle: effects, state, and resolved promises. */
  settle: (body?: () => void | Promise<void>) => Promise<void>
  /**
   * Render again, as a parent re-rendering would.
   *
   * How a test changes the hook's arguments: the callback passed to `renderHook` reads
   * them from the test's own variables, and this is what makes React call it again.
   */
  rerender: () => Promise<void>
  /** Wait `ms` of real time, then settle — for a debounce or a flash's timer. */
  after: (ms: number) => Promise<void>
  unmount: () => Promise<void>
}

/** Mount `hook`, optionally inside a wrapper such as a provider. */
export async function renderHook<T>(
  hook: () => T,
  wrap: (children: ReactNode) => ReactNode = (children) => children,
): Promise<Harness<T>> {
  let value: T | undefined
  let renders = 0
  const Probe = () => {
    renders += 1
    value = hook()
    return null
  }

  const container = document.createElement('div')
  document.body.append(container)
  const root = createRoot(container)
  const settle = async (body?: () => void | Promise<void>) => {
    await act(async () => {
      await body?.()
    })
  }

  const draw = () => settle(() => root.render(wrap(createElement(Probe))))
  await draw()

  return {
    get current(): T {
      if (renders === 0) throw new Error('the hook has not rendered')
      return value as T
    },
    get renders(): number {
      return renders
    },
    settle,
    rerender: draw,
    after: (ms: number) => settle(() => sleep(ms)),
    unmount: async () => {
      await settle(() => root.unmount())
      container.remove()
    },
  }
}

export function sleep(ms: number): Promise<void> {
  return new Promise((resolve) => setTimeout(resolve, ms))
}

/** Silence React's render-error logging for one assertion that expects a throw. */
export async function quietly<T>(body: () => Promise<T>): Promise<T> {
  const spoke = console.error
  console.error = () => {}
  try {
    return await body()
  } finally {
    console.error = spoke
  }
}
