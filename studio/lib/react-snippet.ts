// The same request, as the React call that makes it.
//
// The formatter lives in `@nineveh/react` rather than here on purpose: the query shape
// has changed twice since that package existed — comparisons on `where`, then a second
// `order` column — and a snippet builder sitting outside it would have gone stale both
// times, emitting calls the package no longer accepts. Studio asks the package how to
// write its own calls, and a round-trip test in there keeps the two directions honest.
//
// It is the published package rather than the one in this repo, which is deliberate
// twice over: Turbopack will not resolve a symlinked dependency outside the project
// root, and widening its root to fix that would set it watching the repo's `target/`.
// Depending on the release also means the snippet names the version a reader would
// install, not an unreleased one.

import { snippet } from "@nineveh/react";

/** Where the hooks come from, so what you copy is complete. */
const FROM = "@nineveh/react";

/**
 * The hook call for a request, when there is one.
 *
 * Not every route has one. `/v1/tables` is a catalogue a page reads once rather than
 * something to subscribe to, so it gets no snippet rather than a misleading one — the
 * tab simply isn't offered.
 */
export function hookFor(path: string, live = false): string | undefined {
  const [route, search = ""] = path.split("?");

  if (route === "/v1/status") {
    return `import { useStatus } from '${FROM}'\n\nconst { synced, behind, progress } = useStatus()`;
  }

  const table = route?.match(/^\/v1\/tables\/([^/]+)$/)?.[1];
  if (table === undefined) return undefined;

  const hook = live ? "useLiveTable" : "useTable";
  const call = snippet(decodeURIComponent(table), search, { hook });
  return `import { ${hook} } from '${FROM}'\n\n${call}`;
}
