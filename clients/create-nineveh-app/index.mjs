#!/usr/bin/env node
// Start a React app on a Nineveh project.
//
// The interesting part is that it asks the project what it holds. `GET /v1/tables`
// reports every table's kind, key and typed columns, so by the time the files are
// written this knows which table to show and what its columns are called — and the
// first thing you see is your own rows rather than a placeholder to go and rewrite.
import { cp, mkdir, readFile, readdir, writeFile } from 'node:fs/promises'
import { existsSync } from 'node:fs'
import { createInterface } from 'node:readline/promises'
import { spawnSync } from 'node:child_process'
import { argv, env, exit, stdin, stdout } from 'node:process'
import { dirname, join, resolve } from 'node:path'
import { fileURLToPath } from 'node:url'

const HERE = dirname(fileURLToPath(import.meta.url))
const bold = (s) => `\u001b[1m${s}\u001b[0m`
const dim = (s) => `\u001b[2m${s}\u001b[0m`
const say = (s = '') => stdout.write(`${s}\n`)
const die = (s) => {
  stdout.write(`\n\u001b[31merror\u001b[0m ${s}\n`)
  exit(1)
}

// --- what we were told, and what we have to ask ----------------------------------
const args = argv.slice(2).filter((a) => !a.startsWith('-'))
const flag = (name) => {
  const at = argv.indexOf(`--${name}`)
  return at === -1 ? undefined : argv[at + 1]
}

if (argv.includes('--help') || argv.includes('-h')) {
  stdout.write(`create-nineveh-app — a React app on your Nineveh project

  npm create nineveh-app@latest [folder] -- [options]

  --project <url>   the project, everything before /v1
  --key <key>       a read key. Also NINEVEH_KEY. Blank for a local project.
  --table <name>    which table the first page shows. Defaults to the widest.

  Anything not given is asked for, unless there is nobody to ask — piped or scripted,
  it takes the defaults.
`)
  exit(0)
}

say()
say(`  ${bold('Nineveh')} — a React app on your project`)
say()

// Opened only if something actually needs asking, and never when there is nobody to
// ask: piped into a script, or run with every answer already given, this takes the
// fallbacks instead of hanging or throwing on a closed stdin.
let rl
const ask = async (question, fallback) => {
  if (!stdin.isTTY) return fallback
  rl ??= createInterface({ input: stdin, output: stdout })
  const answer = (await rl.question(`  ${question}${fallback ? dim(` (${fallback})`) : ''} `)).trim()
  return answer === '' ? fallback : answer
}
const done = () => rl?.close()

const name = args[0] ?? (await ask('Folder name?', 'my-nineveh-app'))
const target = resolve(name)
if (existsSync(target) && (await readdir(target)).length > 0) {
  done()
  die(`${name} already exists and isn't empty.`)
}

const project = (
  flag('project') ??
  env.NINEVEH_PROJECT ??
  (await ask('Project URL?', 'http://127.0.0.1:4000'))
).replace(/\/+$/, '')
const key = flag('key') ?? env.NINEVEH_KEY ?? (await ask(`Read key? ${dim('blank for a local project')}`, ''))

// --- ask the project what it holds ------------------------------------------------
say()
say(dim(`  reading ${project}/v1/tables …`))
const headers = { accept: 'application/json' }
if (key !== '') headers.authorization = `Bearer ${key}`

let tables
try {
  const response = await fetch(`${project}/v1/tables`, { headers })
  if (!response.ok) {
    const hint =
      response.status === 401 || response.status === 403
        ? 'that project needs a read key — Studio → Settings → API keys'
        : response.status === 404
          ? 'the URL is everything before `/v1`. `nineveh up` serves a project at /projects/<name>'
          : (await response.text().catch(() => '')).slice(0, 160)
    done()
    die(`${project} answered ${response.status}. ${hint}`)
  }
  tables = await response.json()
} catch (cause) {
  done()
  die(`could not reach ${project}: ${cause.message}`)
}

if (!Array.isArray(tables) || tables.length === 0) {
  done()
  die(`${project} has no state tables yet. Give it sources and let it run first.`)
}

