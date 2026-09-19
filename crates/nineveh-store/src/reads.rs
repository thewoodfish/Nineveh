//! When a project's API was last read (ADR 0023).
//!
//! A project nobody reads is folding for nobody. This is the part of "is anything
//! waiting on this project" that has to be observed rather than declared — a webhook
//! endpoint and a live subscriber both announce themselves, but a query that stopped
//! arriving leaves no trace unless one is kept.
//!
//! It is deliberately coarse. The question is whether anyone has read this project in
//! the last day, so a timestamp that lags by a minute is exact enough, and paying for
//! a write on every read to learn nothing more would be a poor trade.

use sqlx::PgPool;

use crate::error::StoreError;

/// Note that `project` was read just now.
///
/// Callers should rate-limit themselves; this writes every time it is called.
///
/// # Errors
///
/// If the database fails.
pub async fn touch(pool: &PgPool, project: &str) -> Result<(), StoreError> {
    sqlx::query!(
        r#"INSERT INTO nineveh.project_reads (project, last_read_at)
           VALUES ($1, now())
           ON CONFLICT (project) DO UPDATE SET last_read_at = now()"#,
        project
    )
    .execute(pool)
    .await?;
    Ok(())
}

/// How many seconds ago `project` was last read, or `None` if it never has been.
///
/// Seconds rather than a timestamp because every caller is asking the same question —
/// "recently enough?" — and a duration answers it without a time library.
///
/// # Errors
///
/// If the database fails.
pub async fn seconds_since_read(pool: &PgPool, project: &str) -> Result<Option<i64>, StoreError> {
    let row = sqlx::query!(
        r#"SELECT extract(epoch FROM now() - last_read_at)::bigint AS "ago!"
           FROM nineveh.project_reads WHERE project = $1"#,
        project
    )
    .fetch_optional(pool)
    .await?;
    Ok(row.map(|r| r.ago))
}

/// Forget a project's read history, when the project goes.
///
/// # Errors
///
/// If the database fails.
pub async fn forget(pool: &PgPool, project: &str) -> Result<(), StoreError> {
    sqlx::query!(
        "DELETE FROM nineveh.project_reads WHERE project = $1",
        project
    )
    .execute(pool)
    .await?;
    Ok(())
}
