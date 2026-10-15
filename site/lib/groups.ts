// The sidebar's sections, kept apart from `docs.ts` because the components that need
// them run in the browser and `docs.ts` reads the repo off disk. A client component
// importing a value from that module pulls `node:fs` into the bundle with it.

/**
 * The documentation's four parts, in order.
 *
 * Grouped rather than flat because the order carries an argument: a developer arrives
 * wanting rows in their app, so React comes before the backend that fills it, and the
 * reference material it is all built on comes last.
 */
export const GROUPS = ["Start", "React", "Backend", "Reference"] as const;

export type Group = (typeof GROUPS)[number];