// The widest table makes the best first screen: most to look at, most obviously theirs.
const widest = [...tables].sort((a, b) => b.columns.length - a.columns.length)[0]
const chosen = await (async () => {
  const asked = flag('table') ?? env.NINEVEH_TABLE
  if (asked !== undefined) return asked
  if (tables.length === 1 || !stdin.isTTY) return widest.name
  say()
  say('  Tables:')
  for (const t of tables) {
    say(`    ${t.name}${dim(` — ${t.kind}, ${t.columns.length} columns`)}`)
  }
  say()
  return ask('Which one to show?', widest.name)
})()
done()

const table = tables.find((t) => t.name === chosen)
if (!table) die(`${project} has no table called \`${chosen}\`.`)

// --- write the files ---------------------------------------------------------------
await mkdir(target, { recursive: true })
await cp(join(HERE, 'template'), target, { recursive: true })

/** A row's key for React, from the table's key columns. */
const rowKey =
  table.key.length === 0
    ? 'row._version'
    : table.key.length === 1
      ? `String(row.${table.key[0]})`
      : `[${table.key.map((k) => `row.${k}`).join(', ')}].join(":")`
const flashKey =
  table.key.length === 0
    ? '"_version"'
    : table.key.length === 1
      ? JSON.stringify(table.key[0])
      : `[${table.key.map((k) => JSON.stringify(k)).join(', ')}]`
// `_version` last: it is on every row and is the least interesting column on screen.
const columns = [...table.columns.map((c) => c.name), '_version']
const headersHtml = columns.map((c) => `              <th>${c}</th>`).join('\n')
const cellsHtml = columns
  .map((c) => `                <td>{String(row.${/^[A-Za-z_$][\w$]*$/.test(c) ? c : `["${c}"]`})}</td>`)
  .join('\n')

const fill = (text) =>
  text
    .replaceAll('__NAME__', name)
    .replaceAll('__TABLE__', table.name)
    .replaceAll('__FIRST__', table.columns[0]?.name ?? '_version')
    .replaceAll('__ROWKEY__', rowKey)
    .replaceAll('__KEY__', flashKey)
    .replaceAll('__HEADERS__', headersHtml)
    .replaceAll('__CELLS__', cellsHtml)

for (const file of ['package.json', 'index.html', 'src/App.tsx', 'src/main.tsx']) {
  const at = join(target, file)
  await writeFile(at, fill(await readFile(at, 'utf8')))
}
// npm refuses to ship a file called `.gitignore` inside a package, so it travels as
// `gitignore` and is renamed here.
await cp(join(target, 'gitignore'), join(target, '.gitignore'))
await writeFile(
  join(target, '.env'),
  `VITE_NINEVEH_PROJECT=${project}\nVITE_NINEVEH_KEY=${key}\n`,
)
await writeFile(
  join(target, '.env.example'),
  `# Your project's URL, and a read key from Studio → Settings → API keys.\n` +
    `# A local project in local mode needs no key.\n` +
    `VITE_NINEVEH_PROJECT=${project}\nVITE_NINEVEH_KEY=\n`,
)

say()
say(`  ${bold(name)} — showing ${bold(table.name)} (${table.columns.length} columns)`)

// --- install, then generate the types ----------------------------------------------
const run = (command, commandArgs, label) => {
  say(dim(`  ${label} …`))
  const done = spawnSync(command, commandArgs, { cwd: target, stdio: 'pipe', encoding: 'utf8' })
  if (done.status !== 0) {
    say(dim(`  ${label} failed; run it yourself:`))
    say(dim(`    ${command} ${commandArgs.join(' ')}`))
    return false
  }
  return true
}

const installed = run('npm', ['install', '--no-audit', '--no-fund'], 'installing')
if (installed) {
  // The generator ships with the package just installed, so the types come from the
  // same version of it that the app imports.
  const types = ['node_modules/.bin/nineveh-types', project, '--out', 'src/nineveh-tables.d.ts']
  if (key !== '') types.push('--key', key)
  run(types[0], types.slice(1), 'generating types for your tables')
}

say()
say(`  ${bold('Done.')}`)
say()
say(`    cd ${name}`)
if (!installed) say('    npm install')
say('    npm run dev')
say()
say(dim('  Your key is in .env, which .gitignore already covers.'))
say(dim('  Regenerate types after a config change with: npm run types'))
say()
