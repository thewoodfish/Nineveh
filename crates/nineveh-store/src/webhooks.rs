//! Where a project's state changes are delivered (ADR 0020).
//!
//! One row per named endpoint: its signing secret, the position in the change outbox
//! it has been delivered up to, and how the last attempt went. The sender in
//! `nineveh-realtime` reads and advances these; the control plane shows them.
//!
//! An endpoint's secret is generated once, the first time the endpoint is seen, and
//! kept as written — a signature can't be computed from a hash. It survives rebuilds,
//! so a receiver's signature check keeps working across them.

use sqlx::PgPool;

use crate::error::StoreError;

/// How a delivery is signed, and how far it has got.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Endpoint {
    pub name: String,
    /// The secret every delivery to this endpoint is signed with.
    pub secret: String,
    /// The last change delivered, as `(version, seq)`; `None` before the first.
    pub cursor: Option<(i64, i32)>,
    /// Failed attempts since the last delivery.
    pub failures: i32,
    pub last_error: Option<String>,
}

/// The endpoint `name` of `schema`, created with a fresh secret if it's new.
///
/// # Errors
///
/// If the database fails, or the OS has no randomness for a new secret.
pub async fn ensure(pool: &PgPool, schema: &str, name: &str) -> Result<Endpoint, StoreError> {
    let secret = new_secret()?;
    let row = sqlx::query!(
        r#"INSERT INTO nineveh.webhooks (schema_name, name, secret)
           VALUES ($1, $2, $3)
           ON CONFLICT (schema_name, name) DO UPDATE SET name = excluded.name
           RETURNING secret, version, seq, failures, last_error"#,
        schema,
        name,
        secret,
    )
    .fetch_one(pool)
    .await?;
    Ok(Endpoint {
        name: name.to_owned(),
        secret: row.secret,
        cursor: row.version.zip(row.seq),
        failures: row.failures,
        last_error: row.last_error,
    })
}

/// Every endpoint of `schema`, by name.
///
/// # Errors
///
/// If the database fails.
pub async fn list(pool: &PgPool, schema: &str) -> Result<Vec<Endpoint>, StoreError> {
    let rows = sqlx::query!(
        r#"SELECT name, secret, version, seq, failures, last_error FROM nineveh.webhooks
           WHERE schema_name = $1 ORDER BY name"#,
        schema,
    )
    .fetch_all(pool)
    .await?;
    Ok(rows
        .into_iter()
        .map(|r| Endpoint {
            name: r.name,
            secret: r.secret,
            cursor: r.version.zip(r.seq),
            failures: r.failures,
            last_error: r.last_error,
        })
        .collect())
}

/// Record a delivery: the endpoint is at `(version, seq)` and healthy again.
///
/// # Errors
///
/// If the database fails.
pub async fn delivered(
    pool: &PgPool,
    schema: &str,
    name: &str,
    version: i64,
    seq: i32,
) -> Result<(), StoreError> {
    sqlx::query!(
        r#"UPDATE nineveh.webhooks
           SET version = $3, seq = $4, failures = 0, last_error = NULL,
               last_delivered_at = now()
           WHERE schema_name = $1 AND name = $2"#,
        schema,
        name,
        version,
        seq,
    )
    .execute(pool)
    .await?;
    Ok(())
}

/// Record a failed attempt, for backing off and for reporting.
///
/// # Errors
///
/// If the database fails.
pub async fn failed(
    pool: &PgPool,
    schema: &str,
    name: &str,
    error: &str,
) -> Result<i32, StoreError> {
    let row = sqlx::query!(
        r#"UPDATE nineveh.webhooks SET failures = failures + 1, last_error = $3
           WHERE schema_name = $1 AND name = $2
           RETURNING failures"#,
        schema,
        name,
        error,
    )
    .fetch_optional(pool)
    .await?;
    Ok(row.map_or(0, |r| r.failures))
}

/// Put a new endpoint at a position, if it hasn't got one: where a sender starts it,
/// which is the newest change, so configuring an endpoint doesn't deliver the
/// project's history into someone's backend.
///
/// # Errors
///
/// If the database fails.
pub async fn start_at(
    pool: &PgPool,
    schema: &str,
    name: &str,
    version: i64,
    seq: i32,
) -> Result<(), StoreError> {
    sqlx::query!(
        r#"UPDATE nineveh.webhooks SET version = $3, seq = $4
           WHERE schema_name = $1 AND name = $2 AND version IS NULL"#,
        schema,
        name,
        version,
        seq,
    )
    .execute(pool)
    .await?;
    Ok(())
}

/// Move an endpoint past changes it didn't ask for, without claiming a delivery.
///
/// # Errors
///
/// If the database fails.
pub async fn advance(
    pool: &PgPool,
    schema: &str,
    name: &str,
    version: i64,
    seq: i32,
) -> Result<(), StoreError> {
    sqlx::query!(
        "UPDATE nineveh.webhooks SET version = $3, seq = $4 WHERE schema_name = $1 AND name = $2",
        schema,
        name,
        version,
        seq,
    )
    .execute(pool)
    .await?;
    Ok(())
}

/// Move an endpoint to a position without delivering anything: what a rebuild's swap
/// does, since the new build's outbox is a different feed and replaying it would
/// deliver the project's whole history again (ADR 0016).
///
/// # Errors
///
/// If the database fails.
pub async fn skip_to(
    pool: &PgPool,
    schema: &str,
    version: Option<i64>,
    seq: Option<i32>,
) -> Result<(), StoreError> {
    sqlx::query!(
        "UPDATE nineveh.webhooks SET version = $2, seq = $3 WHERE schema_name = $1",
        schema,
        version,
        seq,
    )
    .execute(pool)
    .await?;
    Ok(())
}

/// Give an endpoint a new secret, invalidating the old one.
///
/// # Errors
///
/// If the database fails, or the OS has no randomness.
pub async fn rotate(pool: &PgPool, schema: &str, name: &str) -> Result<String, StoreError> {
    let secret = new_secret()?;
    sqlx::query!(
        "UPDATE nineveh.webhooks SET secret = $3 WHERE schema_name = $1 AND name = $2",
        schema,
        name,
        secret,
    )
    .execute(pool)
    .await?;
    Ok(secret)
}

/// Forget endpoints of `schema` that `keep` doesn't name: what a config change leaves
/// behind. With an empty `keep`, forget them all, as deleting a project does.
///
/// # Errors
///
/// If the database fails.
pub async fn forget_others(pool: &PgPool, schema: &str, keep: &[String]) -> Result<(), StoreError> {
    sqlx::query!(
        "DELETE FROM nineveh.webhooks WHERE schema_name = $1 AND name <> ALL($2)",
        schema,
        keep,
    )
    .execute(pool)
    .await?;
    Ok(())
}

/// A secret to sign deliveries with: `whsec_` and 32 random bytes as hex.
fn new_secret() -> Result<String, StoreError> {
    let mut bytes = [0u8; 32];
    getrandom::fill(&mut bytes).map_err(|e| StoreError::Random(e.to_string()))?;
    let mut secret = String::with_capacity(6 + 64);
    secret.push_str("whsec_");
    for byte in bytes {
        use std::fmt::Write as _;
        // Writing to a String can't fail.
        let _ = write!(secret, "{byte:02x}");
    }
    Ok(secret)
}
