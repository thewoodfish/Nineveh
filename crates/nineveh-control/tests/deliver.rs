//! Webhook delivery end to end (ADR 0020): a real receiver on localhost, a real
//! change outbox, and a real sender between them.
//!
//! Needs `NINEVEH_TEST_DATABASE_URL`; skips without it, except in CI.

#![allow(
    clippy::unwrap_used,
    clippy::panic,
    clippy::print_stderr,
    reason = "test-only crate: helpers panic on unexpected results and note skips"
)]

use std::net::SocketAddr;
use std::sync::{Arc, Mutex, PoisonError};
use std::time::Duration;

use axum::Router;
use axum::extract::State;
use axum::http::{HeaderMap, StatusCode};
use axum::routing::post;
use hmac::{Hmac, Mac};
use nineveh_config::parse;
use nineveh_control::Deliveries;
use nineveh_store::webhooks;
use serde_json::Value;
use sha2::Sha256;
use sqlx::PgPool;

mod common;

use common::{logs, pool};

/// What a receiver kept, and how it should answer.
#[derive(Default)]
struct Received {
    deliveries: Vec<(HeaderMap, Value, String)>,
    /// Answer this many requests with a 500 before accepting any.
    refuse: usize,
}

type Shared = Arc<Mutex<Received>>;

fn held(shared: &Shared) -> std::sync::MutexGuard<'_, Received> {
    shared.lock().unwrap_or_else(PoisonError::into_inner)
}

/// A receiver on a port of its own, and what it has been sent.
async fn receiver(refuse: usize) -> (String, Shared) {
    let shared: Shared = Arc::new(Mutex::new(Received {
        refuse,
        ..Received::default()
    }));
    let app = Router::new()
        .route(
            "/hook",
            post(
                |State(state): State<Shared>, headers: HeaderMap, body: String| async move {
                    let mut held = held(&state);
                    if held.refuse > 0 {
                        held.refuse -= 1;
                        held.deliveries.push((
                            headers,
                            serde_json::from_str(&body).unwrap_or(Value::Null),
                            body,
                        ));
                        return StatusCode::INTERNAL_SERVER_ERROR;
                    }
                    held.deliveries.push((
                        headers,
                        serde_json::from_str(&body).unwrap_or(Value::Null),
                        body,
                    ));
                    StatusCode::OK
                },
            ),
        )
        .with_state(Arc::clone(&shared));
    let listener = tokio::net::TcpListener::bind(SocketAddr::from(([127, 0, 0, 1], 0)))
        .await
        .unwrap();
    let port = listener.local_addr().unwrap().port();
    tokio::spawn(async move {
        let _ = axum::serve(listener, app).await;
    });
    (format!("http://localhost:{port}/hook"), shared)
}

