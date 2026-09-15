//! The change feed over a real Postgres: replay, live delivery on commit, filters,
//! resuming by `Last-Event-ID`, and the reset when a rebuild is swapped in.
//!
//! Needs `NINEVEH_TEST_DATABASE_URL`, like the store's tests.

#![allow(
    clippy::unwrap_used,
    clippy::expect_used,
    clippy::panic,
    clippy::print_stderr,
    reason = "test-only crate: helpers panic on unexpected results and note skips"
)]

use std::sync::atomic::{AtomicU32, Ordering};
use std::time::Duration;

use axum::Router;
use axum::body::{Body, BodyDataStream};
use axum::http::{Request, StatusCode};
use futures::StreamExt;
use nineveh_config::{Project, parse};
use nineveh_decode::{DecodedTransaction, LockBuilder, Lockfile};
use nineveh_engine::{Engine, FoldError};
use nineveh_realtime::{Feed, router};
use nineveh_store::{Loaded, Store, shadow_name};
use nineveh_testkit::vault::{self, Op};
use serde_json::Value;
use sqlx::PgPool;
use tower::ServiceExt;

async fn pool() -> Option<PgPool> {
    let Ok(url) = std::env::var("NINEVEH_TEST_DATABASE_URL") else {
        assert!(
            std::env::var_os("CI").is_none(),
            "CI must set NINEVEH_TEST_DATABASE_URL"
        );
        eprintln!("skipping: set NINEVEH_TEST_DATABASE_URL to run the feed's tests");
        return None;
    };
    Some(PgPool::connect(&url).await.unwrap())
}

fn fresh(name: &str) -> String {
    static NEXT: AtomicU32 = AtomicU32::new(0);
    format!(
        "rt_{name}_{}_{}",
        std::process::id(),
        NEXT.fetch_add(1, Ordering::Relaxed)
    )
}

fn project_from(yaml: &str) -> (Lockfile, Project) {
    let config = parse(yaml).unwrap();
    let mut builder = LockBuilder::new(config.network);
    for module in vault::modules() {
        builder.add_module(module);
    }
    let lock = builder.build(&config.roots()).unwrap();
    let project = config.resolve(&lock).unwrap();
    (lock, project)
}

/// Commit each transaction on its own, as the pipeline would at the tip.
async fn commit(pool: &PgPool, schema: &str, yaml: &str, decoded: &[DecodedTransaction]) {
    let (lock, project) = project_from(yaml);
    let engine = Engine::new(&project);
    let mut store = Store::open(pool.clone(), schema, &project, &lock)
        .await
        .unwrap();
    for tx in decoded {
        if Some(tx.version) <= store.cursor() {
            continue;
        }
        let mut cache = Loaded::default();
        let batch = std::slice::from_ref(tx);
        let changes = loop {
            match engine.fold(&cache, batch) {
                Ok(changes) => break changes,
                Err(FoldError::NotLoaded(keys)) => cache.merge(store.load(keys).await.unwrap()),
                Err(e) => panic!("{e}"),
            }
        };
        store.commit(&changes).await.unwrap();
    }
}

fn workload(ops: &[Op]) -> Vec<DecodedTransaction> {
    let (lock, project) = vault::project();
    let (txs, _) = vault::transactions(ops);
    vault::decode(&lock, &project, &txs)
}

/// An open event stream.
struct Events {
    body: BodyDataStream,
    buffer: String,
}

#[derive(Debug, Clone)]
struct Sse {
    event: String,
    id: Option<String>,
    data: Value,
}

impl Events {
    async fn open(app: &Router, uri: &str, last_event_id: Option<&str>) -> (StatusCode, Self) {
        let mut request = Request::get(uri);
        if let Some(id) = last_event_id {
            request = request.header("last-event-id", id);
        }
        let response = app
            .clone()
            .oneshot(request.body(Body::empty()).unwrap())
            .await
            .unwrap();
        let status = response.status();
        (
            status,
            Self {
                body: response.into_body().into_data_stream(),
                buffer: String::new(),
            },
        )
    }

    /// The next event, or `None` if none arrives within a few seconds.
    async fn next(&mut self) -> Option<Sse> {
        loop {
            if let Some(end) = self.buffer.find("\n\n") {
                let block: String = self.buffer.drain(..end + 2).collect();
                let mut event = String::from("message");
                let mut id = None;
                let mut data = String::new();
                for line in block.lines() {
                    if let Some(v) = line.strip_prefix("event:") {
                        v.trim().clone_into(&mut event);
                    } else if let Some(v) = line.strip_prefix("id:") {
                        id = Some(v.trim().to_owned());
                    } else if let Some(v) = line.strip_prefix("data:") {
                        data.push_str(v.trim());
                    }
                }
                if data.is_empty() {
                    continue; // a keep-alive comment
                }
                return Some(Sse {
                    event,
                    id,
                    data: serde_json::from_str(&data).unwrap(),
                });
            }
            let chunk = tokio::time::timeout(Duration::from_secs(5), self.body.next())
                .await
                .ok()??
                .unwrap();
            self.buffer.push_str(std::str::from_utf8(&chunk).unwrap());
        }
    }

