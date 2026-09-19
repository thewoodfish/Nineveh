# The Nineveh guide

How to get a live backend off an Aptos contract, what you get, and what happens to it
while you aren't watching.

This is the hands-on path. The reference material lives elsewhere and isn't repeated
here: [`config.md`](config.md) for every key in `nineveh.yaml`, and
[`expressions.md`](expressions.md) for the reducer expression language. Architecture
decisions and their reasoning are in [`adr/`](adr/); measurements are in
[`research/`](research/).

**Status.** Everything described here works today unless a paragraph says otherwise,
and the last section lists what doesn't exist yet. GraphQL is the notable absence —
`api: { graphql: true }` parses and is ignored.

---

## 1. The model, in one minute

You can use Nineveh without this, but every surprise later makes sense in terms of it.

```
Aptos Transaction Stream
        │
        ▼
    decode            against the Move layouts pinned in nineveh.lock
        │
        ▼
    records           kept, per project — this is the log
        │
        ▼
    reducers          the ONLY thing that writes state
        │
        ▼
    state tables      Postgres, one schema per project
        │
        ├──▶ REST          GET /v1/tables/...
        ├──▶ change feed   GET /v1/changes  (Server-Sent Events)
        └──▶ webhooks      signed POSTs
```

Four consequences you will actually run into:

- **State is a projection, not a database you write to.** There is no `POST /rows`.
  Everything in a state table got there by a reducer folding chain data.
- **A reducer is deterministic.** No clock, no randomness, no network. That is what
  makes a replay produce byte-identical state.
- **The records are kept.** Editing a rule replays them locally instead of re-reading
  the chain, which is the difference between seconds and hours (ADR 0022).
- **Aptos finalizes.** There is no reorg handling anywhere, because there are no
  reorgs. Safety is a version cursor plus idempotent restart.

---

## 2. Before you start

**Postgres.** Any 14+. On macOS via Homebrew the binaries aren't on `PATH`:

```sh
brew install postgresql@16 && brew services start postgresql@16
export PATH="/opt/homebrew/opt/postgresql@16/bin:$PATH"
createdb nineveh
```

**Rust.** `rust-toolchain.toml` pins the version; `rustup` installs it on first use.

```sh
cargo build --release -p nineveh-cli     # the `nineveh` binary
```

