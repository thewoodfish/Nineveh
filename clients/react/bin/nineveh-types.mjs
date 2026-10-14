#!/usr/bin/env node
// Write a declaration file for a project's state tables.
//
// Reads `GET /v1/tables`, which already reports every table's kind, key and typed
// columns, so this needs nothing but the URL you already read rows from — no repo, no
// Rust, no config. The mapping from a column type to a TypeScript one lives in the
// package (`declaration`), where it is typed and tested; this file is argv and I/O.
import { existsSync, readFileSync } from 'node:fs'
import { writeFile } from 'node:fs/promises'
import { argv, env, exit, stderr, stdout } from 'node:process'

import { declaration } from '../dist/declare.js'

const USAGE = `nineveh-types — types for your project's tables

  npx nineveh-types [project-url] [--key <key>] [--out <file>]

  [project-url]   everything before /v1, e.g. https://api.nineveh.dev/projects/market
                  or http://127.0.0.1:4000 for one served by \`nineveh run --serve\`.
                  Left out, it is read from .env — VITE_NINEVEH_PROJECT or
                  NINEVEH_PROJECT — along with the key.
  --key, -k       a project read key. Also read from NINEVEH_KEY.
  --out, -o       where to write it. Defaults to src/nineveh-tables.d.ts when there
                  is a src/, since that is what a tsconfig includes, else the cwd.
  --stdout        write to stdout instead of a file

It needs no importing: being inside your tsconfig's \`include\` is what counts. Put it
somewhere the compiler reads, and \`useTable\` knows your tables.
`

/**
 * `.env` in the current directory, as a plain object.
 *
 * Only what this needs: `KEY=value` lines, `#` comments, optional surrounding quotes.
 * Not a dotenv implementation, and it never touches the environment — the values go
 * straight into the options and nowhere else.
 */
function dotenv() {
  if (!existsSync('.env')) return {}
  const found = {}
  for (const line of readFileSync('.env', 'utf8').split('\n')) {
    const at = line.indexOf('=')
    if (at <= 0 || line.trimStart().startsWith('#')) continue
    const name = line.slice(0, at).trim()
    const value = line.slice(at + 1).trim().replace(/^["']|["']$/g, '')
    if (value !== '') found[name] = value
  }
  return found
}

function parse(args) {
  // Inside `src/` when there is one, because that is what a tsconfig includes. A
  // declaration file outside the compiler's `include` is read by nothing and changes
  // nothing — it does not warn, it just quietly does not sharpen anything.
  const out = existsSync('src') ? 'src/nineveh-tables.d.ts' : 'nineveh-tables.d.ts'
  const options = { out, key: env.NINEVEH_KEY, stdout: false }
  const rest = []
  for (let i = 0; i < args.length; i += 1) {
    const arg = args[i]
    if (arg === '--help' || arg === '-h') return { help: true }
    else if (arg === '--stdout') options.stdout = true
    else if (arg === '--key' || arg === '-k') options.key = args[(i += 1)]
    else if (arg === '--out' || arg === '-o') options.out = args[(i += 1)]
    else if (arg.startsWith('-')) return { error: `unknown option \`${arg}\`` }
    else rest.push(arg)
  }
  if (rest.length === 0) {
    // Nothing on the command line, so look where a Vite app keeps it. npm does not read
    // `.env` for a script, which is what makes `npm run types` need this to work at all.
    const env_ = dotenv()
    const project = env_.VITE_NINEVEH_PROJECT ?? env_.NINEVEH_PROJECT
    if (project === undefined) return { error: 'no project URL, and none in .env' }
    options.key ??= env_.VITE_NINEVEH_KEY ?? env_.NINEVEH_KEY
    rest.push(project)
  }
  if (rest.length > 1) return { error: `expected one project URL, got ${rest.length}` }
  return { ...options, project: rest[0].replace(/\/+$/, '') }
}

const options = parse(argv.slice(2))
if (options.help) {
  stdout.write(USAGE)
  exit(0)
}
if (options.error) {
  stderr.write(`error: ${options.error}\n\n${USAGE}`)
  exit(2)
}

const url = `${options.project}/v1/tables`
const headers = { accept: 'application/json' }
if (options.key !== undefined && options.key !== '') {
  headers.authorization = `Bearer ${options.key}`
}

let response
try {
  response = await fetch(url, { headers })
} catch (cause) {
  stderr.write(`error: could not reach ${url}: ${cause.message}\n`)
  exit(1)
}
if (!response.ok) {
  const said = await response.text().catch(() => '')
  const hint =
    response.status === 401 || response.status === 403
      ? '\nhint: this project needs a read key — pass --key, or set NINEVEH_KEY.'
      : response.status === 404
        ? '\nhint: the URL is everything before `/v1`. A project served by `nineveh up` is at /projects/<name>.'
        : ''
  stderr.write(`error: ${url} returned ${response.status}: ${said.slice(0, 200)}${hint}\n`)
  exit(1)
}

const tables = await response.json()
if (!Array.isArray(tables)) {
  stderr.write(`error: ${url} did not answer with a list of tables\n`)
  exit(1)
}
if (tables.length === 0) {
  stderr.write(`error: ${options.project} has no state tables yet\n`)
  exit(1)
}

const text = declaration(tables, options.project)
if (options.stdout) {
  stdout.write(text)
} else {
  await writeFile(options.out, text)
  const columns = tables.reduce((n, t) => n + t.columns.length, 0)
  stderr.write(
    `wrote ${options.out}: ${tables.length} tables, ${columns} columns\n` +
      `it needs no import — but it must be inside your tsconfig's \`include\` to do anything\n`,
  )
}
