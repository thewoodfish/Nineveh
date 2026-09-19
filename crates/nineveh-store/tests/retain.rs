//! Pruning, and the two floors that stop it destroying something (ADR 0024).
//!
//! Needs `NINEVEH_TEST_DATABASE_URL`; skips without it, except in CI.

#![allow(
    clippy::unwrap_used,
    clippy::panic,
    clippy::print_stderr,
    reason = "test-only crate: helpers panic on unexpected results and note skips"
)]

use nineveh_core::{Address, Value, Version};
use nineveh_decode::{Origin, Record, RecordData, SourceId, StoredRecord};
use nineveh_store::records::{self, Logged};
use nineveh_store::{retain, webhooks};
use sqlx::PgPool;
use sqlx::postgres::PgPoolOptions;

async fn pool() -> Option<PgPool> {
    let Ok(url) = std::env::var("NINEVEH_TEST_DATABASE_URL") else {
        assert!(
            std::env::var_os("CI").is_none(),
            "CI must set NINEVEH_TEST_DATABASE_URL"
        );
        eprintln!("skipping: set NINEVEH_TEST_DATABASE_URL to run the retention tests");
        return None;
    };
    let pool = PgPoolOptions::new()
        .max_connections(4)
        .connect(&url)
        .await
        .unwrap();
    nineveh_store::migrate(&pool).await.unwrap();
    Some(pool)
}

fn name(n: u32) -> String {
    format!("retain_{}_{n}", std::process::id())
}

/// Records big enough that a byte limit bites at a predictable count.
fn logged(versions: std::ops::Range<u64>) -> Vec<Logged> {
    versions
        .map(|v| Logged {
            version: Version::new(v),
            ord: 0,
            timestamp_micros: 1_789_774_040_929_309,
            success: true,
            sender: None,
            record: StoredRecord::of(
                &Record {
                    source: SourceId(0),
                    origin: Origin::Event(0),
                    data: RecordData::Event {
                        ty: "0x1::market::Sold".parse().unwrap(),
                        account: Address::special(1),
                        creation_number: 1,
                        sequence_number: v,
                        value: Value::String("x".repeat(200)),
                    },
                },
                "sold",
            ),
        })
        .collect()
}

/// The log gives up its oldest records to stay within its allowance.
#[tokio::test]
async fn the_record_log_is_pruned_oldest_first_to_fit() {
    let Some(pool) = pool().await else { return };
    let project = name(1);
    records::forget(&pool, &project).await.unwrap();
    let rows = logged(100..200);
    records::append(
        &pool,
        &project,
        "lock",
        &["sold".to_owned()],
        &rows,
        Version::new(199),
    )
    .await
    .unwrap();

    let (count, bytes) = records::usage(&pool, &project).await.unwrap();
    assert_eq!(count, 100);

    // Everything is folded, so everything may be taken; ask for about half.
    let taken = retain::prune_records(&pool, &project, bytes / 2, Some(Version::new(199)))
        .await
        .unwrap();
    assert!(taken > 0, "something was over the limit and had to go");

    let (left, now) = records::usage(&pool, &project).await.unwrap();
    assert!(
        now <= bytes / 2 + bytes / count,
        "pruned to about the limit"
    );
    assert!(
        left > 0 && left < count,
        "some went, not all: {left} of {count}"
    );

    let earliest = records::first_version(&pool, &project)
        .await
        .unwrap()
        .unwrap();
    assert!(
        earliest > Version::new(100),
        "the oldest went first, not the newest: log starts at {earliest}"
    );

    records::forget(&pool, &project).await.unwrap();
}