**A Geomi key.** The Transaction Stream rejects anonymous requests. Get one at
[geomi.dev](https://geomi.dev). Keys are **per network**, so a control plane serving
more than one network holds one for each.

```sh
export APTOS_API_KEY_TESTNET=aptoslabs_...
export NINEVEH_DATABASE_URL=postgres:///nineveh
```

> Pass keys as environment variables. Never commit one, and never put one in
> `nineveh.yaml` — the config is meant to be checked in.

---

## 3. Path A — the control plane and Studio

This is the path most people want: no YAML to write, a contract address is enough.

```sh
# terminal 1
nineveh up --streams 1

# terminal 2
cd studio && npm install && npm run dev
```

Open <http://localhost:3000>. With no GitHub credentials configured, the plane runs in
**local mode**: no sign-in, loopback only, and no tier limits — it is your machine and
your key.

### A project that works right now

Aptos' own framework is published on every network and is always busy, so it needs no
deploy:

1. **New project** → network **testnet** → address `0x1` → **Inspect**.
2. It finds over 150 modules. Click **Choose what to follow**, press **None** in each
   of the three groups, then tick a single event: `NewBlockEvent` (`0x1::block`).
   Following everything it offers works — it's around 300 tables — but that's a lot for
   a first look.
3. Name it `blocks`, choose **From now on**, and **Create backend with 1 table**.

Within a couple of seconds the Overview shows *Following the chain*, `0` versions
behind, and a `new_block_event` table filling at roughly 200 versions a second.

> Pick `NewBlockEvent`, not `NewBlock`. Both are in the catalog; testnet emits the
> legacy handle-based one. A source that matches nothing is not an error — you get a
> project that runs cleanly and stays empty, which is exactly what you'd see if the
> contract were simply quiet.

### What to look at

- **Overview** — cursor, chain head, lag, throughput, and *History kept* (how much
  record log this project has, and how far back it can be rebuilt for free).
- **Change feed** — rows arriving live.
- **API playground** — the REST API against your own data.
- **Settings → Plan** — your limits, or "Nothing is limited" in local mode.
- **Settings → Ingest** — the shared readers. One per network, however many projects.

---

## 4. Path B — the CLI, one project

Use this when the project lives in your repo and you want the config in version
control. `nineveh.yaml`:

```yaml
name: blocks
network: testnet
start_version: auto

sources:
  new_blocks: { event: 0x1::block::NewBlockEvent }

state:
  new_block_event: { log: new_blocks }
```

Then:

```sh
nineveh init          # reads the ABIs, writes nineveh.lock, resolves start_version
nineveh validate      # offline; reports every problem at its line
nineveh run --serve   # build and keep current, API on 127.0.0.1:4000
```

`nineveh init` writes **`nineveh.lock`**. Commit it. It pins the Move struct layouts
every value is decoded against, so a build today and a build next year decode the same
bytes the same way even if the contract is upgraded (ADR 0010).

`start_version: auto` resolves to the first transaction that touched any source's
contract address — at or before its modules were published — so nothing is missed.
For `0x1` that is genesis, which is a very long backfill. Use an explicit recent
version, or Studio's **From now on**, unless you truly want all of history.

### The other commands

| Command | What it does |
| --- | --- |
| `nineveh validate` | Checks the config against the lock. Offline, no key needed. |
| `nineveh serve` | Serves an existing build without running the pipeline. |
| `nineveh replay --yes` | Drops the schema and builds again from the start. Destructive; the flag is the confirmation. |
| `nineveh run --until <v>` | Stops once version `v` is committed. Useful for reproducible tests. |

`--streams N` backfills N disjoint version ranges at once and reassembles them in
order. Four is the default. **Use `--streams 1` for anything casual** — a deep parallel
backfill is the heaviest thing Nineveh does to your laptop and your disk.

---

## 5. Reading your data

Everything below is against a project's base URL:

- under the control plane: `http://127.0.0.1:4000/projects/{name}`
- under `nineveh run --serve` or `nineveh serve`: `http://127.0.0.1:4000`

### REST

```sh
BASE=http://127.0.0.1:4000/projects/blocks

curl $BASE/v1/status            # build, cursor, rebuild in progress, pipeline health
curl $BASE/v1/tables            # every table's kind, key and columns
curl $BASE/v1/tables/new_block_event
```

Query parameters on `/v1/tables/{name}`:

| Parameter | Meaning |
| --- | --- |
| `limit` | Default 50, maximum 1000. |
| `offset` | Skip this many. |
| `order` | `order=height` or `order=height.desc`. Default: most recently changed first. |
| `count=exact` | Also count the matching rows. Costs a second query — don't ask for it on every page. |
| *anything else* | `column=value` filters on equality. |

```sh
curl "$BASE/v1/tables/new_block_event?proposer=0x1&limit=10&order=height.desc"
```

**Wide integers are strings.** `u64`, `u128` and `u256` don't fit a JavaScript number
and don't fit Postgres `BIGINT` either, so they are `NUMERIC` in the database and
decimal strings in JSON (ADR 0008). Parse them with `BigInt`, never `Number`.

Every row carries **`_version`**, the version of the change that last wrote it.

### The change feed

Server-Sent Events, in commit order:

```sh
curl -N "$BASE/v1/changes"
```

```
event: change
id: 11292175483.0
data: {"version":"11292175483","seq":0,"table":"new_block_event","op":"insert","key":{...},"row":{...}}
```

- The event `id` is `version.seq`, so a browser's `EventSource` resumes through the
  standard `Last-Event-ID` header with nothing to write.
- `?after=version.seq` starts after a position; `?after=beginning` replays the whole
  feed; with neither, you get changes from now on.
- `?tables=a,b` keeps only those tables.
- A `reset` event means a rebuild swapped in (ADR 0016) — reload whatever you listed.

In hosted mode a browser's `EventSource` can't set headers, which is why a project key
is also accepted as an `apikey` query parameter:

```js
new EventSource(`${BASE}/v1/changes?apikey=nvk_...`);
```

### Webhooks

Declared in the config, so they're version-controlled with everything else:

```yaml
webhooks:
  my_backend:
    url: https://myapp.example/hooks/nineveh
    on: [new_block_event.inserted]
    rows: true
```

Each POST carries a batch of up to 100 changes:

```json
{ "project": "blocks", "endpoint": "my_backend",
  "changes": [ { "table": "...", "op": "insert", "version": "...", "seq": 0,
                 "key": {...}, "row": {...} } ] }
```

and is signed with that endpoint's own secret:

```
X-Nineveh-Signature: t=1789774040,v1=<hex>
```

`v1` is `HMAC-SHA256(secret, "<t>.<body>")`. The timestamp is inside the signed text,
so you can refuse an old one:

```js
const [t, v1] = sig.split(",").map(p => p.split("=")[1]);
const expected = createHmac("sha256", secret).update(`${t}.${rawBody}`).digest("hex");
if (!timingSafeEqual(Buffer.from(v1), Buffer.from(expected))) throw new Error("bad signature");
if (Math.abs(Date.now() / 1000 - Number(t)) > 300) throw new Error("too old");
```

**Delivery is at least once.** A batch is marked delivered only after a 2xx, so a
response lost on the way back means the batch is sent again. Two ways to handle that:

- `rows: true` (default) — compare each change's `version` and `seq` with what you've
  applied, and skip what you've seen.
- `rows: false` — the delivery carries only the key: *this row changed, come and look*.
  Smaller, and self-correcting, because a fetch always returns current state however
  the deliveries were retried or reordered.

Get each endpoint's secret from **Settings → Webhooks**, where you can also rotate it.
An endpoint that is failing shows its error and its position there.

---

## 6. Changing a project

This is where Nineveh differs most from what you'd expect, and it's worth
understanding before you edit anything.

**Changing rules replays locally. Adding a source backfills.**

A project keeps every record its sources matched. Records are keyed by *source*, not by
rules — they record what arrived, not what you did with it. So:

| Change | Cost |
| --- | --- |
| Edit a reducer rule | Local replay from the record log. Seconds to minutes. |
| Add a state table over existing sources | Local replay. |
| Change a column type or key | Local replay. |
| **Add a source** | Backfill from the chain. There is no history for something that was never followed. |
| Change `start_version` | Backfill, if it moves earlier than the log reaches back. |

The rebuild happens **beside** the live tables, in a `__next` schema, and swaps in
atomically once it catches up (ADR 0016). Your API keeps serving the old build the
whole time, then sends one `reset` on the change feed. Nothing goes down and nothing
serves half a rebuild.

In Studio: **Settings → Configuration**. From the CLI: edit `nineveh.yaml`, then
`nineveh run` — it notices and rebuilds.

---

## 7. What happens when nobody is looking

A project nobody reads **stops folding** after 24 hours (ADR 0023). In Studio it shows
**Idle**, and the list card says *keeping records* instead of *following the chain*.

Idle is not stopped, and the distinction matters:

|  | Idle | Stopped |
| --- | --- | --- |
| Follows the chain | yes | no |
| Keeps records | yes | no |
| Computes rows | no | no |
| Serves the API | yes | yes |
| Restarted by | reading it | you |

Opening the project, or any query against its API, wakes it — and the read waits for
the catch-up rather than answering from behind it. The wait is bounded: a project may
only fall about two seconds of folding behind before it is woken whether or not anyone
is reading, so waking is a local replay of a small backlog, not a re-read of the chain.

This is why records keep accruing while idle. If they didn't, a project left alone for
a week would cost **five hours of streaming** to wake.

**What you'll see:** open an idle project and it is already caught up. You have to look
at the projects list, which doesn't read any project's API, to catch one idling at all.

---

## 8. Limits, retention and what gets deleted

In hosted mode every account is on the **Free** tier. Local mode has no tier — there is
no account, it's your machine and your key, and nothing is limited.

| Limit | Value | Why it exists |
| --- | --- | --- |
| Projects | 2 | Fold CPU and a schema each. |
| Networks | testnet, devnet | Mainnet is the one that costs real stream time. |
| Start within | 6 hours of the tip | A deep backfill holds one of four catch-up streams for hours. |
| Record log | 1 GB per project | Stored bytes. |
| Change feed kept | 7 days | Stored bytes. |

Studio reads these from `/control/v1/me` rather than hard-coding them, so the numbers
are stated in one place.

**Two things are pruned, on different rules, and neither touches your state tables.**

The **change feed** goes by time — seven days, long enough that a receiver down over a
weekend still catches up — and is *never* pruned past the slowest webhook endpoint's
position. An endpoint stuck behind holds its own backlog open, which is correct: those
deliveries are still owed.

The **record log** goes by size, oldest first, and only ever gives up records the fold
has already consumed. A record the fold hasn't reached is an input that exists nowhere
but here and the chain — and past about a fortnight, only the chain. So a project that
is idle, halted or behind **keeps everything**, however far over its allowance it is,
and reports that it's over rather than destroying what it still needs.

The practical meaning of "1 GB of history": at the ~500 bytes a record measures, that's
about two million records — months of a normal app contract — and it is exactly how far
back a rebuild can reach without paying for history again.

---

## 9. Operating it

### The numbers that matter

From `GET /v1/status`, or the Overview:

- **cursor** — last committed version. The one number that must keep moving.
- **lag_secs** — seconds between now and the last committed block's time. This, not
  the version gap, is what "behind" means to a user.
- **versions_per_sec** — throughput. Testnet commits about 220 versions a second and
  mainnet about 148, so anything below that during a tail means falling behind.
- **phase** — `starting`, `running`, `retrying`, `halted`, `stopped`.

### Shared ingest

One Transaction Stream per **network**, not per project (ADR 0021). Aptos caps
concurrent streams per organization — **7 on testnet, 22 on mainnet**, measured — so a
stream per project would cap a hosted deployment at seven customers whatever the
demand. Adding a project costs no new connection.

A project that starts behind the shared reader takes one of four **catch-up streams**,
fills its history, and joins. **Settings → Ingest** shows how many are free. Zero free
means the next backfill waits for one, which is intended — a wait, never a connection
the cap would refuse.

```sh
curl http://127.0.0.1:4000/control/v1/readers
# [{"network":"testnet","position":"11292176967","projects":2,"slots_free":4}]
```

### Halted projects

A **deterministic** failure — a value that doesn't decode, a reducer underflow, a
config that doesn't match the build — halts that one project at that version with a
located error, and never skips it. Everything before the failing version is committed.
Nothing else is affected.

A **retryable** failure — a dropped connection, Postgres restarting — retries with
growing backoff and resumes from the committed cursor. Commits are atomic and the fold
is deterministic, so the result is exactly as if nothing had failed.

`Stream-duration-limit-reached-please-reconnect` is **normal**. The Transaction Stream
closes a connection after its maximum duration and expects a new one. It's the
commonest thing in `last_error` on a perfectly healthy project, and Studio deliberately
doesn't flag it.

### One plane at a time

**Run one control plane against a database.** Two won't corrupt anything — a commit is
a compare-and-swap on the cursor, taken under a row lock, so the second writer is
refused before it writes a row and halts that project with `the cursor for X moved …:
another writer is committing to this project`. That error is deliberately not
retryable, so it stops rather than spins.

What two planes do cost you is projects halted partway through, needing a start to come
back, and a second shared reader per network spending streams against a cap of 7 on
testnet. It is detected at commit time rather than at startup, so a plane can't tell it
is the second one until it tries to write — which matters most for rolling deploys,
where a new instance is healthy before the old one drains. Prefer stop-then-start.

---

## 10. When something goes wrong

| What you see | What it is |
| --- | --- |
| A project runs cleanly and stays empty | The source matched nothing. Check the exact type name — `NewBlock` and `NewBlockEvent` are different types, and a contract may emit only one. |
| `Filter is too complicated` | More event types than the stream's filter allows. Nineveh falls back to a coarser filter automatically; if you see this, a source list is enormous. |
| `relation "projects" already exists` on start | An old database from before the migration search-path fix. Use a fresh `createdb`. |
| New control endpoints 404 | A stale `nineveh up` from an earlier session is still holding the port with an old binary. `lsof -nP -iTCP:4000 -sTCP:LISTEN`. |
| `ResourceExhausted ... 429` | The concurrent-stream cap. Something else is holding streams on the same key. |
| Studio shows *Lost the API* | The plane isn't answering. Studio keeps showing the last known state rather than blanking. |

### Running the tests

```sh
createdb nineveh_test
NINEVEH_TEST_DATABASE_URL=postgres:///nineveh_test cargo test --workspace
```

Tests that need Postgres skip without that variable (and fail in CI without it). Don't
set `DATABASE_URL` — that switches sqlx's macros to checking against a live database.

Live checks against testnet are `#[ignore]`d:

```sh
APTOS_API_KEY=... NINEVEH_TEST_DATABASE_URL=postgres:///nineveh_test \
  cargo test -p nineveh-ingest -p nineveh-pipeline -p nineveh-cli -- --ignored
```

---

## 11. Hosted mode

With a GitHub OAuth app configured, the plane requires sign-in, scopes projects to
accounts, and requires a project API key on every data request (ADR 0018):

```sh
nineveh up \
  --github-client-id ... --github-client-secret ... \
  --public-url https://api.example.com \
  --studio-url https://studio.example.com
```

Keys are made in **Settings → API keys** and sent as `Authorization: Bearer nvk_...`,
an `apikey` header, or an `apikey` query parameter. Only the SHA-256 of a key is
stored, so a key is shown exactly once, when it's created.

> Hosted mode is covered by tests against a fake GitHub, but has not yet been run
> against real GitHub on a real domain. Expect the OAuth redirect round-trip to be the
> first thing that needs attention.

---

## 12. What isn't built

Stated plainly so nobody writes something against it:

- **GraphQL.** `api: { graphql: true }` parses and does nothing. REST and the change
  feed are real.
- **Live aggregate queries.** You cannot subscribe to `SELECT top 10 by volume` and
  have it stay correct. Change feeds are row- and table-level. Keeping an aggregate
  live is incremental view maintenance, and it is deliberately out of scope.
- **Writable tables, auth, row-level security.** Chain-derived state is a read-only
  projection. A full Supabase-scale backend is a separate, much larger thing.
- **Billing.** The Free tier's limits are enforced; there is nothing to upgrade to, and
  the product says "coming soon" rather than offering to sell you something.
- **Matching before decode.** The shared reader hands every batch to every project on
  its network, and each decodes against its own lock. Correct, but the CPU cost scales
  with projects; it will matter before stream count does again.
- **Deployment.** There is no Dockerfile, no health endpoint, and `nineveh up` handles
  Ctrl-C but not SIGTERM.

---

## 13. A walkthrough to test against

In order. Each step is meant to produce something you can see.

1. `createdb`, export a testnet key, `nineveh up --streams 1`, Studio on :3000.
2. Create `blocks` from `0x1`, one source `NewBlockEvent`, **From now on**.
   → Overview reaches *Caught up*, rows arrive within seconds.
3. `curl $BASE/v1/tables` and `.../v1/tables/new_block_event?limit=5&order=height.desc`.
   → Confirm wide integers come back as **strings**.
4. `curl -N $BASE/v1/changes` in one terminal while rows arrive.
   → Events stream; kill it and resume with `?after=<last id>` and nothing is missed.
5. Add a second project on the same network.
   → **Settings → Ingest** still shows **one** stream, now with 2 projects.
6. Edit the first project's config in Studio — add a state table over the existing
   source. → It rebuilds from the record log, the old tables keep serving, the feed
   sends `reset`.
7. Check **History kept** on the Overview.
   → Records and bytes climb; "back to version N" is how far a rebuild reaches.
8. Force idle: `update nineveh.project_reads set last_read_at = now() - interval '2 days'
   where project = 'blocks';` and wait up to 60s.
   → The projects list shows **Idle** / *keeping records*; the plane logs it. Open the
   project and it is already caught up.
9. Stop and restart the plane mid-backfill.
   → It resumes from the committed cursor with no gap and no duplicate.
10. Point a webhook at a local receiver, verify a signature, return a non-2xx once.
    → The batch is redelivered; the endpoint's error shows in Settings → Webhooks.

Things worth deliberately breaking, because the behaviour is the point: a source type
that doesn't exist (validate should say so, at the line), a reducer that underflows (the
project should halt at a version with a located error, not skip), and two `nineveh up`
processes on one database (don't — see §9; this is the one that has no guard yet).
