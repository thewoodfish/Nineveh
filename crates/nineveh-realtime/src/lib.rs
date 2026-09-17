//! Nineveh's change feed: every change to a project's state rows, as Server-Sent
//! Events, tailing the transactional outbox (ADR 0006).
//!
//! `GET /v1/changes` streams `change` events in commit order. Each event's id is
//! `version.seq`, so a browser that reconnects resumes where it left off through the
//! standard `Last-Event-ID` header. Parameters:
//!
//! - `after=version.seq` starts after that change; `after=beginning` replays the whole
//!   feed. Without either (or `Last-Event-ID`), the stream starts at the newest change.
//! - `tables=a,b` keeps only those tables' changes.
//!
//! A `change` event's data is `{"version", "seq", "table", "op", "key", "row"}`, with
//! `row` in the REST API's shape (`null` for a delete) and `version` a decimal string.
//! When a rebuild is swapped in (ADR 0016), the feed is replaced: the stream sends a
//! `reset` event and continues from the new feed's newest change, and a client should
//! reload what it listed.
//!
//! Commits `NOTIFY` a wake-up; each stream then reads the outbox from its own position,
//! so a missed notification only delays a change until the next poll.

use std::collections::{HashMap, VecDeque};
use std::convert::Infallible;
use std::sync::{Arc, Mutex, MutexGuard, PoisonError};
use std::time::Duration;

use axum::Router;
use axum::extract::{Query, State};
use axum::http::{HeaderMap, StatusCode};
use axum::response::sse::{Event, KeepAlive, Sse};
use axum::response::{IntoResponse, Response};
use axum::routing::get;
use futures::Stream;
use serde::Deserialize;
use serde_json::{Value, json};
use sqlx::PgPool;
use sqlx::postgres::PgListener;
use tokio::sync::broadcast;
use tracing::warn;

/// The channel commits notify, with the schema as payload (`nineveh_store`'s
/// `NOTIFY_CHANNEL`).
const NOTIFY_CHANNEL: &str = "nineveh_changes";
/// How often a stream reads the outbox with no wake-up, in case one was missed.
const POLL: Duration = Duration::from_secs(2);
/// Changes read at a time.
const BATCH: i64 = 500;

/// A position in the feed: after this `(version, seq)`.
type Position = (i64, i32);
/// Before the first change.
const BEGINNING: Position = (-1, -1);

/// One Postgres listener for every feed in the process. A commit's notification names
/// its schema, and wakes that schema's feed. Each listener holds a connection for as
/// long as it lives, so a process serving many projects shares one.
#[derive(Debug)]
pub struct Hub {
    pool: PgPool,
    wakes: Mutex<HashMap<String, broadcast::Sender<()>>>,
}

impl Hub {
    /// Listen for commits to any schema.
    ///
    /// # Errors
    ///
    /// If the listening connection can't be opened.
    pub async fn start(pool: PgPool) -> Result<Arc<Self>, sqlx::Error> {
        let mut listener = PgListener::connect_with(&pool).await?;
        listener.listen(NOTIFY_CHANNEL).await?;
        let hub = Arc::new(Self {
            pool,
            wakes: Mutex::new(HashMap::new()),
        });
        let weak = Arc::downgrade(&hub);
        tokio::spawn(async move {
            loop {
                let notification = listener.recv().await;
                let Some(hub) = weak.upgrade() else { return };
                match notification {
                    Ok(n) => {
                        if let Some(wake) = hub.wakes().get(n.payload()) {
                            let _ = wake.send(());
                        }
                    }
                    // The listener reconnects by itself; streams poll meanwhile.
                    Err(error) => warn!(%error, "change listener failed; retrying"),
                }
            }
        });
        Ok(hub)
    }

    /// The feed of `schema`. It keeps the hub listening while it's alive.
    #[must_use]
    pub fn feed(self: &Arc<Self>, schema: impl Into<String>) -> Arc<Feed> {
        let schema = schema.into();
        let wake = self
            .wakes()
            .entry(schema.clone())
            .or_insert_with(|| broadcast::channel(16).0)
            .clone();
        Arc::new(Feed {
            pool: self.pool.clone(),
            schema,
            wake,
            _hub: Arc::clone(self),
        })
    }

    fn wakes(&self) -> MutexGuard<'_, HashMap<String, broadcast::Sender<()>>> {
        // A panic while holding the map leaves it whole: it's only ever inserted into.
        self.wakes.lock().unwrap_or_else(PoisonError::into_inner)
    }
}

/// One project's change feed.
#[derive(Debug)]
pub struct Feed {
    pool: PgPool,
    schema: String,
    wake: broadcast::Sender<()>,
    _hub: Arc<Hub>,
}

impl Feed {
    /// The feed of `schema`, with a listener of its own. A process serving several
    /// projects should share one [`Hub`] instead.
    ///
    /// # Errors
    ///
    /// If the listening connection can't be opened.
    pub async fn start(pool: PgPool, schema: impl Into<String>) -> Result<Arc<Self>, sqlx::Error> {
        Ok(Hub::start(pool).await?.feed(schema))
    }

    #[must_use]
    pub fn schema(&self) -> &str {
        &self.schema
    }

    /// Wake-ups for this schema's commits, for anything else that tails the outbox:
    /// the webhook sender delivers within a commit or two rather than a poll (ADR
    /// 0020). A missed wake-up only delays a change until the next poll.
    #[must_use]
    pub fn wake(&self) -> broadcast::Receiver<()> {
        self.wake.subscribe()
    }
}

/// The feed's route.
pub fn router(feed: Arc<Feed>) -> Router {
    Router::new()
        .route("/v1/changes", get(changes))
        .with_state(feed)
}

