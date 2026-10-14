import type { ReactNode } from 'react'

/**
 * The bits every page here needs, kept out of the examples so each one is only the
 * thing it is showing. Nothing in this file is special to Nineveh.
 */

/** What a read looks like while it is happening, and after it went wrong. */
type Result = {
  data?: readonly unknown[] | undefined
  loading: boolean
  error: Error | null
  connected?: boolean
}

export function Panel({
  title,
  note,
  result,
  empty,
  children,
}: {
  title: string
  note: string
  result: Result
  empty: string
  children: ReactNode
}) {
  const { data, loading, error, connected } = result
  return (
    <section className="panel">
      <header>
        <h2>{title}</h2>
        {connected !== undefined && (
          <span className={connected ? 'dot' : 'dot off'} title={connected ? 'Following' : 'Connecting…'} />
        )}
      </header>
      <p className="note">{note}</p>
      <div className="card">
        {error !== null ? (
          <p className="bad">{error.message}</p>
        ) : loading && data === undefined ? (
          <p className="waiting">Reading…</p>
        ) : data?.length === 0 ? (
          <p className="waiting">{empty}</p>
        ) : (
          children
        )}
      </div>
    </section>
  )
}

/** `0x1e8742…8d2a`, because a full address is 66 characters of noise. */
export function address(value: unknown): string {
  const text = String(value)
  return text.length > 14 ? `${text.slice(0, 6)}…${text.slice(-4)}` : text
}

/**
 * A credit amount, grouped.
 *
 * The API renders `u64` and wider as decimal strings, because `Number()` loses digits
 * above 2^53 and does it silently. `BigInt` is the only safe way in, and these are
 * whole credits, so there is nothing to divide.
 */
export function credits(value: unknown): string {
  try {
    return BigInt(String(value)).toLocaleString('en-US')
  } catch {
    return String(value)
  }
}

export function Rank({ place }: { place: number }) {
  return <span className={place <= 3 ? `place p${place}` : 'place'}>{place}</span>
}
