//! The record log (ADR 0022).
//!
//! Every record the decoder emitted for a project, kept so a rebuild can replay them
//! instead of re-reading the chain. Re-reading is the expensive option: a 90-day-old
//! mainnet contract takes days to stream again and holds one of Geomi's limited
//! concurrent streams throughout, and past a fortnight the stream is the only source
//! for those bytes at all (`docs/research/spike-a-stream.md`).
//!
//! The log is keyed by project, not by state schema. A rebuild builds into `S__next`
//! and the swap drops `S` (ADR 0016), so records hung off a schema would be deleted by
//! the operation that needs them.
//!
//! The log leads the fold. Its cursor says how far records are written; the cursor in
//! `nineveh.projects` says how far state is committed, and the first may run ahead of
//! the second.

use sqlx::PgPool;

use nineveh_core::Version;
use nineveh_decode::StoredRecord;

use crate::error::StoreError;

/// One record as the log holds it, with the transaction facts a rule may read.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Logged {
    pub version: Version,
    /// Position in the transaction, in the order the decoder emitted it.
    pub ord: i32,
    pub timestamp_micros: u64,
    pub success: bool,
    pub sender: Option<String>,
    pub record: StoredRecord,
}

/// How far the log is written, and what it was decoded against.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct LogState {
    /// The last version whose records are all written; `None` before the first.
    pub cursor: Option<Version>,
    /// The lock the records were decoded against. A change means they have to be
    /// filled again from the stream: they were decoded under layouts since corrected.
    pub lock_hash: String,
    /// The sources the log holds records for, sorted. A config that adds one has no
    /// history for it here.
    pub sources: Vec<String>,
}

/// The log's state for `project`, or `None` if it has never been written.
///
/// # Errors
///
/// If the database fails.
pub async fn state(pool: &PgPool, project: &str) -> Result<Option<LogState>, StoreError> {
    let row = sqlx::query!(
        "SELECT cursor, lock_hash, sources FROM nineveh.record_cursors WHERE project = $1",
        project
    )
    .fetch_optional(pool)
    .await?;
    Ok(row.map(|r| LogState {
        cursor: r
            .cursor
            .and_then(|c| u64::try_from(c).ok())
            .map(Version::new),
        lock_hash: r.lock_hash,
        sources: r
            .sources
            .split(',')
            .filter(|s| !s.is_empty())
            .map(ToOwned::to_owned)
            .collect(),
    }))
}

/// Append `records` and move the log's cursor to `through`, in one transaction.
///
/// Appending is idempotent: a version re-read after a crash overwrites its own rows
/// rather than duplicating them, which is what lets the writer restart from its cursor
/// without checking what it already wrote.
///
/// # Errors
///
/// If the database fails, or a record can't be encoded, which means a bug.
pub async fn append(
    pool: &PgPool,
    project: &str,
    lock_hash: &str,
    sources: &[String],
    records: &[Logged],
    through: Version,
) -> Result<(), StoreError> {
    let mut tx = pool.begin().await?;
    for logged in records {
        // sqlx is built without its `json` feature, so JSON crosses as text and
        // Postgres does the cast — the same way the outbox writes `new_row`.
        let json = serde_json::to_string(&logged.record).map_err(|e| StoreError::Corrupt {
            table: "nineveh.records".into(),
            reason: format!("encoding a record: {e}"),
        })?;
        let version = i64::try_from(logged.version.get()).map_err(|_| StoreError::Corrupt {
            table: "nineveh.records".into(),
            reason: "version past i64".into(),
        })?;
        let timestamp = i64::try_from(logged.timestamp_micros).unwrap_or(i64::MAX);
        sqlx::query!(
            r#"INSERT INTO nineveh.records
                   (project, version, ord, timestamp_micros, success, sender, record)
               VALUES ($1, $2, $3, $4, $5, $6, $7::text::jsonb)
               ON CONFLICT (project, version, ord) DO UPDATE
                   SET timestamp_micros = excluded.timestamp_micros,
                       success          = excluded.success,
                       sender           = excluded.sender,
                       record           = excluded.record"#,
            project,
            version,
            logged.ord,
            timestamp,
            logged.success,
            logged.sender,
            json,
        )
        .execute(&mut *tx)
        .await?;
    }
    let through_i64 = i64::try_from(through.get()).map_err(|_| StoreError::Corrupt {
        table: "nineveh.record_cursors".into(),
        reason: "version past i64".into(),
    })?;
    sqlx::query!(
        r#"INSERT INTO nineveh.record_cursors (project, cursor, lock_hash, sources)
           VALUES ($1, $2, $3, $4)
           ON CONFLICT (project) DO UPDATE
               SET cursor = excluded.cursor,
                   lock_hash = excluded.lock_hash,
                   sources = excluded.sources,
                   updated_at = now()"#,
        project,
        through_i64,
        lock_hash,
        sources.join(","),
    )
    .execute(&mut *tx)
    .await?;
    tx.commit().await?;
    Ok(())
}