/// The floor that matters: a record the fold hasn't consumed is an input that only the
/// chain still has, so pruning may never reach it however far over quota the log is.
#[tokio::test]
async fn records_above_the_fold_cursor_are_never_pruned() {
    let Some(pool) = pool().await else { return };
    let project = name(2);
    records::forget(&pool, &project).await.unwrap();
    records::append(
        &pool,
        &project,
        "lock",
        &["sold".to_owned()],
        &logged(100..200),
        Version::new(199),
    )
    .await
    .unwrap();

    // A project that has folded nothing keeps everything, however small the allowance.
    assert_eq!(
        retain::prune_records(&pool, &project, 0, None)
            .await
            .unwrap(),
        0,
        "nothing folded, so nothing may be taken"
    );
    assert_eq!(records::usage(&pool, &project).await.unwrap().0, 100);

    // One that folded half keeps the other half, even asked to prune to nothing.
    retain::prune_records(&pool, &project, 0, Some(Version::new(149)))
        .await
        .unwrap();
    let (left, _) = records::usage(&pool, &project).await.unwrap();
    assert_eq!(left, 50, "the unfolded half survives: {left} left");
    assert_eq!(
        records::first_version(&pool, &project).await.unwrap(),
        Some(Version::new(150)),
        "and what survives is exactly what the fold hasn't reached"
    );

    records::forget(&pool, &project).await.unwrap();
}

/// An endpoint that is behind holds the outbox open: its deliveries are still owed, and
/// deleting them would turn at-least-once into at-most-once with nothing to show for it.
#[tokio::test]
async fn the_outbox_is_not_pruned_past_a_webhook_that_is_behind() {
    let Some(pool) = pool().await else { return };
    let schema = name(3);
    sqlx::query(
        "INSERT INTO nineveh.projects (schema_name, project, network, fingerprint)
                 VALUES ($1, $1, 'testnet', 'x') ON CONFLICT (schema_name) DO NOTHING",
    )
    .bind(&schema)
    .execute(&pool)
    .await
    .unwrap();
    for version in 1..=10_i64 {
        sqlx::query(
            "INSERT INTO nineveh.changes
                 (schema_name, version, seq, table_name, op, key, new_row, committed_at)
             VALUES ($1, $2, 0, 't', 'insert', '{}'::jsonb, NULL, now() - interval '30 days')",
        )
        .bind(&schema)
        .bind(version)
        .execute(&pool)
        .await
        .unwrap();
    }

    let left = |pool: PgPool, schema: String| async move {
        sqlx::query_scalar::<_, i64>("SELECT count(*) FROM nineveh.changes WHERE schema_name = $1")
            .bind(schema)
            .fetch_one(&pool)
            .await
            .unwrap()
    };

    // An endpoint that has delivered through version 4 pins everything after it.
    webhooks::ensure(&pool, &schema, "slow").await.unwrap();
    webhooks::delivered(&pool, &schema, "slow", 4, 0)
        .await
        .unwrap();
    retain::prune_changes(&pool, &schema, 7).await.unwrap();
    assert_eq!(
        left(pool.clone(), schema.clone()).await,
        6,
        "only what the slow endpoint has already had is taken"
    );

    // Once it catches up, the rest goes.
    webhooks::delivered(&pool, &schema, "slow", 10, 0)
        .await
        .unwrap();
    retain::prune_changes(&pool, &schema, 7).await.unwrap();
    assert_eq!(
        left(pool.clone(), schema.clone()).await,
        0,
        "a caught-up endpoint holds nothing open"
    );

    sqlx::query("DELETE FROM nineveh.projects WHERE schema_name = $1")
        .bind(&schema)
        .execute(&pool)
        .await
        .unwrap();
}

/// Rows inside the window stay, however far along every reader is.
#[tokio::test]
async fn recent_changes_are_kept() {
    let Some(pool) = pool().await else { return };
    let schema = name(4);
    sqlx::query(
        "INSERT INTO nineveh.projects (schema_name, project, network, fingerprint)
                 VALUES ($1, $1, 'testnet', 'x') ON CONFLICT (schema_name) DO NOTHING",
    )
    .bind(&schema)
    .execute(&pool)
    .await
    .unwrap();
    sqlx::query(
        "INSERT INTO nineveh.changes (schema_name, version, seq, table_name, op, key)
         VALUES ($1, 1, 0, 't', 'insert', '{}'::jsonb)",
    )
    .bind(&schema)
    .execute(&pool)
    .await
    .unwrap();

    assert_eq!(
        retain::prune_changes(&pool, &schema, 7).await.unwrap(),
        0,
        "a change committed a moment ago is inside any window"
    );

    sqlx::query("DELETE FROM nineveh.projects WHERE schema_name = $1")
        .bind(&schema)
        .execute(&pool)
        .await
        .unwrap();
}
