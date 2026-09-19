//! Pruning what a project no longer needs (ADR 0024).
//!
//! Two tables grow without limit, and they need different answers.
//!
//! The outbox is a buffer: its rows matter until every reader has seen them, so it is
//! kept for a window — long enough that a receiver down over a weekend still catches up
//! — and never pruned past the slowest webhook endpoint's cursor. Deleting ahead of an
//! endpoint would drop deliveries it was owed and turn at-least-once into at-most-once
//! with no error anywhere.
//!
//! The record log is history, and its value is that a project's past isn't bought from
//! the chain twice. A time window would cap exactly that, so it goes by size, oldest
//! first — and only ever gives up records the fold has already consumed. Records above
//! the fold cursor have not become state yet, and deleting them would leave rows that
//! can never be computed from inputs only the chain still has.

use sqlx::PgPool;

use nineveh_core::Version;

use crate::error::StoreError;

/// Rows a single pass deletes, so a large backlog is worked off over several passes
/// rather than held in one long transaction.
const BATCH: i64 = 20_000;

/// What a pass deleted.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub struct Pruned {
    pub changes: u64,
    pub records: u64,
    /// Whether more remains for the next pass.
    pub more: bool,
}

/// Delete outbox rows older than `days`, but never past a webhook endpoint that hasn't
/// delivered them.
///
/// An endpoint stuck behind holds its schema's outbox open, which is correct — those
/// deliveries are still owed — and visible, since its failures and last error are
/// already reported.
///
/// # Errors
///
/// If the database fails.
pub async fn prune_changes(pool: &PgPool, schema: &str, days: i32) -> Result<u64, StoreError> {
    let done = sqlx::query!(
        r#"WITH floor AS (
               SELECT min(version) AS version FROM nineveh.webhooks
               WHERE schema_name = $1 AND version IS NOT NULL
           ),
           doomed AS (
               SELECT c.version, c.seq FROM nineveh.changes c, floor
               WHERE c.schema_name = $1
                 AND c.committed_at < now() - make_interval(days => $2)
                 AND (floor.version IS NULL OR c.version <= floor.version)
               ORDER BY c.version, c.seq
               LIMIT $3
           )
           DELETE FROM nineveh.changes c
           USING doomed d
           WHERE c.schema_name = $1 AND c.version = d.version AND c.seq = d.seq"#,
        schema,
        days,
        BATCH,
    )
    .execute(pool)
    .await?;
    Ok(done.rows_affected())
}

/// Delete the oldest records of `project` until its log is within `limit` bytes, taking
/// only what the fold has already consumed.
///
/// A project that is idle, halted or far behind keeps its records however large they
/// grow: it is over quota, and the answer to that is to say so rather than to destroy
/// the inputs it still needs.
///
/// # Errors
///
/// If the database fails.
pub async fn prune_records(
    pool: &PgPool,
    project: &str,
    limit_bytes: i64,
    folded: Option<Version>,
) -> Result<u64, StoreError> {
    let Some(folded) = folded else {
        // Nothing has been folded, so nothing may be taken.
        return Ok(0);
    };
    let folded = i64::try_from(folded.get()).unwrap_or(i64::MAX);
    let (_, bytes) = crate::records::usage(pool, project).await?;
    if bytes <= limit_bytes {
        return Ok(0);
    }
    let over = bytes - limit_bytes;
    let done = sqlx::query!(
        r#"WITH running AS (
               SELECT version, ord,
                      sum(pg_column_size(record)) OVER (ORDER BY version, ord) AS so_far
               FROM nineveh.records
               WHERE project = $1 AND version <= $2
               ORDER BY version, ord
               LIMIT $4
           ),
           doomed AS (SELECT version, ord FROM running WHERE so_far <= $3)
           DELETE FROM nineveh.records r
           USING doomed d
           WHERE r.project = $1 AND r.version = d.version AND r.ord = d.ord"#,
        project,
        folded,
        over,
        BATCH,
    )
    .execute(pool)
    .await?;
    Ok(done.rows_affected())
}
