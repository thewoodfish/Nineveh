//! Accounts, sessions, sign-in states and API keys against a real Postgres (ADR 0018).
//!
//! Needs `NINEVEH_TEST_DATABASE_URL`; skips without it, except in CI.

#![allow(
    clippy::unwrap_used,
    clippy::panic,
    clippy::print_stderr,
    reason = "test-only crate: helpers panic on unexpected results and note skips"
)]

use nineveh_store::{StoreError, accounts, registry};
use sqlx::PgPool;
use sqlx::postgres::PgPoolOptions;

async fn pool() -> Option<PgPool> {
    let Ok(url) = std::env::var("NINEVEH_TEST_DATABASE_URL") else {
        assert!(
            std::env::var_os("CI").is_none(),
            "CI must set NINEVEH_TEST_DATABASE_URL"
        );
        eprintln!("skipping: set NINEVEH_TEST_DATABASE_URL to run the accounts tests");
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

/// A GitHub id no other test run uses.
fn github_id(n: i64) -> i64 {
    i64::from(std::process::id()) * 1000 + n
}

#[tokio::test]
async fn signing_in_again_updates_the_same_account() {
    let Some(pool) = pool().await else { return };
    let id = github_id(1);
    let first = accounts::sign_in(&pool, id, "ada", Some("Ada"), None)
        .await
        .unwrap();
    let again = accounts::sign_in(&pool, id, "ada-renamed", None, Some("https://a/b.png"))
        .await
        .unwrap();
    assert_eq!(first.id, again.id, "keyed by GitHub id, not login");
    assert_eq!(again.login, "ada-renamed");

    let session = format!("session-{id}");
    accounts::create_session(&pool, again.id, session.as_bytes(), 30)
        .await
        .unwrap();
    let found = accounts::session_account(&pool, session.as_bytes())
        .await
        .unwrap()
        .unwrap();
    assert_eq!(
        (found.id, found.login.as_str(), found.avatar_url.as_deref()),
        (again.id, "ada-renamed", Some("https://a/b.png"))
    );
    assert_eq!(
        accounts::session_account(&pool, b"no-such-session")
            .await
            .unwrap(),
        None
    );

    // Expired sessions don't count; ended ones are gone.
    let expired = format!("expired-{id}");
    accounts::create_session(&pool, again.id, expired.as_bytes(), -1)
        .await
        .unwrap();
    assert_eq!(
        accounts::session_account(&pool, expired.as_bytes())
            .await
            .unwrap(),
        None
    );
    accounts::end_session(&pool, session.as_bytes())
        .await
        .unwrap();
    assert_eq!(
        accounts::session_account(&pool, session.as_bytes())
            .await
            .unwrap(),
        None
    );
}

#[tokio::test]
async fn a_sign_in_state_is_spent_once() {
    let Some(pool) = pool().await else { return };
    let state = format!("state-{}", std::process::id());
    accounts::put_oauth_state(&pool, &state).await.unwrap();
    assert!(accounts::take_oauth_state(&pool, &state).await.unwrap());
    assert!(
        !accounts::take_oauth_state(&pool, &state).await.unwrap(),
        "a replayed callback is refused"
    );
    assert!(
        !accounts::take_oauth_state(&pool, "never-issued")
            .await
            .unwrap()
    );
}

#[tokio::test]
async fn keys_reach_their_project_until_revoked() {
    let Some(pool) = pool().await else { return };
    let owner = accounts::sign_in(&pool, github_id(2), "grace", None, None)
        .await
        .unwrap();
    let project = format!("keys_{}", std::process::id());
    let _ = registry::delete(&pool, &project).await;
    registry::insert(&pool, &project, "testnet", "", "", Some(owner.id))
        .await
        .unwrap();
    let listed = registry::list(&pool).await.unwrap();
    let registered = listed.iter().find(|p| p.name == project).unwrap();
    assert_eq!(registered.owner_id, Some(owner.id));

    let hash = format!("key-{project}");
    let key = accounts::create_api_key(&pool, &project, "web app", "nvk_1234abcd", hash.as_bytes())
        .await
        .unwrap();
    assert_eq!(
        (key.label.as_str(), key.prefix.as_str()),
        ("web app", "nvk_1234abcd")
    );
    assert!(matches!(
        accounts::create_api_key(&pool, "no_such_project", "x", "nvk_", b"other").await,
        Err(StoreError::NoProject(_))
    ));

    assert_eq!(
        accounts::api_key_project(&pool, hash.as_bytes())
            .await
            .unwrap(),
        Some(project.clone())
    );
    let keys = accounts::api_keys(&pool, &project).await.unwrap();
    assert_eq!(keys.len(), 1);
    assert!(keys[0].last_used_at.is_some(), "using a key records it");

    assert!(
        accounts::revoke_api_key(&pool, &project, key.id)
            .await
            .unwrap()
    );
    assert!(
        !accounts::revoke_api_key(&pool, &project, key.id)
            .await
            .unwrap()
    );
    assert_eq!(
        accounts::api_key_project(&pool, hash.as_bytes())
            .await
            .unwrap(),
        None,
        "a revoked key reaches nothing"
    );
    assert!(
        accounts::api_keys(&pool, &project)
            .await
            .unwrap()
            .is_empty()
    );

    // Deleting the project deletes its keys.
    let live = format!("live-{project}");
    accounts::create_api_key(&pool, &project, "again", "nvk_", live.as_bytes())
        .await
        .unwrap();
    registry::delete(&pool, &project).await.unwrap();
    assert_eq!(
        accounts::api_key_project(&pool, live.as_bytes())
            .await
            .unwrap(),
        None
    );
}
