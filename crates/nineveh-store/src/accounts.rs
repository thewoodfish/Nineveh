//! Accounts, sessions, sign-ins in flight, and project API keys (ADR 0018).
//!
//! Tokens arrive here already hashed: the store keeps and looks up only the SHA-256
//! of a token, never the token.

use sqlx::PgPool;

use crate::error::StoreError;
use crate::store::migrate;

/// A person, signed in with GitHub.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Account {
    pub id: i64,
    pub github_id: i64,
    pub login: String,
    pub name: Option<String>,
    pub avatar_url: Option<String>,
}

/// A project API key, as its owner sees it: never the key itself.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ApiKey {
    pub id: i64,
    pub project: String,
    pub label: String,
    /// The key's first characters.
    pub prefix: String,
    /// RFC 3339, UTC.
    pub created_at: String,
    pub last_used_at: Option<String>,
}

/// How long a sign-in may take between leaving for GitHub and coming back.
const STATE_MINUTES: i32 = 10;

/// The account for a GitHub user, created on first sign-in and refreshed after.
///
/// # Errors
///
/// If the database fails.
pub async fn sign_in(
    pool: &PgPool,
    github_id: i64,
    login: &str,
    name: Option<&str>,
    avatar_url: Option<&str>,
) -> Result<Account, StoreError> {
    migrate(pool).await?;
    let row = sqlx::query!(
        "INSERT INTO nineveh.accounts (github_id, login, name, avatar_url)
         VALUES ($1, $2, $3, $4)
         ON CONFLICT (github_id) DO UPDATE
             SET login = EXCLUDED.login, name = EXCLUDED.name,
                 avatar_url = EXCLUDED.avatar_url, updated_at = now()
         RETURNING id",
        github_id,
        login,
        name,
        avatar_url
    )
    .fetch_one(pool)
    .await?;
    Ok(Account {
        id: row.id,
        github_id,
        login: login.to_owned(),
        name: name.map(ToOwned::to_owned),
        avatar_url: avatar_url.map(ToOwned::to_owned),
    })
}

/// Start a session for `account`, lasting `days`.
///
/// # Errors
///
/// If the database fails.
pub async fn create_session(
    pool: &PgPool,
    account: i64,
    token_hash: &[u8],
    days: i32,
) -> Result<(), StoreError> {
    sqlx::query!(
        "INSERT INTO nineveh.sessions (token_hash, account_id, expires_at)
         VALUES ($1, $2, now() + make_interval(days => $3))",
        token_hash,
        account,
        days
    )
    .execute(pool)
    .await?;
    Ok(())
}

/// The account whose unexpired session this is.
///
/// # Errors
///
/// If the database fails.
pub async fn session_account(
    pool: &PgPool,
    token_hash: &[u8],
) -> Result<Option<Account>, StoreError> {
    let row = sqlx::query!(
        "SELECT a.id, a.github_id, a.login, a.name, a.avatar_url
         FROM nineveh.sessions s JOIN nineveh.accounts a ON a.id = s.account_id
         WHERE s.token_hash = $1 AND s.expires_at > now()",
        token_hash
    )
    .fetch_optional(pool)
    .await?;
    Ok(row.map(|r| Account {
        id: r.id,
        github_id: r.github_id,
        login: r.login,
        name: r.name,
        avatar_url: r.avatar_url,
    }))
}

/// End a session.
///
/// # Errors
///
/// If the database fails.
pub async fn end_session(pool: &PgPool, token_hash: &[u8]) -> Result<(), StoreError> {
    sqlx::query!(
        "DELETE FROM nineveh.sessions WHERE token_hash = $1",
        token_hash
    )
    .execute(pool)
    .await?;
    Ok(())
}

/// Remember a sign-in's `state` until it comes back, clearing any that expired.
///
/// # Errors
///
/// If the database fails.
pub async fn put_oauth_state(pool: &PgPool, state: &str) -> Result<(), StoreError> {
    migrate(pool).await?;
    sqlx::query!(
        "DELETE FROM nineveh.oauth_states
         WHERE created_at < now() - make_interval(mins => $1)",
        STATE_MINUTES
    )
    .execute(pool)
    .await?;
    sqlx::query!(
        "INSERT INTO nineveh.oauth_states (state) VALUES ($1)",
        state
    )
    .execute(pool)
    .await?;
    Ok(())
}

