//! Move enum values from a real mainnet transaction, stored field by field: the perp
//! DEX's versioned events (`V1`, and `TradeEvent`'s `V2`) land in typed columns.
//!
//! Needs `NINEVEH_TEST_DATABASE_URL`; skips without it, except in CI.

#![allow(
    clippy::unwrap_used,
    clippy::panic,
    clippy::print_stderr,
    reason = "test-only crate: helpers panic on unexpected results and note skips"
)]

use std::fs;
use std::path::{Path, PathBuf};

use nineveh_config::parse;
use nineveh_decode::{LockBuilder, ModuleAbi, TransactionDecoder};
use nineveh_engine::{Engine, FoldError};
use nineveh_proto::transaction::Transaction;
use nineveh_store::{Loaded, Store};
use prost::Message;
use sqlx::PgPool;
use sqlx::postgres::PgPoolOptions;

const PERP: &str = "0x50ead22afd6ffd9769e3b3d6e0e64a2a350d68e8b102c4e72e33d0b8cfdfdb06";
/// A mainnet transaction with two `TradeEvent`s and two `PositionUpdateEvent`s.
const FIXTURE: &str = "mainnet-7205731421";

fn fixtures() -> PathBuf {
    Path::new(env!("CARGO_MANIFEST_DIR")).join("../../fixtures")
}

async fn pool() -> Option<PgPool> {
    let Ok(url) = std::env::var("NINEVEH_TEST_DATABASE_URL") else {
        assert!(
            std::env::var_os("CI").is_none(),
            "CI must set NINEVEH_TEST_DATABASE_URL"
        );
        eprintln!("skipping: set NINEVEH_TEST_DATABASE_URL to run the store's Postgres tests");
        return None;
    };
    Some(
        PgPoolOptions::new()
            .max_connections(4)
            .connect(&url)
            .await
            .unwrap(),
    )
}

#[tokio::test]
async fn enum_events_land_in_typed_columns() {
    let Some(pool) = pool().await else { return };
    let yaml = format!(
        "\
name: perp
network: mainnet
sources:
  trades:    {{ event: {PERP}::perp_positions::TradeEvent }}
  positions: {{ event: {PERP}::perp_positions::PositionUpdateEvent }}
state:
  trade_log:    {{ log: trades }}
  position_log: {{ log: positions }}
"
    );
    let config = parse(&yaml).unwrap();
    let mut builder = LockBuilder::new(config.network);
    for entry in fs::read_dir(fixtures().join("abi/mainnet")).unwrap() {
        let text = fs::read_to_string(entry.unwrap().path()).unwrap();
        builder.add_module(serde_json::from_str::<ModuleAbi>(&text).unwrap());
    }
    let lock = builder.build(&config.roots()).unwrap();
    let project = config
        .resolve(&lock)
        .unwrap_or_else(|d| panic!("{}", d.render("nineveh.yaml", &yaml)));

    let bytes = fs::read(fixtures().join(format!("mainnet/{FIXTURE}.pb"))).unwrap();
    let tx = Transaction::decode(bytes.as_slice()).unwrap();
    let decoded = TransactionDecoder::new(&lock, project.selection())
        .decode(&tx)
        .unwrap();

    let schema = format!("enums_{}", std::process::id());
    Store::reset(&pool, &schema).await.unwrap();
    let mut store = Store::open(pool.clone(), &schema, &project, &lock)
        .await
        .unwrap();
    let engine = Engine::new(&project);
    let mut cache = Loaded::default();
    let changes = loop {
        match engine.fold(&cache, std::slice::from_ref(&decoded)) {
            Ok(changes) => break changes,
            Err(FoldError::NotLoaded(keys)) => cache.merge(store.load(keys).await.unwrap()),
            Err(e) => panic!("{e}"),
        }
    };
    store.commit(&changes).await.unwrap();

    // Typed columns, not one JSON blob: the variant, addresses, exact integers.
    let trades: Vec<(String, String, String, String, Option<String>)> = sqlx::query_as(&format!(
        "SELECT _variant, account::text, size::text, pg_typeof(size)::text, \
                counter_party_account::text
         FROM {schema}.trade_log ORDER BY event_index"
    ))
    .fetch_all(&pool)
    .await
    .unwrap();
    assert_eq!(trades.len(), 2, "{trades:?}");
    for (variant, account, size, size_type, counter_party) in &trades {
        assert!(variant == "V1" || variant == "V2", "{variant}");
        assert!(
            account.starts_with("0x") && account.len() == 66,
            "{account}"
        );
        assert!(size.parse::<u64>().unwrap() > 0);
        assert_eq!(size_type, "numeric");
        // Only V2 has a counterparty.
        assert_eq!(counter_party.is_some(), variant == "V2", "{variant}");
    }

    let positions: Vec<(String, bool)> = sqlx::query_as(&format!(
        "SELECT \"user\"::text, is_long FROM {schema}.position_log ORDER BY event_index"
    ))
    .fetch_all(&pool)
    .await
    .unwrap();
    assert_eq!(positions.len(), 2);

    // The change feed carries the same shape.
    let row: serde_json::Value = sqlx::query_scalar(
        "SELECT new_row FROM nineveh.changes WHERE schema_name = $1 AND table_name = 'trade_log'
         ORDER BY version, seq LIMIT 1",
    )
    .bind(&schema)
    .fetch_one(&pool)
    .await
    .unwrap();
    assert!(
        row.get("account").is_some() && row.get("value").is_none(),
        "{row}"
    );

    Store::reset(&pool, &schema).await.unwrap();
}
