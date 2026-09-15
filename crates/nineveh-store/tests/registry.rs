//! The control plane's registry against a real Postgres: names are unique across the
//! registry and existing builds, and deleting a project drops its state.
//!
//! Needs `NINEVEH_TEST_DATABASE_URL`; skips without it, except in CI.

#![allow(
    clippy::unwrap_used,
    clippy::panic,
    clippy::print_stderr,
    reason = "test-only crate: helpers panic on unexpected results and note skips"
)]

use nineveh_store::{Store, StoreError, registry};
use nineveh_testkit::vault;
use sqlx::PgPool;
use sqlx::postgres::PgPoolOptions;

async fn pool() -> Option<PgPool> {
    let Ok(url) = std::env::var("NINEVEH_TEST_DATABASE_URL") else {
        assert!(
            std::env::var_os("CI").is_none(),
            "CI must set NINEVEH_TEST_DATABASE_URL"
        );
        eprintln!("skipping: set NINEVEH_TEST_DATABASE_URL to run the registry's tests");
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

async fn schema_exists(pool: &PgPool, name: &str) -> bool {
    sqlx::query_scalar::<_, bool>("SELECT EXISTS (SELECT 1 FROM pg_namespace WHERE nspname = $1)")
        .bind(name)
        .fetch_one(pool)
        .await
        .unwrap()
}

#[tokio::test]
async fn registers_updates_and_deletes_projects() {
    let Some(pool) = pool().await else { return };
    let name = format!("reg_{}", std::process::id());
    let _ = registry::delete(&pool, &name).await;

    registry::insert(&pool, &name, "testnet", "config: 1", "lock: 1")
        .await
        .unwrap();
    assert!(matches!(
        registry::insert(&pool, &name, "testnet", "config: 2", "lock: 2").await,
        Err(StoreError::ProjectExists(_))
    ));
    let listed = registry::list(&pool).await.unwrap();
    let found = listed.iter().find(|p| p.name == name).unwrap();
    assert_eq!(
        (found.network.as_str(), found.config.as_str(), found.running),
        ("testnet", "config: 1", true)
    );
    assert!(found.created_at.ends_with('Z'), "{}", found.created_at);

    registry::update(&pool, &name, "config: 2", "lock: 2")
        .await
        .unwrap();
    registry::set_running(&pool, &name, false).await.unwrap();
    let found = registry::list(&pool)
        .await
        .unwrap()
        .into_iter()
        .find(|p| p.name == name)
        .unwrap();
    assert_eq!(
        (found.config.as_str(), found.lock.as_str()),
        ("config: 2", "lock: 2")
    );
    assert!(!found.running);

    // Its state goes with it.
    let (lock, project) = vault::project();
    Store::open(pool.clone(), &name, &project, &lock)
        .await
        .unwrap();
    assert!(schema_exists(&pool, &name).await);
    registry::delete(&pool, &name).await.unwrap();
    assert!(!schema_exists(&pool, &name).await);
    assert!(
        registry::list(&pool)
            .await
            .unwrap()
            .iter()
            .all(|p| p.name != name)
    );
    assert!(matches!(
        registry::delete(&pool, &name).await,
        Err(StoreError::NoProject(_))
    ));
    assert!(matches!(
        registry::update(&pool, &name, "", "").await,
        Err(StoreError::NoProject(_))
    ));
}

#[tokio::test]
async fn refuses_a_name_a_build_already_has() {
    let Some(pool) = pool().await else { return };
    let name = format!("reg_taken_{}", std::process::id());
    let (lock, project) = vault::project();
    Store::reset(&pool, &name).await.unwrap();
    // A build made by `nineveh run`, outside the registry.
    Store::open(pool.clone(), &name, &project, &lock)
        .await
        .unwrap();
    assert!(matches!(
        registry::insert(&pool, &name, "testnet", "", "").await,
        Err(StoreError::ProjectExists(_))
    ));
    assert!(matches!(
        registry::insert(&pool, "nineveh", "testnet", "", "").await,
        Err(StoreError::ReservedSchema(_))
    ));
    Store::reset(&pool, &name).await.unwrap();
}
