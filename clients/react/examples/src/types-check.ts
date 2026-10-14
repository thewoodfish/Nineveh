/**
 * What the generated declarations are for, stated as a test the compiler runs.
 *
 * Every `@ts-expect-error` below fails the build if the line it marks stops being an
 * error — so this file is how "a wrong column name or a `u64` treated as a number is a
 * type error" keeps being true. It exists to be typechecked, not imported.
 *
 * It depends on `nineveh-tables.d.ts` beside it, generated from the market project by
 * `npx nineveh-types <url>`.
 */
import type { RowOf, TableName, TableQuery } from '@nineveh/react'

// A table that isn't one. TypeScript even suggests `sellers`.
// @ts-expect-error
const table: TableName = 'sellres'

// A column that isn't on it, in a filter and in an order.
// @ts-expect-error
const filter: TableQuery<RowOf<'sellers'>> = { where: { revenu: { gte: 1 } } }
// @ts-expect-error
const order: TableQuery<RowOf<'sellers'>> = { order: 'revenu.desc' }
// A direction that isn't one: `.desc` and `.asc` are all there is.
// @ts-expect-error
const direction: TableQuery<RowOf<'sellers'>> = { order: 'revenue.descending' }

declare const seller: RowOf<'sellers'>
declare const sale: RowOf<'sales'>

// `revenue` is a `u128`, so it arrives as a decimal string. Treating it as a number is
// the mistake this whole deliverable is about: it is silently wrong above 2^53.
// @ts-expect-error
const revenue: number = seller.revenue
// @ts-expect-error
const doubled = seller.revenue * 2
// Reading a column that does not exist.
// @ts-expect-error
const missing = seller.nope

// And the things that must stay legal: a narrow integer really is a number, a wide one
// really is a string, and both directions of a real column are fine.
const eventIndex: number = sale.event_index
const price: string = sale.price
const fine: TableQuery<RowOf<'sellers'>> = { where: { sold: { gte: 1 } }, order: ['revenue.desc', 'seller'] }

export type { TableName }
export { table, filter, order, direction, revenue, doubled, missing, eventIndex, price, fine }
