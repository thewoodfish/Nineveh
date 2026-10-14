import { createContext, createElement, useContext, useMemo } from 'react'
import type { ReactNode } from 'react'

import { NinevehClient } from './client.ts'

const Context = createContext<NinevehClient | undefined>(undefined)

export type NinevehProviderProps = {
  /** A project's base URL, e.g. `https://api.nineveh.dev/projects/myapp`. */
  project?: string
  /** A project API key. Optional only against a local plane in local mode. */
  apiKey?: string
  /** An already-built client, instead of `project`/`apiKey`. */
  client?: NinevehClient
  children?: ReactNode
}

/**
 * One client for everything under it: the base URL, the key, and the one change-feed
 * connection the live hooks share.
 */
export function NinevehProvider(props: NinevehProviderProps): ReactNode {
  const { project, apiKey, client, children } = props
  const value = useMemo(() => {
    if (client !== undefined) return client
    if (project === undefined) {
      throw new Error('NinevehProvider needs a `project` URL, or a `client`')
    }
    return new NinevehClient(apiKey === undefined ? { project } : { project, apiKey })
  }, [client, project, apiKey])
  return createElement(Context.Provider, { value }, children)
}

/** The client the nearest provider holds. */
export function useNinevehClient(): NinevehClient {
  const client = useContext(Context)
  if (client === undefined) {
    throw new Error('useNineveh hooks must be used inside a <NinevehProvider>')
  }
  return client
}
