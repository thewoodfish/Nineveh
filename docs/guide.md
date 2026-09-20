# The Nineveh Guide

> **Get live data from your Aptos contract in under 2 minutes — no infrastructure, no indexer, no hassle.**

This guide assumes you're using the hosted Nineveh service (where we run the infrastructure for you). If you're self-hosting, see the [self-hosted guide](../SELF_HOSTED.md).

---

## 🚀 Quick Start: Your First Project in 90 Seconds

Follow these steps to have live blockchain data flowing to your app:

1. **Sign in to Studio**  
   → Open <https://studio.nineveh.dev>  
   → Click “Sign in with GitHub”

2. **Create your project**  
   → Click **New project**  
   → Network: `testnet`  
   → Contract address: `0x1` (Aptos framework)  
   → Click **Inspect**

3. **Choose what to follow**  
   → In the modal, click **None** for all three sections  
   → Scroll to **Events** and tick `NewBlockEvent` (`0x1::block`)  
   → Name your project: `blocks`  
   → Select **From now on** (starts from current chain tip)  
   → Click **Create backend with 1 table**

4. **See live data arrive**  
   → Watch the Overview: *Following the chain*, `0` versions behind  
   → The `new_block_event` table fills at ~200 rows/second  
   → Open the API playground in Studio to query it  
   → Or curl directly:  
     ```sh
     curl https://api.nineveh.dev/projects/blocks/v1/tables/new_block_event?limit=3
     ```

5. **Subscribe to changes (optional)**  
   → In another terminal:  
     ```sh
     curl -N https://api.nineveh.dev/projects/blocks/v1/changes
     ```
   → You’ll see live Server-Sent Events as blocks are added

💡 **Pro tip**: Use “From now on” to avoid waiting for deep backfill. To get historical data later, adjust `start_version` in Settings → Configuration.

---