/// Spend a sign-in's `state`: whether it was issued, unexpired and not spent before.
///
/// # Errors
///
/// If the database fails.
pub async fn take_oauth_state(pool: &PgPool, state: &str) -> Result<bool, StoreError> {
    let taken = sqlx::query!(
        "DELETE FROM nineveh.oauth_states
         WHERE state = $1 AND created_at >= now() - make_interval(mins => $2)",
        state,
        STATE_MINUTES
    )
    .execute(pool)
    .await?;
    Ok(taken.rows_affected() == 1)
}

/// Record a new key for `project`.
///
/// # Errors
///
/// [`StoreError::NoProject`] if there's no such project, or if the database fails.
pub async fn create_api_key(
    pool: &PgPool,
    project: &str,
    label: &str,
    prefix: &str,
    key_hash: &[u8],
) -> Result<ApiKey, StoreError> {
    let row = sqlx::query!(
        r#"INSERT INTO nineveh.api_keys (project, label, prefix, key_hash)
           SELECT name, $2, $3, $4 FROM nineveh.control_projects WHERE name = $1
           RETURNING id,
             to_char(created_at AT TIME ZONE 'UTC', 'YYYY-MM-DD"T"HH24:MI:SS.MS"Z"') AS "created_at!""#,
        project,
        label,
        prefix,
        key_hash
    )
    .fetch_optional(pool)
    .await?
    .ok_or_else(|| StoreError::NoProject(project.to_owned()))?;
    Ok(ApiKey {
        id: row.id,
        project: project.to_owned(),
        label: label.to_owned(),
        prefix: prefix.to_owned(),
        created_at: row.created_at,
        last_used_at: None,
    })
}

/// `project`'s keys that aren't revoked, newest first.
///
/// # Errors
///
/// If the database fails.
pub async fn api_keys(pool: &PgPool, project: &str) -> Result<Vec<ApiKey>, StoreError> {
    let rows = sqlx::query!(
        r#"SELECT id, project, label, prefix,
             to_char(created_at AT TIME ZONE 'UTC', 'YYYY-MM-DD"T"HH24:MI:SS.MS"Z"') AS "created_at!",
             to_char(last_used_at AT TIME ZONE 'UTC', 'YYYY-MM-DD"T"HH24:MI:SS.MS"Z"') AS last_used_at
           FROM nineveh.api_keys
           WHERE project = $1 AND revoked_at IS NULL
           ORDER BY id DESC"#,
        project
    )
    .fetch_all(pool)
    .await?;
    Ok(rows
        .into_iter()
        .map(|r| ApiKey {
            id: r.id,
            project: r.project,
            label: r.label,
            prefix: r.prefix,
            created_at: r.created_at,
            last_used_at: r.last_used_at,
        })
        .collect())
}

/// Revoke one of `project`'s keys: whether it was live.
///
/// # Errors
///
/// If the database fails.
pub async fn revoke_api_key(pool: &PgPool, project: &str, id: i64) -> Result<bool, StoreError> {
    let revoked = sqlx::query!(
        "UPDATE nineveh.api_keys SET revoked_at = now()
         WHERE project = $1 AND id = $2 AND revoked_at IS NULL",
        project,
        id
    )
    .execute(pool)
    .await?;
    Ok(revoked.rows_affected() == 1)
}

/// The project a live key reaches, noting that it was used. Use is recorded at most
/// once a minute per key, so reads don't each write.
///
/// # Errors
///
/// If the database fails.
pub async fn api_key_project(pool: &PgPool, key_hash: &[u8]) -> Result<Option<String>, StoreError> {
    let Some(row) = sqlx::query!(
        r#"SELECT id, project,
             (last_used_at IS NULL OR last_used_at < now() - interval '1 minute') AS "stale!"
           FROM nineveh.api_keys WHERE key_hash = $1 AND revoked_at IS NULL"#,
        key_hash
    )
    .fetch_optional(pool)
    .await?
    else {
        return Ok(None);
    };
    if row.stale {
        sqlx::query!(
            "UPDATE nineveh.api_keys SET last_used_at = now() WHERE id = $1",
            row.id
        )
        .execute(pool)
        .await?;
    }
    Ok(Some(row.project))
}
