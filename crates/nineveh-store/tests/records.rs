//! The record log against a real Postgres (ADR 0022).
//!
//! Needs `NINEVEH_TEST_DATABASE_URL`; skips without it, except in CI.
//!
//! The point of these is the round trip. A rebuild replays what's in here instead of
//! re-reading the chain, so a record that changes on its way through Postgres is a
//! rebuild that quietly produces different rows than the stream did.

#![allow(
    clippy::unwrap_used,
    clippy::panic,
    clippy::print_stderr,
    reason = "test-only crate: helpers panic on unexpected results and note skips"
)]

use nineveh_core::{Address, Identifier, U256, Value, Version};
use nineveh_decode::{Container, Origin, Record, RecordData, SourceId, StoredData, StoredRecord};
use nineveh_store::records::{self, Logged};
use sqlx::PgPool;
use sqlx::postgres::PgPoolOptions;
use std::str::FromStr as _;

async fn pool() -> Option<PgPool> {
    let Ok(url) = std::env::var("NINEVEH_TEST_DATABASE_URL") else {
        assert!(
            std::env::var_os("CI").is_none(),
            "CI must set NINEVEH_TEST_DATABASE_URL"
        );
        eprintln!("skipping: set NINEVEH_TEST_DATABASE_URL to run the record log tests");
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

/// A project name no other test run collides with.
fn project(n: u32) -> String {
    format!("records_{}_{n}", std::process::id())
}

fn ident(s: &str) -> Identifier {
    Identifier::from_str(s).unwrap()
}

/// A record of every kind the decoder emits, with the awkward values in it: a u256 at
/// its maximum, an empty byte string, a `None`, a nested struct.
fn every_kind() -> Vec<Record> {
    vec![
        Record {
            source: SourceId(0),
            origin: Origin::Event(0),
            data: RecordData::Event {
                ty: "0x1::market::Sold".parse().unwrap(),
                account: Address::special(1),
                creation_number: 4,
                sequence_number: u64::MAX,
                value: Value::Struct(vec![
                    (ident("price"), Value::U256(U256::MAX)),
                    (ident("memo"), Value::Option(None)),
                    (ident("raw"), Value::Bytes(Vec::new())),
                ]),
            },
        },
        Record {
            source: SourceId(0),
            origin: Origin::Change(1),
            data: RecordData::ResourceWrite {
                address: Address::special(3),
                ty: "0x1::vault::Vault".parse().unwrap(),
                value: Value::Vector(vec![Value::I128(i128::MIN)]),
            },
        },
        Record {
            source: SourceId(0),
            origin: Origin::Change(2),
            data: RecordData::TableWrite {
                container: Container::SmartTable,
                handle: Address::special(9),
                key: Value::U64(7),
                value: Value::String("✨".to_owned()),
            },
        },
        Record {
            source: SourceId(0),
            origin: Origin::Change(3),
            data: RecordData::TableDelete {
                container: Container::BigOrderedMap,
                handle: Address::special(9),
                key: Value::Address(Address::special(2)),
            },
        },
    ]
}

/// Everything on one version, so ordering by `ord` is what's under test.
fn logged(records: &[Record]) -> Vec<Logged> {
    records
        .iter()
        .enumerate()
        .map(|(i, record)| Logged {
            version: Version::new(100),
            ord: i32::try_from(i).unwrap(),
            timestamp_micros: 1_789_774_040_929_309,
            success: true,
            sender: Some(Address::special(1).to_string()),
            record: StoredRecord::of(record, "sold"),
        })
        .collect()
}

#[tokio::test]
async fn records_survive_the_database_unchanged() {
    let Some(pool) = pool().await else { return };
    let name = project(1);
    let originals = every_kind();

    records::append(
        &pool,
        &name,
        "lock-a",
        &["sold".to_owned()],
        &logged(&originals),
        Version::new(100),
    )
    .await
    .unwrap();

    let read = records::read(&pool, &name, Version::new(0), Version::new(200))
        .await
        .unwrap();
    assert_eq!(read.len(), originals.len(), "every record comes back");

    for (got, original) in read.iter().zip(&originals) {
        assert_eq!(
            got.record.source, "sold",
            "the source is named, not numbered"
        );
        let back = Record::from_stored(got.record.clone(), original.source).unwrap();
        assert_eq!(&back, original, "a record changed inside Postgres");
    }

    let state = records::state(&pool, &name).await.unwrap().unwrap();
    assert_eq!(state.cursor, Some(Version::new(100)));
    assert_eq!(state.lock_hash, "lock-a");
    assert_eq!(
        state.sources,
        vec!["sold".to_owned()],
        "the log records what it covers"
    );

    records::forget(&pool, &name).await.unwrap();
}

/// The writer restarts from its cursor without checking what it already wrote, so
/// re-appending a version has to overwrite rather than duplicate or fail.
#[tokio::test]
async fn appending_a_version_twice_is_idempotent() {
    let Some(pool) = pool().await else { return };
    let name = project(2);
    let originals = every_kind();
    let rows = logged(&originals);

    for _ in 0..2 {
        records::append(
            &pool,
            &name,
            "lock-a",
            &["sold".to_owned()],
            &rows,
            Version::new(100),
        )
        .await
        .unwrap();
    }

    let read = records::read(&pool, &name, Version::new(0), Version::new(200))
        .await
        .unwrap();
    assert_eq!(
        read.len(),
        originals.len(),
        "re-appending must not duplicate"
    );

    records::forget(&pool, &name).await.unwrap();
}

/// The log is keyed by project, so one project's records are invisible to another —
/// and `forget` takes only its own.
#[tokio::test]
async fn projects_do_not_see_each_others_records() {
    let Some(pool) = pool().await else { return };
    let (a, b) = (project(3), project(4));
    let rows = logged(&every_kind());
    records::append(
        &pool,
        &a,
        "lock-a",
        &["sold".to_owned()],
        &rows,
        Version::new(100),
    )
    .await
    .unwrap();
    records::append(
        &pool,
        &b,
        "lock-b",
        &["sold".to_owned()],
        &rows,
        Version::new(100),
    )
    .await
    .unwrap();

    records::forget(&pool, &a).await.unwrap();
    assert!(
        records::read(&pool, &a, Version::new(0), Version::new(200))
            .await
            .unwrap()
            .is_empty(),
        "forgetting a project empties it"
    );
    assert_eq!(
        records::read(&pool, &b, Version::new(0), Version::new(200))
            .await
            .unwrap()
            .len(),
        4,
        "and leaves everyone else alone"
    );
    assert!(records::state(&pool, &a).await.unwrap().is_none());

    let (count, bytes) = records::usage(&pool, &b).await.unwrap();
    assert_eq!(count, 4);
    assert!(
        bytes > 0,
        "usage reports the bytes a tier would be metered on"
    );

    records::forget(&pool, &b).await.unwrap();
}

/// A row written by an encoder we no longer understand must halt one project with a
/// located error, not take the process down.
#[tokio::test]
async fn a_corrupt_row_is_an_error_not_a_panic() {
    let Some(pool) = pool().await else { return };
    let name = project(5);
    let mut rows = logged(&every_kind());
    rows.truncate(1);
    rows[0].record.data = StoredData::ResourceDelete {
        address: "not an address".to_owned(),
        ty: "0x1::a::B".to_owned(),
    };
    records::append(
        &pool,
        &name,
        "lock-a",
        &["sold".to_owned()],
        &rows,
        Version::new(100),
    )
    .await
    .unwrap();

    let read = records::read(&pool, &name, Version::new(0), Version::new(200))
        .await
        .unwrap();
    assert!(
        Record::from_stored(read[0].record.clone(), SourceId(0)).is_err(),
        "a bad address is reported, not unwrapped"
    );

    records::forget(&pool, &name).await.unwrap();
}
