import assert from 'node:assert/strict'
import { test } from 'node:test'

import { declaration, tsType } from './declare.ts'
import type { TableInfo } from './types.ts'

test('a wide integer is a string and a narrow one is a number', () => {
  // The whole reason this generator exists. The API renders integers up to 32 bits as
  // JSON numbers and anything wider as a decimal string, so a hand-written type that
  // calls a `u64` a number is wrong in a way nothing complains about until the balance
  // is above 2^53 and quietly wrong.
  for (const narrow of ['u8', 'u16', 'u32', 'i8', 'i16', 'i32']) {
    assert.equal(tsType(narrow), 'number', narrow)
  }
  for (const wide of ['u64', 'u128', 'u256', 'i64', 'i128', 'i256']) {
    assert.equal(tsType(wide), 'string', wide)
  }
})

test('every column type the config has is mapped', () => {
  // The 17 of `ColumnType::ALL`. A type that fell through to the `unknown` default
  // would still compile, and would make every value of that column need a cast.
  const types = {
    bool: 'boolean',
    u8: 'number', u16: 'number', u32: 'number', i8: 'number', i16: 'number', i32: 'number',
    u64: 'string', u128: 'string', u256: 'string',
    i64: 'string', i128: 'string', i256: 'string',
    address: 'string', string: 'string', bytes: 'string',
    json: 'unknown',
  }
  assert.equal(Object.keys(types).length, 17)
  for (const [type, expected] of Object.entries(types)) {
    assert.equal(tsType(type), expected, type)
  }
  // A type from a newer server than this package: usable, and the only thing to cast.
  assert.equal(tsType('decimal128'), 'unknown')
})

const sellers: TableInfo = {
  name: 'sellers',
  kind: 'reduce',
  key: ['seller'],
  columns: [
    { name: 'seller', type: 'address', nullable: false },
    { name: 'revenue', type: 'u128', nullable: false },
    { name: 'note', type: 'string', nullable: true },
  ],
}

test('the declaration augments the package, and says which table it came from', () => {
  const text = declaration([sellers], 'http://127.0.0.1:4000')

  assert.match(text, /declare module '@nineveh\/react'/)
  assert.match(text, /interface NinevehTables/)
  assert.match(text, /sellers: \{/)
  assert.match(text, /seller: string/)
  assert.match(text, /revenue: string/)
  // Nullable means null is a value the column really has, so it is in the type.
  assert.match(text, /note: string \| null/)
  // `_version` is on every row and is not a declared column, so it is added.
  assert.match(text, /_version: string/)
  // The kind and key are in the doc comment, because they are what a reader needs to
  // know to use the table and are nowhere else in TypeScript.
  assert.match(text, /reduce, keyed on seller/)
  assert.match(text, /Generated from http:\/\/127\.0\.0\.1:4000/)
})

test('tables come out in a stable order, whatever order they arrived in', () => {
  const buyers: TableInfo = { name: 'buyers', kind: 'reduce', key: ['buyer'], columns: [] }
  // Otherwise regenerating against the same project rewrites the file and shows up as a
  // diff, which trains people to stop reading the diffs.
  const one = declaration([sellers, buyers])
  const other = declaration([buyers, sellers])
  assert.equal(one, other)
  assert.ok(one.indexOf('buyers:') < one.indexOf('sellers:'))
})

test('a column name that is not an identifier is quoted', () => {
  const odd: TableInfo = {
    name: 'odd',
    kind: 'mirror',
    key: [],
    columns: [{ name: 'has-a-dash', type: 'u8', nullable: false }],
  }
  // Config names cannot contain a dash, but this file is generated from whatever the
  // server reports, and an unquoted dash would not parse.
  assert.match(declaration([odd]), /"has-a-dash": number/)
  assert.match(declaration([odd]), /mirror, keyed on no key/)
})
