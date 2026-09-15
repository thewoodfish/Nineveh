//! The API over a real Postgres, with state built by the store from the vault workload.
//!
//! Needs `NINEVEH_TEST_DATABASE_URL`, like the store's tests.

#![allow(
    clippy::unwrap_used,
    clippy::panic,
    clippy::print_stderr,
    reason = "test-only crate: helpers panic on unexpected results and note skips"
)]

use std::sync::Arc;
use std::sync::atomic::{AtomicU32, Ordering};

use axum::Router;
use axum::body::{Body, to_bytes};
use axum::http::{Request, StatusCode};
use nineveh_api::{Api, router};
use nineveh_engine::{Engine, FoldError};
use nineveh_store::{Loaded, Store};
use nineveh_testkit::vault::{self, Op};
use serde_json::{Value, json};
use sqlx::PgPool;
use tokio::sync::watch;
use tower::ServiceExt;

async fn pool() -> Option<PgPool> {
    let Ok(url) = std::env::var("NINEVEH_TEST_DATABASE_URL") else {
        assert!(
            std::env::var_os("CI").is_none(),
            "CI must set NINEVEH_TEST_DATABASE_URL"
        );
        eprintln!("skipping: set NINEVEH_TEST_DATABASE_URL to run the API's tests");
        return None;
    };
    Some(PgPool::connect(&url).await.unwrap())
}

/// The vault built through the store into a fresh schema, and the API over it.
async fn serve(pool: &PgPool, ops: &[Op]) -> (Router, String) {
    static NEXT: AtomicU32 = AtomicU32::new(0);
    let schema = format!(
        "api_{}_{}",
        std::process::id(),
        NEXT.fetch_add(1, Ordering::Relaxed)
    );
    Store::reset(pool, &schema).await.unwrap();
    let (lock, project) = vault::project();
    let (txs, _) = vault::transactions(ops);
    let decoded = vault::decode(&lock, &project, &txs);
    let engine = Engine::new(&project);
    let mut store = Store::open(pool.clone(), &schema, &project, &lock)
        .await
        .unwrap();
    for tx in decoded.chunks(1) {
        let mut cache = Loaded::default();
        let changes = loop {
            match engine.fold(&cache, tx) {
                Ok(changes) => break changes,
                Err(FoldError::NotLoaded(keys)) => cache.merge(store.load(keys).await.unwrap()),
                Err(e) => panic!("{e}"),
            }
        };
        store.commit(&changes).await.unwrap();
    }
    let (_, health) = watch::channel(None);
    let api = Api::new(pool.clone(), &schema, &project, health);
    (router(Arc::new(api)), schema)
}

async fn get(app: &Router, uri: &str) -> (StatusCode, Value) {
    let response = app
        .clone()
        .oneshot(Request::get(uri).body(Body::empty()).unwrap())
        .await
        .unwrap();
    let status = response.status();
    let body = to_bytes(response.into_body(), 1 << 20).await.unwrap();
    (status, serde_json::from_slice(&body).unwrap())
}

