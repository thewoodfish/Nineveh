# Running a project

What it costs to change a project, what happens when nobody's looking, what the limits
are, and how to read it when something looks wrong.

## 1. Changing a project

This is the part that surprises people, in a good way. **Editing a reducer is cheap.
Adding a source is not.**

A project keeps every record its sources matched. Records are what *arrived*; they
don't know what you did with them. So when you change what you do with them, Nineveh
replays its own stored records rather than re-reading the chain.

| Change | What it costs |
| --- | --- |
| Edit a reducer | a local replay of your records |
| Add a state table over sources you already follow | a local replay of your records |
| Change a column type, or a key | a local replay of your records |
| **Add a source** | a backfill from the chain |
| Move `start_version` earlier than your records go | a backfill from the chain |

The last two are expensive for the same reason: no history exists for something you
never followed. Everything else is a replay of data you already have.

### Rebuilds don't take you down

When a change alters what gets built, Nineveh builds the new tables **beside** the
running ones:

1. the new build fills in a separate schema
2. your API keeps serving the old one the whole time
3. once it catches up, the two swap atomically
4. the change feed emits one `reset`, and clients reload

Nothing is ever half-swapped, and there is no window where the API is down or serving
a partial table.

## 2. Idle projects

A project nobody reads stops folding after 24 hours. Studio shows it as **Idle**, and
the card says *keeping records* rather than *following the chain*.

It hasn't stopped. It is still collecting records; it just isn't spending CPU turning
them into rows that nobody is asking for. Reading the project wakes it: the read waits
for a short local replay, and by the time the page renders it's caught up.

|  | Idle | Stopped |
| --- | --- | --- |
| Keeps records | yes | no |
| Folds them into rows | no | no |
| Serves the API | yes | yes |
| Comes back when | you read it | you start it |

Records keep accruing while idle on purpose. If they didn't, waking a project after a
week would mean hours of streaming to catch up. Instead it's always a few seconds of
replay, however long it slept.

To spot an idle project, look at the projects list, since opening the project would
wake it.

## 3. Limits

| Limit | Value | Why |
| --- | --- | --- |
| Projects | 2 | each costs fold CPU and a database schema |
| Networks | testnet, devnet | mainnet costs real stream time |
| Start within | 6 hours of the chain tip | deep backfills tie up shared catch-up capacity |
| Record log | 1 GB per project | about two million records, months of a normal contract |
| Change feed kept | 7 days | |

Studio reads these live, so what it shows is always current.

These are the free tier, and the free tier is permanent. It isn't a trial and it won't
be switched off. Paid plans, when they exist, add mainnet and production scale on top of
it rather than replacing it.

### What gets pruned

**The change feed** is pruned by age, but never past the slowest webhook's position. An
endpoint that's stuck keeps its own backlog, because those deliveries are still owed.

**The record log** is pruned oldest-first when it's over size, and only ever gives up
records the fold has already consumed. A record the fold hasn't reached exists in
exactly two places, here and the chain, so it's never thrown away. A project that is
idle, halted or behind keeps everything, however far over its allowance, and tells you
it's over rather than destroying what it needs.

"1 GB of history" is really "how far back can I rebuild without paying for history
again". The Overview shows it as a version number: *back to version N*.

## 4. The numbers that matter

From `GET /v1/status`, or the Studio Overview:

- **cursor**: the last committed version. It must keep moving.
- **lag**: seconds between now and the block time of the last thing committed. This,
  not the version gap, is what "behind" means.
- **throughput**: versions a second. Tailing the chain needs roughly 220 on testnet
  and 150 on mainnet; below that on a tailing project means falling behind.
- **phase**: `starting`, `running`, `retrying`, `halted` or `stopped`.

## 5. When a project halts

Two kinds of failure, treated differently on purpose.

**A deterministic failure**, such as a reducer underflowing, a value that doesn't fit,
or data that doesn't match the pinned types, halts that one project at that version,
with an error naming the rule. Everything before it is committed. It is never skipped, because
a skipped record would mean silently wrong state forever. Fix the reducer and replay.

**A retryable failure**, such as a dropped connection or a database restart, retries
with growing backoff and resumes from the committed cursor. Commits are atomic and the fold
is deterministic, so the result is exactly as if nothing had failed.

One message that looks alarming and isn't:

```
Stream-duration-limit-reached-please-reconnect
```

Aptos closes stream connections at a maximum duration and expects a new one. It's the
commonest thing in `last_error` on a perfectly healthy project.

## 6. When something looks wrong

| What you see | What it means |
| --- | --- |
| Runs cleanly, stays empty | The source matched nothing. Check the exact type name: `NewBlock` and `NewBlockEvent` are different types, and a contract may emit only one. |
| A number is wrong | A reducer put it there; nothing else can write a table. Read the handler for that column. |
| `429` / `ResourceExhausted` | The concurrent-stream cap. Something else is holding streams on the same key. |
| Studio says *Lost the API* | The backend isn't answering. Studio shows the last state it knew rather than blanking the page. |
| A rebuild seems stuck | Check the Overview's lag, not its version gap. A rebuild from the tip of a long log is still a replay, and finishes. |

## 7. Sharing the chain

All projects on a network read from one Transaction Stream, not one each. Aptos caps
how many streams an account may hold open, so a stream per project would put a hard
ceiling on how many projects can exist. Sharing means adding a project costs no new
connection.

A project that starts behind the tip borrows one of a few catch-up streams, fills its
history, and then joins the shared one. If all the catch-up streams are busy, a new
backfill waits its turn, which is why starting **from now on** is instant and starting
from a contract's whole history sometimes isn't.