/// Every record of `project` from `from` up to and including `to`, in fold order.
///
/// # Errors
///
/// If the database fails, or a row doesn't decode, which means an older encoder or a
/// corrupted log.
pub async fn read(
    pool: &PgPool,
    project: &str,
    from: Version,
    to: Version,
) -> Result<Vec<Logged>, StoreError> {
    let bound = |v: Version| {
        i64::try_from(v.get()).map_err(|_| StoreError::Corrupt {
            table: "nineveh.records".into(),
            reason: "version past i64".into(),
        })
    };
    let rows = sqlx::query!(
        r#"SELECT version, ord, timestamp_micros, success, sender, record::text AS "record!"
           FROM nineveh.records
           WHERE project = $1 AND version >= $2 AND version <= $3
           ORDER BY version, ord"#,
        project,
        bound(from)?,
        bound(to)?,
    )
    .fetch_all(pool)
    .await?;

    rows.into_iter()
        .map(|r| {
            let record: StoredRecord =
                serde_json::from_str(&r.record).map_err(|e| StoreError::Corrupt {
                    table: "nineveh.records".into(),
                    reason: format!("record at version {} ord {}: {e}", r.version, r.ord),
                })?;
            Ok(Logged {
                version: Version::new(u64::try_from(r.version).unwrap_or(0)),
                ord: r.ord,
                timestamp_micros: u64::try_from(r.timestamp_micros).unwrap_or(0),
                success: r.success,
                sender: r.sender,
                record,
            })
        })
        .collect()
}

/// How many records are logged past `folded` — the backlog a fold would have to work
/// through to become current (ADR 0023).
///
/// This is the number idleness is bounded by. Catching up reads the log, so the cost
/// is records rather than elapsed time: a quiet contract idle for months has a smaller
/// backlog than a busy one idle for an hour.
///
/// # Errors
///
/// If the database fails.
pub async fn pending(
    pool: &PgPool,
    project: &str,
    folded: Option<Version>,
) -> Result<i64, StoreError> {
    let after = folded.map_or(-1, |v| i64::try_from(v.get()).unwrap_or(i64::MAX));
    let row = sqlx::query!(
        r#"SELECT count(*) AS "pending!" FROM nineveh.records
           WHERE project = $1 AND version > $2"#,
        project,
        after
    )
    .fetch_one(pool)
    .await?;
    Ok(row.pending)
}

/// The earliest version logged for `project`, or `None` if nothing is.
///
/// A rebuild needs the log to reach back to where it starts; this says how far back
/// that is.
///
/// # Errors
///
/// If the database fails.
pub async fn first_version(pool: &PgPool, project: &str) -> Result<Option<Version>, StoreError> {
    let row = sqlx::query!(
        "SELECT min(version) AS earliest FROM nineveh.records WHERE project = $1",
        project
    )
    .fetch_one(pool)
    .await?;
    Ok(row
        .earliest
        .and_then(|v| u64::try_from(v).ok())
        .map(Version::new))
}

/// Drop everything logged for `project`. Used when the lock changes: the records were
/// decoded under layouts since corrected, so they have to come from the stream again.
///
/// # Errors
///
/// If the database fails.
pub async fn forget(pool: &PgPool, project: &str) -> Result<(), StoreError> {
    let mut tx = pool.begin().await?;
    sqlx::query!("DELETE FROM nineveh.records WHERE project = $1", project)
        .execute(&mut *tx)
        .await?;
    sqlx::query!(
        "DELETE FROM nineveh.record_cursors WHERE project = $1",
        project
    )
    .execute(&mut *tx)
    .await?;
    tx.commit().await?;
    Ok(())
}

/// How many records and how many bytes `project` has logged — the number a tier is
/// metered on, and the one Studio shows.
///
/// # Errors
///
/// If the database fails.
pub async fn usage(pool: &PgPool, project: &str) -> Result<(i64, i64), StoreError> {
    let row = sqlx::query!(
        r#"SELECT count(*) AS "count!", coalesce(sum(pg_column_size(record)), 0)::bigint AS "bytes!"
           FROM nineveh.records WHERE project = $1"#,
        project
    )
    .fetch_one(pool)
    .await?;
    Ok((row.count, row.bytes))
}
