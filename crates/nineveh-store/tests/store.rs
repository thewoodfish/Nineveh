//! The store against a real Postgres: the replay property through the database,
//! typed columns, the outbox, and the guards around the single writer and the build
//! fingerprint.
//!
//! Needs `NINEVEH_TEST_DATABASE_URL` pointing at a database it may create schemas in.
//! Without it the tests skip, except in CI, where they fail. (Not `DATABASE_URL`:
//! setting that switches sqlx's macros to checking queries against the database at
//! compile time.)

#![allow(
    clippy::unwrap_used,
    clippy::panic,
    clippy::print_stderr,
    reason = "test-only crate: helpers panic on unexpected results and note skips"
)]

use std::sync::atomic::{AtomicU32, Ordering};

use nineveh_config::{Project, parse};
use nineveh_decode::{DecodedTransaction, LockBuilder, Lockfile};
use nineveh_engine::{ChangeSet, Engine, FoldError, MemoryState, TableId};
use nineveh_store::{Loaded, NOTIFY_CHANNEL, Store, StoreError, webhooks};
use nineveh_testkit::vault::{self, Op};
use proptest::prelude::*;
use proptest::test_runner::{Config as RunnerConfig, TestRunner};
use sqlx::postgres::{PgListener, PgPoolOptions};
use sqlx::{PgPool, Row as _};

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

/// A fresh schema name, unique across test processes and threads.
async fn fresh_schema(pool: &PgPool, name: &str) -> String {
    static NEXT: AtomicU32 = AtomicU32::new(0);
    let schema = format!(
        "t_{name}_{}_{}",
        std::process::id(),
        NEXT.fetch_add(1, Ordering::Relaxed)
    );
    Store::reset(pool, &schema).await.unwrap();
    schema
}

fn project_from(yaml: &str) -> (Lockfile, Project) {
    let config = parse(yaml).unwrap_or_else(|d| panic!("{}", d.render("nineveh.yaml", yaml)));
    let mut builder = LockBuilder::new(config.network);
    for module in vault::modules() {
        builder.add_module(module);
    }
    let lock = builder.build(&config.roots()).unwrap();
    let project = config.resolve(&lock).unwrap();
    (lock, project)
}

/// Fold a batch over the store, loading whatever keys the fold asks for.
async fn fold(
    store: &Store,
    engine: &Engine<'_>,
    cache: &mut Loaded,
    batch: &[DecodedTransaction],
) -> ChangeSet {
    loop {
        match engine.fold(cache, batch) {
            Ok(changes) => return changes,
            Err(FoldError::NotLoaded(keys)) => cache.merge(store.load(keys).await.unwrap()),
            Err(e) => panic!("{e}"),
        }
    }
}

/// The vault's tables: the seven state tables, then the engine's internal ones.
fn table_ids() -> Vec<TableId> {
    (0..8)
        .map(TableId::State)
        .chain([TableId::Handles, TableId::Buckets])
        .collect()
}

const DEPOSIT_LOG: u32 = 4;

/// Every committed change in the outbox, in feed order.
async fn outbox(pool: &PgPool, schema: &str) -> Vec<(i64, i32, String, String, String, String)> {
    sqlx::query(
        "SELECT version, seq, table_name, op, key::text, coalesce(new_row::text, '')
         FROM nineveh.changes WHERE schema_name = $1 ORDER BY version, seq",
    )
    .bind(schema)
    .fetch_all(pool)
    .await
    .unwrap()
    .into_iter()
    .map(|r| (r.get(0), r.get(1), r.get(2), r.get(3), r.get(4), r.get(5)))
    .collect()
}