    async fn take(&mut self, n: usize) -> Vec<Sse> {
        let mut events = Vec::new();
        for _ in 0..n {
            events.push(self.next().await.expect("an event"));
        }
        events
    }
}

async fn outbox_ids(pool: &PgPool, schema: &str) -> Vec<String> {
    sqlx::query_as::<_, (i64, i32)>(
        "SELECT version, seq FROM nineveh.changes WHERE schema_name = $1 ORDER BY version, seq",
    )
    .bind(schema)
    .fetch_all(pool)
    .await
    .unwrap()
    .into_iter()
    .map(|(v, s)| format!("{v}.{s}"))
    .collect()
}

#[tokio::test]
async fn replays_then_delivers_new_commits_live() {
    let Some(pool) = pool().await else { return };
    let schema = fresh("live");
    Store::reset(&pool, &schema).await.unwrap();
    let decoded = workload(&[
        Op::Deposit {
            user: 1,
            amount: 700,
        },
        Op::CreateVault { vault: 0 },
        Op::Deposit { user: 2, amount: 5 },
        Op::Deposit { user: 1, amount: 3 },
    ]);
    commit(&pool, &schema, vault::CONFIG, &decoded[..2]).await;
    let app = router(Feed::start(pool.clone(), &schema).await.unwrap());

    // From the beginning: exactly the outbox, in order, with feed-shaped rows.
    let ids = outbox_ids(&pool, &schema).await;
    let (status, mut all) = Events::open(&app, "/v1/changes?after=beginning", None).await;
    assert_eq!(status, StatusCode::OK);
    let replayed = all.take(ids.len()).await;
    let replayed_ids: Vec<String> = replayed.iter().map(|e| e.id.clone().unwrap()).collect();
    assert_eq!(replayed_ids, ids);
    let first = &replayed[0];
    assert_eq!(first.event, "change");
    assert_eq!(first.data["version"], "1001");
    assert_eq!(first.data["table"], "balances");
    assert_eq!(first.data["op"], "insert");
    assert_eq!(first.data["row"]["balance"], "700");

    // Without a position the stream starts at the newest change, then gets commits
    // as they happen.
    let (_, mut tail) = Events::open(&app, "/v1/changes?tables=balances", None).await;
    commit(&pool, &schema, vault::CONFIG, &decoded).await;
    let live = tail.take(2).await;
    assert_eq!(live[0].data["version"], "1003");
    assert_eq!(live[1].data["version"], "1004");
    assert_eq!(live[1].data["op"], "update");
    assert_eq!(live[1].data["row"]["balance"], "703");
    assert!(
        live.iter().all(|e| e.data["table"] == "balances"),
        "filtered"
    );

    // The replaying stream sees them too, in commit order.
    let more = all
        .take(outbox_ids(&pool, &schema).await.len() - ids.len())
        .await;
    assert!(
        more.iter()
            .all(|e| e.id.as_deref() > Some(ids.last().unwrap().as_str()))
    );

    // Reconnecting with Last-Event-ID resumes after it.
    let all_ids = outbox_ids(&pool, &schema).await;
    let (_, mut resumed) = Events::open(&app, "/v1/changes", Some(&all_ids[1])).await;
    let next = resumed.next().await.unwrap();
    assert_eq!(next.id.as_deref(), Some(all_ids[2].as_str()));

    Store::reset(&pool, &schema).await.unwrap();
}

#[tokio::test]
async fn a_swapped_in_rebuild_resets_the_stream() {
    let Some(pool) = pool().await else { return };
    let schema = fresh("swap");
    let shadow = shadow_name(&schema).unwrap();
    Store::reset(&pool, &schema).await.unwrap();
    Store::reset(&pool, &shadow).await.unwrap();
    let decoded = workload(&[
        Op::Deposit { user: 1, amount: 7 },
        Op::Deposit { user: 1, amount: 8 },
    ]);
    commit(&pool, &schema, vault::CONFIG, &decoded[..1]).await;
    let app = router(Feed::start(pool.clone(), &schema).await.unwrap());
    let (_, mut events) = Events::open(&app, "/v1/changes", None).await;

    // A rebuild under a changed rule, swapped in.
    let changed = vault::CONFIG.replace("deposits + 1", "deposits + 2");
    commit(&pool, &shadow, &changed, &decoded[..1]).await;
    Store::swap(&pool, &schema, &shadow).await.unwrap();
    let reset = events.next().await.unwrap();
    assert_eq!(reset.event, "reset");
    assert!(reset.data["fingerprint"].is_string());

    // Then the new build's changes, as they're committed.
    commit(&pool, &schema, &changed, &decoded).await;
    let change = events.next().await.unwrap();
    assert_eq!(change.event, "change");
    assert_eq!(change.data["version"], "1002");
    assert_eq!(change.data["row"]["deposits"], "4");

    Store::reset(&pool, &schema).await.unwrap();
}

#[tokio::test]
async fn a_bad_position_is_refused() {
    let Some(pool) = pool().await else { return };
    let app = router(Feed::start(pool.clone(), fresh("bad")).await.unwrap());
    let (status, _) = Events::open(&app, "/v1/changes?after=soon", None).await;
    assert_eq!(status, StatusCode::BAD_REQUEST);
}
