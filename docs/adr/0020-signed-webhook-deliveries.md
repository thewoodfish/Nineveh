# 0020. Deliver state changes as signed webhooks, one cursor per endpoint

- Status: Accepted
- Date: 2026-09-18

## Context

The change feed (ADR 0006) serves a browser: a long-lived SSE connection that resumes
by `Last-Event-ID`. It doesn't serve a backend that wants to hear about a liquidation
at 3am without holding a connection open, and it can't reach a serverless function at
all.

`nineveh.yaml` already had a `realtime:` key that parsed, validated and documented
webhooks — including "deliveries are signed" — and **nothing delivered them**. A user
could configure one, get no error, and wait forever. That's worse than a missing
feature, so the choice was to build it or to make the config refuse it.

The transactional outbox makes it tractable. `nineveh.changes` already holds every
row change in commit order, addressed by `(version, seq)`, written in the transaction
that committed the rows. Tailing it is a solved problem in this codebase; delivery is
the part that isn't.

## Decision

**Endpoints are named.** The old shape paired one URL with one change, so a backend
wanting five of them repeated its URL five times, with nowhere to hang a secret or a
position:

```yaml
webhooks:
  my_backend:
    url: https://myapp.example/hooks/nineveh
    on: [balances.changed, holders.inserted]
    rows: true
```

The name is the identity: its secret, its cursor and its health belong to it.

**A cursor per endpoint.** Each endpoint tails the outbox from its own `(version,
seq)`. A receiver that's down holds up only its own deliveries — never the pipeline,
never another endpoint. This is the property the whole design turns on.

**At least once.** A batch (up to 100 changes) counts as delivered only after a 2xx,
so a response lost on the way back is sent again. `X-Nineveh-Delivery` carries the
position the batch covers, and receivers that keep rows should ignore a change they've
already applied. Exactly-once would be a lie, so it isn't offered.

**Signed with the endpoint's own secret**, generated once and stored as written —
unlike API keys (ADR 0018), which are hashed, because a signature can't be computed
from a hash. `X-Nineveh-Signature: t=<unix>,v1=<hex>` is the HMAC-SHA256 of
`<t>.<body>`; the timestamp is inside the signed text, so a receiver can refuse an old
one. The secret survives rebuilds: the endpoint row deliberately has no foreign key to
`nineveh.projects`, whose row a swap replaces, because a receiver's signature check
must not break every time a table is rebuilt.

**Rows by default, keys on request.** A delivery always carries the key — a delete has
no row — and carries the row unless `rows: false`. The row is already in the outbox,
so it costs nothing to send. `rows: false` is the cache-invalidation shape: smaller,
and self-correcting under retries and reordering, because a fetch always returns
current state.

**The address is checked before anything is sent.** A hosted Nineveh posting to
user-supplied URLs must not be a way to reach the private network or a cloud metadata
service. Every address a host resolves to must be public — one bad answer refuses the
whole host — and redirects are refused, because they'd lead somewhere the check never
saw. A URL that plainly says `localhost` is allowed to mean it, for developing against
your own machine.

**A new endpoint starts at the end of the feed.** Configuring a webhook must not
deliver a project's whole history into someone's backend. A rebuild's swap moves every
endpoint the same way, for the same reason, and a running sender notices and follows.

**Failures back off** from a second to five minutes and are recorded with what the
receiver said. An endpoint is never disabled automatically: its backlog is bounded by
the outbox, and silently dropping someone's deliveries is worse than retrying.

## Alternatives considered

- **Keys only, never rows.** Smaller and self-correcting, but it makes every receiver
  do a round trip with an API key to learn what happened. Offered as an option instead
  of imposed. The privacy argument for it doesn't apply: this is public chain data.
- **One sender per project**, delivering to endpoints in turn. Simpler, and one dead
  receiver stalls the others.
- **Disabling an endpoint after N failures**, as several hosted products do. Deferred:
  it needs a way to tell the owner and a way to resume, and neither exists yet.
- **Dropping a batch that keeps failing** so the cursor can advance. Rejected: at
  least once is the promise, and silent loss is the one failure mode a delivery system
  can't have.
- **Delivering from the pipeline's commit**, with the rows to hand. Rejected: a slow
  receiver would then hold up the fold, which is the one thing that must never block.
- **Putting the sender in `nineveh-realtime`**, next to the feed it tails. It needs
  the store for its bookkeeping, and the store pulls in the engine, which the
  dependency rules forbid it (ADR 0005). It lives in `nineveh-control`, which already
  supervises everything a project runs.

## Consequences

- A project's state can drive work outside Nineveh without holding a connection open.
- The outbox now has a second reader whose position can lag far behind the feed's. Its
  retention policy, which doesn't exist yet, now has a second reason to exist: today
  `nineveh.changes` is never pruned.
- Each endpoint costs a task and a small query every couple of seconds while idle.
  Many endpoints across many projects will want a shared poller.
- The signature scheme is now a compatibility surface: receivers implement it, so `v1`
  can be added to but not changed. The known answer in the tests is what an
  implementation can be checked against.
- DNS rebinding between the address check and the connection remains possible in
  principle; the client is pinned to the checked addresses, which closes the ordinary
  case.