/// Run the workload through Postgres in batches of the given sizes (cycled). A crash
/// drops the folded batch, the store and its cache before the commit, and reopens
/// from the database.
async fn run_batched(
    pool: &PgPool,
    schema: &str,
    project: &Project,
    lock: &Lockfile,
    decoded: &[DecodedTransaction],
    sizes: &[usize],
    crashes: &[bool],
) -> Store {
    let engine = Engine::new(project);
    let mut store = Store::open(pool.clone(), schema, project, lock)
        .await
        .unwrap();
    let mut cache = Loaded::default();
    let mut committed = 0;
    let mut step = 0;
    while committed < decoded.len() {
        let size = sizes[step % sizes.len()].max(1);
        let crash = crashes.get(step).copied().unwrap_or(false);
        step += 1;
        let end = committed.saturating_add(size).min(decoded.len());
        let changes = fold(&store, &engine, &mut cache, &decoded[committed..end]).await;
        if crash {
            drop((changes, store));
            store = Store::open(pool.clone(), schema, project, lock)
                .await
                .unwrap();
            cache = Loaded::default();
            let resumed = decoded
                .iter()
                .position(|tx| Some(tx.version) > store.cursor())
                .unwrap_or(decoded.len());
            assert_eq!(resumed, committed, "reopening resumes after the cursor");
            continue;
        }
        store.commit(&changes).await.unwrap();
        cache.apply(&changes);
        committed = end;
    }
    store
}

async fn replay_case(pool: &PgPool, ops: &[Op], sizes: &[usize], crashes: &[bool]) {
    let (lock, project) = vault::project();
    let engine = Engine::new(&project);
    let (txs, model) = vault::transactions(ops);
    let decoded = vault::decode(&lock, &project, &txs);

    // The reference: one pass in memory.
    let mut memory = MemoryState::new();
    let clean = engine.fold(&memory, &decoded).unwrap();
    memory.apply(&clean);

    let batched = fresh_schema(pool, "batched").await;
    let store = run_batched(pool, &batched, &project, &lock, &decoded, sizes, crashes).await;
    assert_eq!(store.cursor(), memory.cursor());

    let mut tables = Vec::new();
    for id in table_ids() {
        let mut stored = store.scan(id).await.unwrap();
        stored.sort();
        let expected: Vec<_> = memory
            .rows(id)
            .map(|(k, r)| {
                let row = (id != TableId::State(DEPOSIT_LOG)).then(|| r.clone());
                (k.clone(), row)
            })
            .collect();
        assert_eq!(stored, expected, "{id:?} differs from the in-memory fold");
        tables.push(stored);
    }
    model.check(|table| {
        let rows = &tables[usize::try_from(table).unwrap()];
        rows.iter()
            .map(|(k, r)| (k.clone(), r.clone().unwrap_or_default()))
            .collect()
    });

    // The outbox holds the in-memory feed, whatever the batching.
    let feed = outbox(pool, &batched).await;
    let names = |t: u32| {
        project.config().state[usize::try_from(t).unwrap()]
            .name
            .name
            .clone()
    };
    let expected: Vec<(i64, String)> = clean
        .changes
        .iter()
        .map(|c| (i64::try_from(c.version.get()).unwrap(), names(c.table)))
        .collect();
    let got: Vec<(i64, String)> = feed.iter().map(|c| (c.0, c.2.clone())).collect();
    assert_eq!(got, expected, "outbox order");

    // And it's the same feed, row for row, as a single batch's.
    let single = fresh_schema(pool, "single").await;
    run_batched(pool, &single, &project, &lock, &decoded, &[usize::MAX], &[]).await;
    assert_eq!(
        feed,
        outbox(pool, &single).await,
        "outbox differs by batching"
    );

    Store::reset(pool, &batched).await.unwrap();
    Store::reset(pool, &single).await.unwrap();
}

#[test]
fn any_batching_and_any_crashes_give_the_in_memory_state() {
    let rt = tokio::runtime::Runtime::new().unwrap();
    let Some(pool) = rt.block_on(pool()) else {
        return;
    };
    let mut runner = TestRunner::new(RunnerConfig {
        cases: 16,
        failure_persistence: None,
        ..RunnerConfig::default()
    });
    let strategy = (
        proptest::collection::vec(vault::op(), 1..60),
        proptest::collection::vec(1usize..12, 1..8),
        proptest::collection::vec(any::<bool>(), 0..12),
    );
    runner
        .run(&strategy, |(ops, sizes, crashes)| {
            rt.block_on(replay_case(&pool, &ops, &sizes, &crashes));
            Ok(())
        })
        .unwrap();
}

