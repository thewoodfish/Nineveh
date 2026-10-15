# create-nineveh-app

A React app on your [Nineveh](https://nineveh.dev) project, in about a minute.

```sh
npm create nineveh-app@latest
```

It asks for a folder, your project's URL and its API key, and then — this is the part
worth knowing — it **asks the project what it holds**. `GET /v1/tables` reports every
table's kind, key and typed columns, so by the time the files are written it knows which
table to show and what its columns are called. The first thing you see is your own rows,
not a placeholder to go and rewrite.

What you get:

```
my-app/
  src/App.tsx              one live table, flashing on change — the file to edit
  src/main.tsx             <NinevehProvider> with your URL and key
  src/nineveh-tables.d.ts  generated: your tables, and the type of every column
  src/styles.css           enough to look deliberate
  .env                     your URL and key, already gitignored
```

Then:

```sh
cd my-app
npm run dev
```

Deliberately not a framework. It is a starting file you delete half of — one hook, one
table, one flash animation, and no abstractions in between.

## Options

Anything you don't pass is asked for, unless there is nobody to ask: piped or scripted,
it takes the defaults.

```sh
npm create nineveh-app@latest my-app -- \
  --project https://api.nineveh.dev/projects/myproject \
  --key nvk_… \
  --table sellers
```

| | |
| --- | --- |
| `--project` | the project, everything before `/v1`. `nineveh up` serves one at `/projects/<name>`; `nineveh run --serve` at the root. Also `NINEVEH_PROJECT`. |
| `--key` | the project's API key, from Studio → **Settings → API keys**. Blank for a local project in local mode. Also `NINEVEH_KEY`. |
| `--table` | which table the first page shows. Defaults to the one with the most columns. Also `NINEVEH_TABLE`. |

The key goes in `.env`, which the generated `.gitignore` already covers. It does reach
the browser when you deploy, and that is fine: reads only ever touch your state tables,
never the chain, so an exposed key spends no stream credit. Give each frontend its
own and revoke it when it's done.

## After a config change

Your tables changed, so the types should too:

```sh
npm run types
```

That is [`nineveh-types`](../react#types-for-your-own-tables), which ships with
`@nineveh/react` and reads the same endpoint this used to scaffold the app.