## 📖 Table of Contents
- [The model, in one minute](#1-the-model-in-one-minute)
- [Before you start](#2-before-you-start)
- [Path A — the control plane and Studio](#3-path-a---the-control-plane-and-studio)
- [Reading your data](#4-reading-your-data)
- [Changing a project](#5-changing-a-project)
- [What happens when nobody is looking](#6-what-happens-when-nobody-is-looking)
- [Limits, retention and what gets deleted](#7-limits-retention-and-what-gets-deleted)
- [Operating it](#8-operating-it)
- [When something goes wrong](#9-when-something-goes-wrong)
- [What isn’t built](#10-what-isnt-built)
- [A walkthrough to test against](#11-a-walkthrough-to-test-against)

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

**Four consequences you’ll actually encounter:**

- **State is a projection, not a database you write to.**  
  No `POST /rows` — everything comes from reducers folding chain data.

- **A reducer is deterministic.**  
  No clock, no randomness, no network → replay makes byte-identical state.

- **The records are kept.**  
  Edit a rule? Nineveh replays from the record log (seconds), not re-reads the chain (hours).

- **Aptos finalizes.**  
  No reorgs to handle — safety is just a version cursor plus idempotent restart.

---

## 2. Before you start

**You need exactly two things:**

1. **A GitHub account** — for authentication and project scoping in Studio
2. **Nothing else to install or run** — we handle Rust, Postgres, Geomi keys, and all infrastructure

> 🔑 **Pass keys as environment variables only when self-hosting.**  
> In the hosted service, configuration happens entirely through the Studio UI.

---

## 3. Path A — the control plane and Studio

This is where 95% of users live: sign in to Studio on the web, create a project from a contract address, and call your project’s HTTPS API from your app.

Open <https://studio.nineveh.dev> (or your instance’s Studio URL). Sign in with GitHub.

### ✅ A project that works right now

Aptos’ own framework is published on every network and always busy — perfect for a first look.

1. **New project** → network **testnet** → address `0x1` → **Inspect**  
2. **Choose what to follow** → Click **None** in all sections → Tick `NewBlockEvent` (`0x1::block`)  
   → *Everything it offers works (~300 tables) but that’s overwhelming for a first look.*  
3. **Name it** → `blocks` → Choose **From now on** → **Create backend with 1 table**

Within seconds, the Overview shows *Following the chain*, `0` versions behind, and your `new_block_event` table fills at roughly 200 versions a second.

> 🎯 **Pick `NewBlockEvent`, not `NewBlock`.**  
> Testnet emits the legacy handle-based `NewBlock`. A source matching nothing isn’t an error — you get a clean, empty project (exactly what you’d see if the contract were silent).

### 👀 What to look at in Studio

- **Overview** — cursor, chain head, lag, throughput, and *History kept* (how far back a rebuild can reach)
- **Change feed** — rows arriving live in real-time
- **API playground** — test your REST API against live data
- **Settings → Plan** — your current limits and quotas
- **Settings → Ingest** — shared Transaction Stream status (one per network, shared across all projects)

---

## 4. Reading your data

Everything below targets your project’s base URL in the hosted service:

```
https://api.nineveh.dev/projects/{name}
```

### 🔌 REST API

```sh
BASE=https://api.nineveh.dev/projects/blocks

curl $BASE/v1/status          # build health, cursor, ongoing rebuilds
curl $BASE/v1/tables          # every table’s schema (kind, key, columns)
curl $BASE/v1/tables/new_block_event
```

#### Query parameters on `/v1/tables/{name}`

| Parameter | Meaning | Example |
|-----------|---------|---------|
| `limit` | Max rows returned (default 50, max 1000) | `?limit=10` |
| `offset` | Skip this many rows | `?offset=20` |
| `order` | Sort direction | `?order=height.desc` (newest first) |
| `count=exact` | Also return total count (costs extra query) | `?count=exact` |
| `column=value` | Equality filters (chain multiple) | `?proposer=0x1&height>100000` |

**Example:** Get the 10 most recent blocks from proposer `0x1`, newest first:
```sh
curl "$BASE/v1/tables/new_block_event?proposer=0x1&limit=10&order=height.desc"
```

#### 📊 Wide integers are strings

Move’s `u64`, `u128`, and `u256` don’t fit JavaScript numbers or Postgres `BIGINT` → stored as `NUMERIC` in DB, returned as decimal strings in JSON (ADR 0008).

**Always parse them with `BigInt`, never `Number`:**
```js
// ✅ Correct
const balance = BigInt(row.balance)

// ❌ Wrong — loses precision above 2^53-1
const balance = Number(row.balance)
```

#### 🪪 Every row carries `_version`

The `_version` field shows when the row was last modified — essential for change feed synchronization and optimistic updates.

### 🌊 The change feed

Real-time updates via Server-Sent Events, in commit order:

```sh
curl -N "$BASE/v1/changes"
```

#### Event format
```
event: change
id: 11292175483.0
data: {"version":"11292175483","seq":0,"table":"new_block_event","op":"insert","key":{...},"row":{...}}
```

- The `id` is `version.seq` — lets browsers resume via standard `Last-Event-ID` header
- `?after=version.seq` starts after a position; `?after=beginning` replays everything
- `?tables=a,b` filters to specific tables
- A `reset` event means a rebuild swapped in (ADR 0016) — reload your data

#### 🔐 Authenticating the change feed in browsers

Browser `EventSource` can’t set custom headers — so we accept project keys as query parameters:

```js
new EventSource(`${BASE}/v1/changes?apikey=nvk_...`);
```

Get your project key from **Settings → API keys** in Studio.

### 🪝 Webhooks

Declare webhooks in your config (version-controlled with everything else):

```yaml
webhooks:
  my_backend:
    url: https://myapp.example/hooks/nineveh
    on: [new_block_event.inserted]
    rows: true
```

Each POST carries up to 100 changes:

```json
{
  "project": "blocks",
  "endpoint": "my_backend",
  "changes": [
    {
      "table": "...",
      "op": "insert",
      "version": "...",
      "seq": 0,
      "key": {...},
      "row": {...}
    }
  ]
}
```

#### 🔒 Signature verification

Each request is signed with `X-Nineveh-Signature: t=<timestamp>,v1=<hex>` where:
```
v1 = HMAC-SHA256(secret, "<timestamp>.<rawBody>")
```

Verify it like this:
```js
const [t, v1] = sig.split(",").map(p => p.split("=")[1]);
const expected = createHmac("sha256", secret).update(`${t}.${rawBody}`).digest("hex");
if (!timingSafeEqual(Buffer.from(v1), Buffer.from(expected))) throw new Error("bad signature");
if (Math.abs(Date.now() / 1000 - Number(t)) > 300) throw new Error("too old");
```

#### 📦 Delivery guarantees

- **At-least-once delivery** — batches are retried until acknowledged with 2xx
- Two approaches to handle duplicates:
  1. `rows: true` (default) — compare each change’s `version` and `seq` with what you’ve applied
  2. `rows: false` — delivery carries only the key (“this row changed, come and look”). Self-correcting because a fetch always returns current state.

> 🔑 **Get/set/webhook secrets** in **Settings → Webhooks** — rotate anytime, see failure positions.

---

## 5. Changing a project

This is where Nineveh surprises newcomers in the best way: **changing rules replays locally; adding a source backfills.**

A project keeps **every record its sources matched** — keyed by *source*, not by rules. They record what arrived, not what you did with it.

| Change | Cost | Why |
|--------|------|-----|
| Edit a reducer rule | Seconds to minutes | Local replay from record log |
| Add a state table over existing sources | Seconds to minutes | Local replay |
| Change a column type or key | Seconds to minutes | Local replay |
| **Add a source** | Backfill from chain | No history exists for something never followed |
| Change `start_version` | Backfill if moving earlier than log | Must reconstruct missing history |

### 🔁 The rebuild flow (zero downtime)

1. Rebuild happens **beside** live tables in a `__next` schema
2. Your API keeps serving the old build the entire time
3. Once the rebuild catches up, it **swaps in atomically**
4. The change feed emits one `reset` — clients reload
5. **Nothing goes down. Nothing serves half a rebuild.**

### ⚙️ In Studio

**Settings → Configuration** → Edit your `nineveh.yaml` → Save  
Nineveh detects the change and rebuilds in the background.

> 💡 **Remember:** Changing rules is fast (local replay). Adding sources triggers backfill (network-dependent).

---

## 6. What happens when nobody is looking

A project nobody reads **stops folding** after 24 hours (ADR 0023). In Studio it shows **Idle**, and the list card says *keeping records* instead of *following the chain*.

### 😴 Idle vs Stopped: The important distinction

|  | Idle | Stopped |
|---|------|---------|
| Follows the chain | ✅ yes | ❌ no |
| Keeps records | ✅ yes | ❌ no |
| Computes rows | ❌ no | ❌ no |
| Serves the API | ✅ yes | ✅ yes |
| Restarted by | 👁️ reading it | 👆 you |

### 💤 Why idle projects keep accumulating records

If idle projects **didn’t** keep records, waking one after a week would require **five hours of streaming** to catch up.

Instead:
- Records keep accruing while idle (bounded: only ~2 seconds of folding behind)
- Opening the project or querying its API wakes it
- The read waits for the catch-up (a local replay of a small backlog)
- You’ll see the project is **already caught up** when you open it
- To spot an idle project, check the **projects list** (which doesn’t read any project’s API)

---

## 7. Limits, retention and what gets deleted

| Limit | Value | Why it exists |
|-------|-------|---------------|
| **Projects** | 2 | Fold CPU and a schema each |
| **Networks** | testnet, devnet | Mainnet costs real stream time |
| **Start within** | 6 hours of the tip | Deep backfill ties up catch-up streams |
| **Record log** | 1 GB per project | Stored bytes (~2M records = months of data) |
| **Change feed kept** | 7 days | Stored bytes |

Studio reads these from `/control/v1/me` — numbers are always current.

### 🗑️ What gets pruned (and what’s safe)

- **Change feed** — pruned by time (7 days), never past the slowest webhook’s position  
  → An endpoint stuck behind holds its own backlog (those deliveries are still owed)

- **Record log** — pruned by size (oldest first), only gives up records the fold has already consumed  
  → A record the fold hasn’t reached exists nowhere but here and the chain  
  → Idle/halted/behind projects **keep everything** however far over allowance — and report they’re over (never destroy needed data)

> 💾 **Practical meaning of "1 GB of history"**  
> At ~500 bytes per record: about two million records — months of a normal app contract — and exactly how far a rebuild can reach without paying for history again.

---

## 8. Operating it

### 📊 The numbers that matter

From `GET /v1/status` or the Studio Overview:

- **cursor** — last committed version (must keep moving)
- **lag_secs** — seconds between now and last committed block’s time (*this*, not version gap, is “behind”)
- **versions_per_sec** — throughput (testnet: ~220 v/s, mainnet: ~148 v/s — tailing below this means falling behind)
- **phase** — `starting`, `running`, `retrying`, `halted`, `stopped`

### 🔗 Shared ingest (one stream per network)

One Transaction Stream per **network**, shared across all projects (ADR 0021).  
Aptos caps concurrent streams per org — **7 on testnet, 22 on mainnet** — so:
- A stream per project would limit hosted deployments to seven customers
- Adding a project costs **zero new connections**
- Projects starting behind take one of four **catch-up streams**, fill history, then join

**Check shared readers:**
```sh
curl https://api.nineveh.dev/control/v1/readers
# [{"network":"testnet","position":"11292176967","projects":2,"slots_free":4}]
```
→ `slots_free` shows available catch-up streams (zero means next backfill waits — intended behavior)

### ⚠️ Halted projects

- **Deterministic failure** (bad data, reducer underflow, config mismatch)  
  → Halts one project at that version with a **located error**, never skips it  
  → Everything before the failing version is committed  
  → Nothing else is affected

- **Retryable failure** (dropped connection, Postgres restart)  
  → Retries with growing backoff, resumes from committed cursor  
  → Commits are atomic, fold is deterministic → result is exactly as if nothing failed

- `Stream-duration-limit-reached-please-reconnect`  
  → **Normal** — Transaction Stream closes connections at max duration, expects new one  
  → Commonest thing in `last_error` on healthy projects — Studio doesn’t flag it

### 🚂 One plane at a time

**Run one control plane against a database.**  
Two control planes trying to write the same project will conflict at commit time:

> *“the cursor for X moved …: another writer is committing to this project”*

That error is **deliberately not retryable** — it stops rather than spins.

What two planes *do* cost you:
- Projects halted partway through (need a restart to recover)
- A second shared reader per network eating into your stream cap (7 on testnet)

Detected at commit time (not startup) — so for rolling deploys: **stop the old plane, then start the new one**.

---

## 9. When something goes wrong

| What you see | What it is |
|--------------|------------|
| A project runs cleanly and stays empty | The source matched nothing. Check the exact type name — `NewBlock` and `NewBlockEvent` are different types, and a contract may emit only one. |
| `Filter is too complicated` | More event types than the stream’s filter allows. Nineveh auto-fallbacks to a coarser filter — if you see this, your source list is enormous. |
| New control endpoints 404 | A stale `nineveh up` from an earlier session is still holding the port. Run: `lsof -nP -iTCP:4000 -sTCP:LISTEN` |
| Studio shows *Lost the API* | The plane isn’t answering. Studio shows the last known state rather than blanking. |
| `ResourceExhausted ... 429` | The concurrent-stream cap. Something else is holding streams on your Geomi key. |

---

## 10. What isn’t built

Stated plainly so you don’t design against missing features:

- **📊 GraphQL.** `api: { graphql: true }` parses and does nothing. REST and change feed are real.
- **📈 Live aggregate queries.** You *cannot* subscribe to `SELECT top 10 by volume` and have it stay correct. Change feeds are row- and table-level only. Live aggregates require incremental view maintenance (Materialize/RisingWave territory) — deliberately out of scope for v1.
- **🔐 Writable tables, auth, row-level security.** Chain-derived state is a read-only projection. A full Supabase-scale backend (wallets, gas stations, etc.) is a separate, much larger effort.
- **💰 Billing.** The Free tier’s limits are enforced; nothing to upgrade to yet — product says “coming soon”.
- **🔍 Matching before decode.** The shared reader hands every batch to every project on its network; each decodes against its own lock. Correct, but CPU cost scales with projects — will matter before stream count does again.
- **🐳 Deployment.** No Dockerfile, no health endpoint, and `nineveh up` handles Ctrl-C but not SIGTERM.

---

## 11. A walkthrough to test against

Each step produces something you can see, hear, or touch:

1. **Sign in & create**  
   → Go to <https://studio.nineveh.dev>, sign in with GitHub  
   → Create `blocks` from `0x1`, one source `NewBlockEvent`, **From now on**  
   → Overview reaches *Caught up*, rows arrive within seconds

2. **Confirm wide integers are strings**  
   → `curl $BASE/v1/tables` and `.../v1/tables/new_block_event?limit=5&order=height.desc`  
   → Verify `height` and other big numbers come back as quoted strings

3. **Watch the change feed**  
   → `curl -N $BASE/v1/changes` in one terminal while rows arrive  
   → See event stream; kill it and resume with `?after=<last id>` — nothing missed

4. **Add a second project**  
   → **Settings → Ingest** still shows one stream, now with 2 projects

5. **Trigger a rebuild**  
   → Edit the first project’s config in Studio — add a state table over the existing source  
   → It rebuilds from the record log, old tables keep serving, feed sends `reset`

6. **Check history depth**  
   → **History kept** on the Overview shows records and bytes climbing  
   → “Back to version N” tells you how far a rebuild reaches without repurchasing history

7. **Force idle, then wake**  
   → Set project’s last read to 2 days ago:  
     `update nineveh.project_reads set last_read_at = now() - interval '2 days' where project = 'blocks'`  
   → Wait up to 60s → projects list shows **Idle** / *keeping records*; plane logs it  
   → Open the project → it’s already caught up

8. **Test crash recovery**  
   → Stop and restart the plane mid-backfill  
   → It resumes from the committed cursor — no gap, no duplicate

9. **Verify webhook delivery**  
   → Point a webhook at a local receiver, verify a signature, return a non-2xx  
   → The batch is redelivered; endpoint’s error shows in Settings → Webhooks

### 🧪 Things worth breaking on purpose (to verify behavior)

- **Source type that doesn’t exist** → Validation should fail at the line
- **Reducer that underflows** → Project halts at that version with a located error (never skips silently)
- **Two `nineveh up` processes on one database** → Don’t — see §8; this is the one with no guard yet

---

## 💡 Pro Tips for Hosted Users

### 🎯 Getting the most from your 2 projects
- Use one for **production**, one for **staging/experiments**
- Remember: `testnet` and `devnet` are free; `mainnet` costs real stream time
- Deep backfills (>6h) wait for catch-up streams — use “From now on” for instant startup

### ⚡ Performance & efficiency
- The **Overview** shows real-time lag and throughput — your key health indicators
- **History kept** tells you how far back you can rebuild for free (critical for debugging)
- **Settings → Ingest** lets you monitor shared resource utilization

### 🔐 Security & operations
- **Rotate API keys and webhook secrets** regularly in Settings
- **Monitor failed webhooks** — they show retry counts and failure reasons
- **Idle projects are safe** — they accumulate no cost beyond stored records

### 📚 When you need to go deeper
- **Configuration reference**: [`docs/config.md`](docs/config.md) — every key in `nineveh.yaml`
- **Expression language**: [`docs/expressions.md`](docs/expressions.md) — write powerful reducer rules
- **Architecture decisions**: [`docs/adr/`](docs/adr/) — why we made key technical choices
- **Research measurements**: [`docs/research/`](docs/research/) — stream speeds, costs, and tradeoffs

You’re now ready to build reactive backends for Aptos contracts — no infrastructure, no indexers, just live data from chain to app. 🚀