#[tokio::test]
async fn columns_are_typed_for_the_api() {
    let Some(pool) = pool().await else { return };
    let (lock, project) = vault::project();
    let schema = fresh_schema(&pool, "typed").await;
    let (txs, model) = vault::transactions(&[
        Op::Deposit {
            user: 1,
            amount: 700,
        },
        Op::CreateVault { vault: 0 },
        Op::SetPosition {
            vault: 0,
            user: 2,
            size: 42,
        },
    ]);
    let decoded = vault::decode(&lock, &project, &txs);
    run_batched(&pool, &schema, &project, &lock, &decoded, &[2], &[]).await;

    // u128 and u64 columns are exact numerics; addresses are full-width text.
    let types: Vec<(String, String)> = sqlx::query_as(
        "SELECT attname::text, format_type(atttypid, atttypmod) FROM pg_attribute
         WHERE attrelid = $1::regclass AND attnum > 0 AND NOT attisdropped ORDER BY attnum",
    )
    .bind(format!(r#""{schema}".balances"#))
    .fetch_all(&pool)
    .await
    .unwrap();
    let types: Vec<(&str, &str)> = types
        .iter()
        .map(|(a, b)| (a.as_str(), b.as_str()))
        .collect();
    assert_eq!(
        types,
        [
            ("_key", "bytea"),
            ("_row", "bytea"),
            ("_version", "bigint"),
            ("user", "text"),
            ("balance", "numeric(39,0)"),
            ("deposits", "numeric(20,0)"),
        ]
    );
    let (user, balance, deposits, version): (String, String, String, i64) =
        sqlx::query_as(&format!(
            r#"SELECT "user", balance::text, deposits::text, _version FROM "{schema}".balances"#
        ))
        .fetch_one(&pool)
        .await
        .unwrap();
    assert_eq!(user, vault::user(1).to_string());
    assert_eq!(balance, model.balances[&vault::user(1)].0.to_string());
    assert_eq!(deposits, "1");
    assert_eq!(
        version, 1_001,
        "stamped with the version of its last change"
    );

    // A table mirror stores its value struct field by field, after its key.
    let (handle, key, size, entry): (String, String, String, String) = sqlx::query_as(&format!(
        r#"SELECT handle, key, size::text, entry::text FROM "{schema}".positions WHERE key = $1"#
    ))
    .bind(vault::user(2).to_string())
    .fetch_one(&pool)
    .await
    .unwrap();
    assert_eq!(handle, vault::positions_handle(0).to_string());
    assert_eq!(key, vault::user(2).to_string());
    assert_eq!((size.as_str(), entry.as_str()), ("42", "7"));

    // A resource mirror keeps structured fields as JSON, with integers as strings.
    let (address, owner, shares): (String, String, String) = sqlx::query_as(&format!(
        r#"SELECT address, owner, shares::text FROM "{schema}".vaults"#
    ))
    .fetch_one(&pool)
    .await
    .unwrap();
    assert_eq!(address, vault::vault_address(0).to_string());
    assert_eq!(owner, nineveh_core::Address::ONE.to_string());
    assert!(shares.contains(r#""size": "0""#), "{shares}");

    // A log has the event's version and index as its key.
    let (version, index, amount): (String, i64, String) = sqlx::query_as(&format!(
        r#"SELECT version::text, event_index, amount::text FROM "{schema}".deposit_log"#
    ))
    .fetch_one(&pool)
    .await
    .unwrap();
    assert_eq!(
        (version.as_str(), index, amount.as_str()),
        ("1001", 0, "700")
    );

    // The outbox carries rows in the API's shape.
    let feed = outbox(&pool, &schema).await;
    let first = feed.iter().find(|c| c.2 == "balances").unwrap();
    assert_eq!(first.3, "insert");
    assert_eq!(
        serde_json::from_str::<serde_json::Value>(&first.5).unwrap(),
        serde_json::json!({ "user": vault::user(1).to_string(), "balance": "700", "deposits": "1" })
    );
    Store::reset(&pool, &schema).await.unwrap();
}

#[tokio::test]
async fn a_changed_config_needs_a_rebuild_but_formatting_does_not() {
    let Some(pool) = pool().await else { return };
    let (lock, project) = vault::project();
    let schema = fresh_schema(&pool, "fingerprint").await;
    let (txs, _) = vault::transactions(&[Op::Deposit { user: 0, amount: 5 }]);
    let decoded = vault::decode(&lock, &project, &txs);
    run_batched(&pool, &schema, &project, &lock, &decoded, &[1], &[]).await;

    // Comments and layout don't change what's built.
    let reformatted = format!("# the vault\n{}\n\n", vault::CONFIG.replace(": {", ":  {"));
    let (lock2, project2) = project_from(&reformatted);
    let store = Store::open(pool.clone(), &schema, &project2, &lock2)
        .await
        .unwrap();
    assert_eq!(store.cursor(), Some(decoded[0].version));

    // A rule change does.
    let changed = vault::CONFIG.replace("deposits + 1", "deposits + 2");
    let (lock3, project3) = project_from(&changed);
    let err = Store::open(pool.clone(), &schema, &project3, &lock3)
        .await
        .unwrap_err();
    assert!(matches!(err, StoreError::Rebuild { .. }), "{err}");
    assert!(!err.is_retryable());

    // Replay starts over.
    Store::reset(&pool, &schema).await.unwrap();
    let store = Store::open(pool.clone(), &schema, &project3, &lock3)
        .await
        .unwrap();
    assert_eq!(store.cursor(), None);
    assert!(store.scan(TableId::State(0)).await.unwrap().is_empty());
    Store::reset(&pool, &schema).await.unwrap();
}

#[tokio::test]
async fn a_shadow_build_swaps_in_atomically() {
    let Some(pool) = pool().await else { return };
    let (lock, project) = vault::project();
    let changed = vault::CONFIG.replace("deposits + 1", "deposits + 2");
    let (lock2, project2) = project_from(&changed);
    let live = fresh_schema(&pool, "live").await;
    let shadow = nineveh_store::shadow_name(&live).unwrap();
    Store::reset(&pool, &shadow).await.unwrap();
    let (txs, _) = vault::transactions(&[
        Op::Deposit { user: 0, amount: 5 },
        Op::Deposit { user: 0, amount: 6 },
        Op::Deposit { user: 1, amount: 7 },
    ]);
    let decoded = vault::decode(&lock, &project, &txs);

    // The live build has two transactions; the shadow, under the new rule, all three.
    run_batched(&pool, &live, &project, &lock, &decoded[..2], &[1], &[]).await;
    run_batched(&pool, &shadow, &project2, &lock2, &decoded, &[2], &[]).await;
    let shadow_feed = outbox(&pool, &shadow).await;

    Store::swap(&pool, &live, &shadow).await.unwrap();

    // `live` is now the shadow's build: its fingerprint, cursor, rows and feed.
    let store = Store::open(pool.clone(), &live, &project2, &lock2)
        .await
        .unwrap();
    assert_eq!(store.cursor(), Some(decoded[2].version));
    let deposits: Vec<(String, String)> = sqlx::query_as(&format!(
        r#"SELECT "user", deposits::text FROM "{live}".balances ORDER BY "user""#
    ))
    .fetch_all(&pool)
    .await
    .unwrap();
    assert_eq!(
        deposits,
        [
            (vault::user(0).to_string(), "4".to_owned()),
            (vault::user(1).to_string(), "2".to_owned()),
        ]
    );
    assert_eq!(
        outbox(&pool, &live).await,
        shadow_feed,
        "the shadow's feed moved"
    );
    assert!(outbox(&pool, &shadow).await.is_empty());

    // The shadow is gone, and the old config no longer fits `live`.
    let exists: bool = sqlx::query_scalar(
        "SELECT EXISTS (SELECT 1 FROM information_schema.schemata WHERE schema_name = $1)",
    )
    .bind(&shadow)
    .fetch_one(&pool)
    .await
    .unwrap();
    assert!(!exists);
    let err = Store::open(pool.clone(), &live, &project, &lock)
        .await
        .unwrap_err();
    assert!(matches!(err, StoreError::Rebuild { .. }), "{err}");

    // Swapping from a shadow that isn't there changes nothing.
    let err = Store::swap(&pool, &live, &shadow).await.unwrap_err();
    assert!(matches!(err, StoreError::Missing(_)), "{err}");
    assert_eq!(outbox(&pool, &live).await, shadow_feed);
    Store::reset(&pool, &live).await.unwrap();
}

#[test]
fn shadow_names_are_reserved() {
    assert_eq!(nineveh_store::shadow_name("vault").unwrap(), "vault__next");
    assert!(nineveh_store::shadow_name("vault__next").is_err());
    assert!(
        nineveh_store::shadow_name(&"a".repeat(60)).is_err(),
        "too long for the suffix"
    );
}

#[tokio::test]
async fn a_second_writer_and_stale_batches_are_refused() {
    let Some(pool) = pool().await else { return };
    let (lock, project) = vault::project();
    let engine = Engine::new(&project);
    let schema = fresh_schema(&pool, "writers").await;
    let (txs, _) = vault::transactions(&[
        Op::Deposit { user: 0, amount: 5 },
        Op::Deposit { user: 1, amount: 6 },
    ]);
    let decoded = vault::decode(&lock, &project, &txs);

    let mut a = Store::open(pool.clone(), &schema, &project, &lock)
        .await
        .unwrap();
    let mut b = Store::open(pool.clone(), &schema, &project, &lock)
        .await
        .unwrap();
    let first = fold(&a, &engine, &mut Loaded::default(), &decoded[..1]).await;
    a.commit(&first).await.unwrap();

    // `b` still thinks nothing is committed.
    let err = b.commit(&first).await.unwrap_err();
    assert!(matches!(err, StoreError::CursorMoved { .. }), "{err}");

    // Committing the same batch twice would apply it twice.
    let err = a.commit(&first).await.unwrap_err();
    assert!(matches!(err, StoreError::Stale { .. }), "{err}");

    let second = fold(&a, &engine, &mut Loaded::default(), &decoded[1..]).await;
    a.commit(&second).await.unwrap();
    assert_eq!(a.cursor(), Some(decoded[1].version));
    Store::reset(&pool, &schema).await.unwrap();
}

#[tokio::test]
async fn a_commit_with_changes_wakes_listeners() {
    let Some(pool) = pool().await else { return };
    let (lock, project) = vault::project();
    let engine = Engine::new(&project);
    let schema = fresh_schema(&pool, "notify").await;
    let (txs, _) = vault::transactions(&[Op::Deposit { user: 0, amount: 5 }]);
    let decoded = vault::decode(&lock, &project, &txs);

    let mut listener = PgListener::connect_with(&pool).await.unwrap();
    listener.listen(NOTIFY_CHANNEL).await.unwrap();
    let mut store = Store::open(pool.clone(), &schema, &project, &lock)
        .await
        .unwrap();
    let changes = fold(&store, &engine, &mut Loaded::default(), &decoded).await;
    store.commit(&changes).await.unwrap();

    // Other tests commit too; wait for this schema's wake-up.
    loop {
        let notification = listener.recv().await.unwrap();
        if notification.payload() == schema {
            break;
        }
    }
    Store::reset(&pool, &schema).await.unwrap();
}

#[tokio::test]
async fn schema_names_are_validated() {
    let Some(pool) = pool().await else { return };
    // Nineveh's tables exist, as they do once anything has built: every refusal below
    // happens before migrating, and they must survive it.
    nineveh_store::migrate(&pool).await.unwrap();
    let (lock, project) = vault::project();
    for bad in ["Vault", "_vault", "vault; drop table x", ""] {
        let err = Store::open(pool.clone(), bad, &project, &lock)
            .await
            .unwrap_err();
        assert!(
            matches!(err, StoreError::InvalidSchema(_)),
            "{bad:?}: {err}"
        );
    }
    // Resetting these would drop Nineveh's own tables or the database's.
    for reserved in ["nineveh", "public", "information_schema", "pg_catalog"] {
        let err = Store::reset(&pool, reserved).await.unwrap_err();
        assert!(
            matches!(err, StoreError::ReservedSchema(_)),
            "{reserved:?}: {err}"
        );
        let err = Store::open(pool.clone(), reserved, &project, &lock)
            .await
            .unwrap_err();
        assert!(
            matches!(err, StoreError::ReservedSchema(_)),
            "{reserved:?}: {err}"
        );
    }
    assert!(
        sqlx::query("SELECT 1 FROM nineveh.projects LIMIT 1")
            .fetch_optional(&pool)
            .await
            .is_ok(),
        "Nineveh's tables survive"
    );
}

/// The pipeline runs on spawned tasks, so every store future must be `Send`. This only
/// has to compile.
#[allow(dead_code, reason = "a compile-time check")]
fn futures_are_send(pool: &PgPool, project: &Project, lock: &Lockfile, store: &mut Store) {
    fn send<T: Send>(_: T) {}
    send(nineveh_store::migrate(pool));
    send(Store::open(pool.clone(), "x", project, lock));
    send(Store::reset(pool, "x"));
    send(store.load(Vec::new()));
    send(store.scan(TableId::Handles));
    send(store.commit(&ChangeSet::default()));
}

#[tokio::test]
async fn webhook_endpoints_keep_their_secret_and_their_place() {
    let Some(pool) = pool().await else { return };
    let schema = fresh_schema(&pool, "hooks").await;

    // The secret is made once and kept: a receiver's signature check has to keep
    // working across restarts and rebuilds.
    let first = webhooks::ensure(&pool, &schema, "my_backend")
        .await
        .unwrap();
    assert!(first.secret.starts_with("whsec_"), "{}", first.secret);
    assert_eq!(first.cursor, None, "nothing delivered yet");
    let again = webhooks::ensure(&pool, &schema, "my_backend")
        .await
        .unwrap();
    assert_eq!(again.secret, first.secret);

    // Two endpoints of one project don't share a secret.
    let other = webhooks::ensure(&pool, &schema, "audit").await.unwrap();
    assert_ne!(other.secret, first.secret);

    // Failures count up until a delivery clears them.
    assert_eq!(
        webhooks::failed(&pool, &schema, "my_backend", "connection refused")
            .await
            .unwrap(),
        1
    );
    assert_eq!(
        webhooks::failed(&pool, &schema, "my_backend", "connection refused")
            .await
            .unwrap(),
        2
    );
    webhooks::delivered(&pool, &schema, "my_backend", 1001, 3)
        .await
        .unwrap();
    let after = webhooks::list(&pool, &schema).await.unwrap();
    let mine = after.iter().find(|e| e.name == "my_backend").unwrap();
    assert_eq!(mine.cursor, Some((1001, 3)));
    assert_eq!(mine.failures, 0);
    assert_eq!(mine.last_error, None);

    // A rebuild's swap moves every endpoint to the new feed rather than replaying it.
    webhooks::skip_to(&pool, &schema, Some(42), Some(0))
        .await
        .unwrap();
    let moved = webhooks::list(&pool, &schema).await.unwrap();
    assert!(moved.iter().all(|e| e.cursor == Some((42, 0))));

    // Rotating replaces the secret; the cursor stays where it was.
    let rotated = webhooks::rotate(&pool, &schema, "my_backend")
        .await
        .unwrap();
    assert_ne!(rotated, first.secret);
    let kept = webhooks::list(&pool, &schema).await.unwrap();
    assert_eq!(kept[1].cursor, Some((42, 0)));

    // An endpoint the config no longer names is forgotten.
    webhooks::forget_others(&pool, &schema, &["my_backend".to_owned()])
        .await
        .unwrap();
    let left = webhooks::list(&pool, &schema).await.unwrap();
    assert_eq!(left.len(), 1);
    assert_eq!(left[0].name, "my_backend");
    webhooks::forget_others(&pool, &schema, &[]).await.unwrap();
    assert!(webhooks::list(&pool, &schema).await.unwrap().is_empty());
}