/// Put a change in the outbox, as a commit would.
async fn change(pool: &PgPool, schema: &str, version: i64, seq: i32, table: &str, op: &str) {
    sqlx::query(
        "INSERT INTO nineveh.changes (schema_name, version, seq, table_name, op, key, new_row)
         VALUES ($1, $2, $3, $4, $5, $6::jsonb, $7::jsonb)",
    )
    .bind(schema)
    .bind(version)
    .bind(seq)
    .bind(table)
    .bind(op)
    .bind(format!(r#"{{"user":"0x{version}"}}"#))
    .bind(format!(r#"{{"user":"0x{version}","balance":"{seq}"}}"#))
    .execute(pool)
    .await
    .unwrap();
}

/// A project row, which the outbox's rows point at.
async fn project(pool: &PgPool, schema: &str) {
    sqlx::query(
        "INSERT INTO nineveh.projects (schema_name, project, network, fingerprint)
         VALUES ($1, $1, 'testnet', 'test') ON CONFLICT (schema_name) DO NOTHING",
    )
    .bind(schema)
    .execute(pool)
    .await
    .unwrap();
}

fn config(url: &str) -> String {
    format!(
        "\
name: hooked
network: testnet
sources:
  deposits: {{ event: 0xcafe::vault::DepositEvent }}
state:
  balances:
    key: [user]
    columns: {{ user: address, balance: {{ type: u64, default: 0 }} }}
    reduce:
      - {{ on: deposits, set: {{ balance: \"balance + 1\" }} }}
  audit_log: {{ log: deposits }}
webhooks:
  my_backend:
    url: {url}
    on: [balances.changed]
"
    )
}

/// Wait until `check` holds, or fail after a few seconds.
async fn until(shared: &Shared, what: &str, check: impl Fn(&Received) -> bool) {
    for _ in 0..100 {
        if check(&held(shared)) {
            return;
        }
        tokio::time::sleep(Duration::from_millis(100)).await;
    }
    panic!("{what} never happened");
}

#[tokio::test]
async fn changes_are_delivered_signed_batched_and_retried() {
    logs();
    let Some(pool) = pool("deliver").await else {
        return;
    };
    let schema = "hooked";
    project(&pool, schema).await;

    // A change from before the endpoint existed: configuring a webhook doesn't
    // deliver the project's history.
    change(&pool, schema, 1000, 0, "balances", "insert").await;

    let (url, received) = receiver(0).await;
    let config = parse(&config(&url)).unwrap();
    let hooks = config.webhooks.clone();
    let sender = Deliveries::start(&pool, schema, &hooks, None);

    // Wait until the endpoint has taken its place at the end of the feed.
    for _ in 0..100 {
        let endpoints = webhooks::list(&pool, schema).await.unwrap();
        if endpoints.first().and_then(|e| e.cursor).is_some() {
            break;
        }
        tokio::time::sleep(Duration::from_millis(50)).await;
    }
    let secret = webhooks::list(&pool, schema).await.unwrap()[0]
        .secret
        .clone();

    // Two changes to a table it follows, and one to a table it doesn't.
    change(&pool, schema, 1001, 0, "balances", "insert").await;
    change(&pool, schema, 1001, 1, "audit_log", "insert").await;
    change(&pool, schema, 1002, 0, "balances", "update").await;

    until(&received, "a delivery", |r| !r.deliveries.is_empty()).await;
    until(&received, "every change", |r| {
        r.deliveries
            .iter()
            .flat_map(|(_, body, _)| body["changes"].as_array().cloned().unwrap_or_default())
            .count()
            >= 2
    })
    .await;

    let (headers, body, raw) = held(&received).deliveries[0].clone();
    assert_eq!(body["project"], "hooked");
    assert_eq!(body["endpoint"], "my_backend");

    // Only the table it asked for, and never the older change.
    let delivered: Vec<(String, String)> = held(&received)
        .deliveries
        .iter()
        .flat_map(|(_, body, _)| body["changes"].as_array().cloned().unwrap_or_default())
        .map(|c| {
            (
                c["table"].as_str().unwrap_or_default().to_owned(),
                c["version"].as_str().unwrap_or_default().to_owned(),
            )
        })
        .collect();
    assert!(
        delivered.iter().all(|(table, _)| table == "balances"),
        "only the followed table: {delivered:?}"
    );
    assert!(
        !delivered.iter().any(|(_, version)| version == "1000"),
        "nothing from before the endpoint existed: {delivered:?}"
    );

    // The row rides along by default, and the key always does.
    let first = &body["changes"][0];
    assert_eq!(first["key"]["user"], "0x1001");
    assert_eq!(first["row"]["balance"], "0");

    // Signed over the timestamp and the body, with the endpoint's own secret.
    let signature = headers["x-nineveh-signature"].to_str().unwrap().to_owned();
    let (t, v1) = signature.split_once(',').unwrap();
    let timestamp = t.strip_prefix("t=").unwrap();
    let mut mac = <Hmac<Sha256> as Mac>::new_from_slice(secret.as_bytes()).unwrap();
    mac.update(format!("{timestamp}.{raw}").as_bytes());
    let mut expected = String::new();
    for byte in mac.finalize().into_bytes() {
        use std::fmt::Write as _;
        write!(expected, "{byte:02x}").unwrap();
    }
    assert_eq!(v1, format!("v1={expected}"), "the signature checks out");

    // The delivery names the position it covers, for a receiver to deduplicate on.
    assert!(
        headers["x-nineveh-delivery"]
            .to_str()
            .unwrap()
            .contains('.'),
        "a position, as `version.seq`"
    );

    // The endpoint's place is recorded, so a restart doesn't repeat it.
    until(&received, "the cursor to catch up", |_| true).await;
    for _ in 0..100 {
        let at = webhooks::list(&pool, schema).await.unwrap()[0].cursor;
        if at == Some((1002, 0)) {
            break;
        }
        tokio::time::sleep(Duration::from_millis(50)).await;
    }
    assert_eq!(
        webhooks::list(&pool, schema).await.unwrap()[0].cursor,
        Some((1002, 0)),
        "delivered up to the newest change"
    );
    sender.stop().await;
}

#[tokio::test]
async fn a_receiver_that_fails_is_retried_until_it_recovers() {
    logs();
    let Some(pool) = pool("deliver_retry").await else {
        return;
    };
    let schema = "retried";
    project(&pool, schema).await;
    // Refuse the first two attempts, then accept.
    let (url, received) = receiver(2).await;
    let config = parse(&config(&url).replace("name: hooked", "name: retried")).unwrap();
    let sender = Deliveries::start(&pool, schema, &config.webhooks, None);
    for _ in 0..100 {
        if webhooks::list(&pool, schema)
            .await
            .unwrap()
            .first()
            .and_then(|e| e.cursor)
            .is_some()
        {
            break;
        }
        tokio::time::sleep(Duration::from_millis(50)).await;
    }
    change(&pool, schema, 2001, 0, "balances", "insert").await;

    // It keeps trying, and the failures are recorded while it does.
    until(&received, "a retry", |r| r.deliveries.len() >= 2).await;
    let failing = webhooks::list(&pool, schema).await.unwrap();
    assert!(failing[0].failures > 0, "failures are counted");
    assert!(
        failing[0]
            .last_error
            .as_deref()
            .is_some_and(|e| e.contains("500")),
        "and say what happened: {:?}",
        failing[0].last_error
    );

    // When the receiver recovers, the change arrives and the endpoint is healthy.
    until(&received, "the delivery to land", |r| {
        r.deliveries.len() >= 3
    })
    .await;
    for _ in 0..100 {
        let endpoint = webhooks::list(&pool, schema).await.unwrap();
        if endpoint[0].cursor == Some((2001, 0)) && endpoint[0].failures == 0 {
            break;
        }
        tokio::time::sleep(Duration::from_millis(100)).await;
    }
    let healthy = webhooks::list(&pool, schema).await.unwrap();
    assert_eq!(healthy[0].cursor, Some((2001, 0)));
    assert_eq!(healthy[0].failures, 0, "a delivery clears the failures");
    assert_eq!(healthy[0].last_error, None);
    sender.stop().await;
}