#[tokio::test]
async fn serves_state_tables_in_the_feeds_shape() {
    let Some(pool) = pool().await else { return };
    let (app, schema) = serve(
        &pool,
        &[
            Op::Deposit {
                user: 1,
                amount: 700,
            },
            Op::Deposit { user: 2, amount: 5 },
            Op::Deposit {
                user: 1,
                amount: 50,
            },
            Op::CreateVault { vault: 0 },
        ],
    )
    .await;

    // Tables and their columns, in config order.
    let (status, tables) = get(&app, "/v1/tables").await;
    assert_eq!(status, StatusCode::OK);
    let names: Vec<&str> = tables
        .as_array()
        .unwrap()
        .iter()
        .map(|t| t["name"].as_str().unwrap())
        .collect();
    assert_eq!(
        names,
        [
            "balances",
            "vaults",
            "positions",
            "shares",
            "deposit_log",
            "sizes",
            "share_writes"
        ]
    );
    assert_eq!(
        tables[0],
        json!({
            "name": "balances", "kind": "reduce", "key": ["user"],
            "columns": [
                {"name": "user", "type": "address", "nullable": false},
                {"name": "balance", "type": "u128", "nullable": false},
                {"name": "deposits", "type": "u64", "nullable": false},
            ]
        })
    );

    // Most recently changed first; wide integers as strings, addresses full width.
    let (status, page) = get(&app, "/v1/tables/balances?count=exact").await;
    assert_eq!(status, StatusCode::OK);
    assert_eq!(page["count"], 2);
    assert_eq!(
        page["rows"][0],
        json!({
            "_version": "1003",
            "user": vault::user(1).to_string(),
            "balance": "750",
            "deposits": "2",
        })
    );

    // The same row, as the change feed carried it.
    let feed_row: Value = sqlx::query_scalar(
        "SELECT new_row FROM nineveh.changes WHERE schema_name = $1 AND table_name = 'balances'
         ORDER BY version DESC, seq DESC LIMIT 1",
    )
    .bind(&schema)
    .fetch_one(&pool)
    .await
    .unwrap();
    let mut rest_row = page["rows"][0].clone();
    rest_row.as_object_mut().unwrap().remove("_version");
    assert_eq!(rest_row, feed_row, "REST rows and feed rows have one shape");

    // Filters take short addresses; ordering and paging.
    let short = vault::user(2).to_standard_string();
    let (_, one) = get(&app, &format!("/v1/tables/balances?user={short}")).await;
    assert_eq!(one["rows"].as_array().unwrap().len(), 1);
    assert_eq!(one["rows"][0]["balance"], "5");
    let (_, ordered) = get(&app, "/v1/tables/balances?order=balance&limit=1&offset=1").await;
    assert_eq!(ordered["rows"][0]["balance"], "750");
    let (_, log) = get(&app, "/v1/tables/deposit_log?order=version").await;
    assert_eq!(log["rows"][0]["version"], "1001");
    assert_eq!(log["rows"][0]["event_index"], 0, "a u32 is a JSON number");

    // Structured values are JSON.
    let (_, vaults) = get(&app, "/v1/tables/vaults").await;
    assert_eq!(
        vaults["rows"][0]["owner"],
        nineveh_core::Address::ONE.to_string()
    );
    assert_eq!(vaults["rows"][0]["shares"]["size"], "0");

    // Status: the build's cursor; no rebuild, no pipeline in this process.
    let (_, status) = get(&app, "/v1/status").await;
    assert_eq!(status["project"], "vault");
    assert_eq!(status["build"]["cursor"], "1004");
    assert_eq!(status["rebuild"], Value::Null);
    assert_eq!(status["pipeline"], Value::Null);

    Store::reset(&pool, &schema).await.unwrap();
}

#[tokio::test]
async fn bad_requests_say_what_is_wrong() {
    let Some(pool) = pool().await else { return };
    let (app, schema) = serve(&pool, &[Op::Deposit { user: 1, amount: 7 }]).await;
    let cases = [
        ("/v1/tables/nope", StatusCode::NOT_FOUND, "no state table"),
        (
            "/v1/tables/balances?color=red",
            StatusCode::BAD_REQUEST,
            "no column `color`",
        ),
        (
            "/v1/tables/balances?deposits=many",
            StatusCode::BAD_REQUEST,
            "numeric",
        ),
        (
            "/v1/tables/balances?user=0xzz",
            StatusCode::BAD_REQUEST,
            "isn't an address",
        ),
        (
            "/v1/tables/balances?order=nope",
            StatusCode::BAD_REQUEST,
            "order by",
        ),
        (
            "/v1/tables/balances?limit=-1",
            StatusCode::BAD_REQUEST,
            "non-negative",
        ),
        (
            "/v1/tables/vaults?shares=1",
            StatusCode::BAD_REQUEST,
            "JSON",
        ),
    ];
    for (uri, expected, message) in cases {
        let (status, body) = get(&app, uri).await;
        assert_eq!(status, expected, "{uri}: {body}");
        let error = body["error"].as_str().unwrap();
        assert!(error.contains(message), "{uri}: {error}");
    }
    Store::reset(&pool, &schema).await.unwrap();
}