#[derive(Debug, Deserialize)]
struct Params {
    after: Option<String>,
    tables: Option<String>,
}

async fn changes(
    State(feed): State<Arc<Feed>>,
    Query(params): Query<Params>,
    headers: HeaderMap,
) -> Response {
    let resume = headers
        .get("last-event-id")
        .and_then(|v| v.to_str().ok())
        .map(ToOwned::to_owned);
    let after = match resume.or(params.after).as_deref() {
        None => None,
        Some("beginning") => Some(BEGINNING),
        Some(position) => match parse_position(position) {
            Some(position) => Some(position),
            None => {
                return (
                    StatusCode::BAD_REQUEST,
                    axum::Json(json!({ "error": "`after` must be `beginning` or `version.seq`" })),
                )
                    .into_response();
            }
        },
    };
    let tables: Vec<String> = params
        .tables
        .map(|t| {
            t.split(',')
                .filter(|s| !s.is_empty())
                .map(ToOwned::to_owned)
                .collect()
        })
        .unwrap_or_default();
    let tail = match Tail::new(Arc::clone(&feed), after, tables).await {
        Ok(tail) => tail,
        Err(error) => {
            warn!(%error, "couldn't start a change stream");
            return (
                StatusCode::SERVICE_UNAVAILABLE,
                axum::Json(json!({ "error": "the change feed is unavailable" })),
            )
                .into_response();
        }
    };
    Sse::new(stream(tail))
        .keep_alive(KeepAlive::default())
        .into_response()
}

fn parse_position(text: &str) -> Option<Position> {
    let (version, seq) = text.split_once('.')?;
    Some((version.parse().ok()?, seq.parse().ok()?))
}

/// One client's place in the feed.
struct Tail {
    feed: Arc<Feed>,
    wake: broadcast::Receiver<()>,
    position: Position,
    tables: Vec<String>,
    /// The build this stream is reading; a different one means a swap.
    fingerprint: Option<String>,
    pending: VecDeque<Event>,
}

impl Tail {
    async fn new(
        feed: Arc<Feed>,
        after: Option<Position>,
        tables: Vec<String>,
    ) -> Result<Self, sqlx::Error> {
        let wake = feed.wake.subscribe();
        let fingerprint = fingerprint(&feed).await?;
        let position = match after {
            Some(position) => position,
            None => newest(&feed).await?,
        };
        Ok(Self {
            feed,
            wake,
            position,
            tables,
            fingerprint,
            pending: VecDeque::new(),
        })
    }

    /// The next event, waiting for one if need be.
    async fn next(&mut self) -> Event {
        loop {
            if let Some(event) = self.pending.pop_front() {
                return event;
            }
            match self.read().await {
                Ok(true) => continue,
                Ok(false) => {}
                Err(error) => warn!(%error, "reading the change feed failed; retrying"),
            }
            tokio::select! {
                _ = self.wake.recv() => {}
                () = tokio::time::sleep(POLL) => {}
            }
        }
    }

    /// Queue the changes after the position. Returns whether there were any.
    async fn read(&mut self) -> Result<bool, sqlx::Error> {
        let current = fingerprint(&self.feed).await?;
        if current != self.fingerprint {
            self.fingerprint.clone_from(&current);
            self.position = newest(&self.feed).await?;
            self.pending.push_back(
                Event::default()
                    .event("reset")
                    .data(json!({ "fingerprint": current }).to_string()),
            );
            return Ok(true);
        }
        let rows: Vec<(i64, i32, String, String, Value, Option<Value>)> = sqlx::query_as(
            "SELECT version, seq, table_name, op, key, new_row FROM nineveh.changes
             WHERE schema_name = $1 AND (version, seq) > ($2, $3)
               AND (cardinality($4::text[]) = 0 OR table_name = ANY($4))
             ORDER BY version, seq LIMIT $5",
        )
        .bind(&self.feed.schema)
        .bind(self.position.0)
        .bind(self.position.1)
        .bind(&self.tables)
        .bind(BATCH)
        .fetch_all(&self.feed.pool)
        .await?;
        let any = !rows.is_empty();
        for (version, seq, table, op, key, row) in rows {
            self.position = (version, seq);
            let data = json!({
                "version": version.to_string(),
                "seq": seq,
                "table": table,
                "op": op,
                "key": key,
                "row": row,
            });
            self.pending.push_back(
                Event::default()
                    .id(format!("{version}.{seq}"))
                    .event("change")
                    .data(data.to_string()),
            );
        }
        Ok(any)
    }
}

fn stream(tail: Tail) -> impl Stream<Item = Result<Event, Infallible>> {
    futures::stream::unfold(tail, |mut tail| async move {
        let event = tail.next().await;
        Some((Ok(event), tail))
    })
}

async fn fingerprint(feed: &Feed) -> Result<Option<String>, sqlx::Error> {
    sqlx::query_scalar("SELECT fingerprint FROM nineveh.projects WHERE schema_name = $1")
        .bind(&feed.schema)
        .fetch_optional(&feed.pool)
        .await
}

/// The newest change's position, or the beginning of an empty feed.
async fn newest(feed: &Feed) -> Result<Position, sqlx::Error> {
    let row: Option<(i64, i32)> = sqlx::query_as(
        "SELECT version, seq FROM nineveh.changes WHERE schema_name = $1
         ORDER BY version DESC, seq DESC LIMIT 1",
    )
    .bind(&feed.schema)
    .fetch_optional(&feed.pool)
    .await?;
    Ok(row.unwrap_or(BEGINNING))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn positions_parse_from_event_ids() {
        assert_eq!(parse_position("1003.2"), Some((1003, 2)));
        assert_eq!(parse_position("1003"), None);
        assert_eq!(parse_position("a.b"), None);
    }
}
