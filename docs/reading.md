# Reading your data

Three ways out: a REST API for asking questions, a change feed for being told, and
webhooks for being told somewhere else. All three serve the same tables.

Everything below uses your project's base URL:

```sh
BASE=https://api.nineveh.dev/projects/myproject
```

Running it yourself, that's `http://127.0.0.1:4000/projects/myproject` instead. Nothing
else on this page changes.

## 1. REST

```sh
curl $BASE/v1/status                  # cursor, lag, whether a rebuild is running
curl $BASE/v1/tables                  # every table: kind, key, columns
curl $BASE/v1/tables/balances         # rows
```

Rows come back with the query that produced them:

```json
{
  "rows": [ { "user": "0xabc…", "balance": "7530", "_version": "6003184262" } ],
  "limit": 50,
  "offset": 0,
  "count": null
}
```

### Narrowing it down

| Parameter | Meaning |
| --- | --- |
| `limit` | how many rows, default 50, max 1000 |
| `offset` | skip this many |
| `order` | `?order=balance.desc`, a column and a direction |
| `count=exact` | also return the total, at the cost of a second query |
| `<column>=<value>` | keep rows where the column equals that |

```sh
curl "$BASE/v1/tables/balances?balance>1000&order=balance.desc&limit=10"
```

### Two things about every row

**Wide integers are strings.** `u64`, `u128` and `u256` don't fit a JavaScript number,
so they're returned as decimal strings. Parse them with `BigInt`, never `Number`; the
loss above 2⁵³ is silent.

**`_version` says when the row last changed.** It's the transaction that last wrote it,
and it's what lets you tell a stale copy from a current one.

## 2. The change feed

Server-Sent Events, in commit order:

```sh
curl -N "$BASE/v1/changes"
```

```
event: change
id: 11292175483.0
data: {"version":"11292175483","seq":0,"table":"balances","op":"update","key":{"user":"0xabc…"},"row":{…}}
```

`op` is `insert`, `update` or `delete`. The `id` is `version.seq`, and it is a position
you can come back to:

| | |
| --- | --- |
| `?after=11292175483.0` | resume just after that change |
| `?after=beginning` | replay everything still kept |
| `?tables=balances,volume` | only these tables |

A browser resumes on its own: `EventSource` sends the last id it saw as
`Last-Event-ID` when it reconnects, so a dropped connection costs nothing.

```js
const feed = new EventSource(`${BASE}/v1/changes?apikey=nvk_…`);
feed.addEventListener("change", (e) => apply(JSON.parse(e.data)));
```

`EventSource` can't set headers, which is why the key goes in the query string here.
Get one from **Settings → API keys**.

**A `reset` event means the tables were rebuilt** and swapped in. Your cached copy is
from the old build; reload it.

## 3. Webhooks

Declared in your config, so they're version-controlled with everything else:

```yaml
webhooks:
  my_backend:
    url: https://myapp.example/hooks/nineveh
    on: [balances.changed]
    rows: true
```

`on` takes `<table>.changed`, `.inserted`, `.updated` or `.deleted`. Each POST carries
up to 100 changes in order:

```json
{
  "project": "myproject",
  "endpoint": "my_backend",
  "changes": [
    { "table": "balances", "op": "update", "version": "…", "seq": 0,
      "key": {…}, "row": {…} }
  ]
}
```

### Check the signature

Every request carries `X-Nineveh-Signature: t=<timestamp>,v1=<hex>`, where `v1` is
`HMAC-SHA256(secret, "<timestamp>.<raw body>")`. Verify against the **raw** body, before
any JSON parsing:

```js
const [t, v1] = sig.split(",").map((p) => p.split("=")[1]);
const expected = createHmac("sha256", secret).update(`${t}.${rawBody}`).digest("hex");

if (!timingSafeEqual(Buffer.from(v1), Buffer.from(expected))) throw new Error("bad signature");
if (Math.abs(Date.now() / 1000 - Number(t)) > 300) throw new Error("too old");
```

The timestamp check is what stops someone replaying a request they captured earlier.
Secrets live in **Settings → Webhooks** and can be rotated whenever you like.

### Delivery is at-least-once

A batch is retried until your endpoint answers 2xx, so you will occasionally see the
same change twice. Two ways to be safe:

- **`rows: true`** sends the row. Compare `version` and `seq` against what you've
  already applied and ignore anything older.
- **`rows: false`** sends only the key: *this row changed, come and look*. Then fetch
  it. Duplicates stop mattering, because a fetch always returns what's current.

The second is less code and harder to get wrong. Use it unless you need the row in the
delivery itself.
