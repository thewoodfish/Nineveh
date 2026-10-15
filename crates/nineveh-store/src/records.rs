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

use std::collections::BTreeMap;

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

/// What writing one record did to the log's running totals.
struct Appended {
    /// 1 for a row that wasn't there, 0 for one that replaced another.
    rows: i64,
    /// The size this row added, which for a replacement is the difference.
    bytes: i64,
    /// The source to credit, unless this write changed nothing about which source
    /// that slot belongs to — which is what a version re-read after a crash is.
    credited: Option<String>,
    /// The source whose record was in the slot before, when that was a different one.
    displaced: Option<String>,
}

/// Write one record into the log, and say what it did to the totals.
///
/// `xmax = 0` is how Postgres reports whether the row was inserted or overwrote one
/// that was already there, and the two cases mean different things. An overwrite is
/// not always the same record arriving twice: a version re-read after a crash does
/// rewrite its rows with identical bytes, but a project that has just gained a source
/// re-reads its whole range with a wider selection, and then every version emits more
/// records than it did, so each `ord` holds a record that has shifted along one or
/// more places.
///
/// Counting only genuine inserts credited whichever source happened to land in the new
/// slots at the tail and nothing at all to the records that displaced others, which is
/// how a source could report having matched nothing while the log table built from it
/// — one row per record, by definition — filled up. So an overwrite moves the count
/// from the record that was in the slot to the one that is in it now.
async fn write_one(
    tx: &mut sqlx::Transaction<'_, sqlx::Postgres>,
    project: &str,
    logged: &Logged,
) -> Result<Appended, StoreError> {
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
    // `prior` is the row this one is about to replace, if there is one. The CTE reads
    // it from the statement's own snapshot, so it is what was there before the upsert,
    // which is the whole reason it can be subtracted.
    let written = sqlx::query!(
        r#"WITH prior AS (
               SELECT record ->> 'source' AS source, pg_column_size(record) AS size
               FROM nineveh.records
               WHERE project = $1 AND version = $2 AND ord = $3
           )
           INSERT INTO nineveh.records
               (project, version, ord, timestamp_micros, success, sender, record)
           VALUES ($1, $2, $3, $4, $5, $6, $7::text::jsonb)
           ON CONFLICT (project, version, ord) DO UPDATE
               SET timestamp_micros = excluded.timestamp_micros,
                   success          = excluded.success,
                   sender           = excluded.sender,
                   record           = excluded.record
           RETURNING (xmax = 0) AS "inserted!",
                     pg_column_size(record) AS "size!",
                     (SELECT source FROM prior) AS "prior_source?",
                     (SELECT size FROM prior) AS "prior_size?""#,
        project,
        version,
        logged.ord,
        timestamp,
        logged.success,
        logged.sender,
        json,
    )
    .fetch_one(&mut **tx)
    .await?;
    let source = logged.record.source.clone();
    if written.inserted {
        return Ok(Appended {
            rows: 1,
            bytes: i64::from(written.size),
            credited: Some(source),
            displaced: None,
        });
    }
    // An overwrite by the same source is a version re-read after a crash writing its
    // own record back, and moves nothing: the slot already counts for that source.
    let unchanged = written.prior_source.as_deref() == Some(source.as_str());
    Ok(Appended {
        // The row count is unchanged, but the bytes are not: the record that replaced
        // this slot is a different one, and the usage meter is what decides how far
        // back a project can be rebuilt for free.
        rows: 0,
        bytes: i64::from(written.size) - i64::from(written.prior_size.unwrap_or(0)),
        credited: (!unchanged).then_some(source),
        displaced: written.prior_source.filter(|_| !unchanged),
    })
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
    // What this batch adds to the log's running totals. `xmax = 0` is how Postgres
    // reports whether a row was inserted or overwrote one that was already there, and
    // the two cases are counted differently because they mean different things.
    let mut added_count: i64 = 0;
    let mut added_bytes: i64 = 0;
    // What each source matched in this batch, so a source that never matches anything
    // can be told apart from one whose contract is merely quiet right now.
    //
    // An overwrite is not always the same record arriving twice. A version re-read
    // after a crash does rewrite its rows with identical bytes — but a project that
    // has just gained a source re-reads its whole range with a wider selection, and
    // then every version emits more records than it did, so each `ord` holds a record
    // that has shifted along one or more places. Counting only genuine inserts credited
    // whichever source happened to land in the new slots at the tail and ignored every
    // record that displaced another, which is how a source could report nothing while
    // the log table built from it filled up. So an overwrite moves the count from the
    // record that was in that slot to the one that is now.
    let mut added_matches: BTreeMap<String, i64> = BTreeMap::new();
    for logged in records {
        let written = write_one(&mut tx, project, logged).await?;
        added_count += written.rows;
        added_bytes += written.bytes;
        if let Some(credited) = written.credited {
            *added_matches.entry(credited).or_default() += 1;
        }
        if let Some(displaced) = written.displaced {
            *added_matches.entry(displaced).or_default() -= 1;
        }
    }
    let through_i64 = i64::try_from(through.get()).map_err(|_| StoreError::Corrupt {
        table: "nineveh.record_cursors".into(),
        reason: "version past i64".into(),
    })?;
    // Postgres adds the per-source counts, so a re-read after a crash can't double
    // them: an overwrite of an identical record contributes nothing to `added_matches`.
    // The floor at zero is for logs written before the arithmetic above was right,
    // where a displaced record can be subtracted from a total that never counted it.
    // A count is a count; it should not be able to read as less than none.
    let matches = serde_json::to_string(&added_matches).map_err(|e| StoreError::Corrupt {
        table: "nineveh.record_cursors".into(),
        reason: format!("encoding source counts: {e}"),
    })?;
    sqlx::query!(
        r#"INSERT INTO nineveh.record_cursors
               (project, cursor, lock_hash, sources, record_count, record_bytes, matched)
           VALUES ($1, $2, $3, $4, $5, $6, $7::text::jsonb)
           ON CONFLICT (project) DO UPDATE
               SET cursor = excluded.cursor,
                   lock_hash = excluded.lock_hash,
                   sources = excluded.sources,
                   record_count = nineveh.record_cursors.record_count + excluded.record_count,
                   record_bytes = nineveh.record_cursors.record_bytes + excluded.record_bytes,
                   matched = (
                       SELECT coalesce(jsonb_object_agg(source, greatest(total, 0)), '{}'::jsonb)
                       FROM (
                           SELECT key AS source, sum(value::bigint) AS total
                           FROM (
                               SELECT key, value
                               FROM jsonb_each_text(nineveh.record_cursors.matched)
                               UNION ALL
                               SELECT key, value FROM jsonb_each_text(excluded.matched)
                           ) AS pairs
                           GROUP BY key
                       ) AS summed
                   ),
                   updated_at = now()"#,
        project,
        through_i64,
        lock_hash,
        sources.join(","),
        added_count,
        added_bytes,
        matches,
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

/// How many records each source has ever matched, by source name.
///
/// A source missing from the map has matched nothing — which is not an error, but is
/// almost always a type name that is subtly wrong rather than a genuinely quiet
/// contract. Cumulative, so pruning the log doesn't make a busy source look silent.
///
/// # Errors
///
/// If the database fails, or the stored counts aren't a map of numbers.
pub async fn matched(pool: &PgPool, project: &str) -> Result<BTreeMap<String, i64>, StoreError> {
    let row = sqlx::query_scalar!(
        r#"SELECT matched::text AS "matched!" FROM nineveh.record_cursors WHERE project = $1"#,
        project
    )
    .fetch_optional(pool)
    .await?;
    let Some(json) = row else {
        return Ok(BTreeMap::new());
    };
    serde_json::from_str(&json).map_err(|e| StoreError::Corrupt {
        table: "nineveh.record_cursors".into(),
        reason: format!("reading source counts: {e}"),
    })
}

/// How many records and how many bytes `project` has logged — the number a tier is
/// metered on, and the one Studio shows.
///
/// Read from the running totals rather than measured. Measuring meant a full heap scan
/// with a detoast per row — ten seconds on a log of 860,000 records — on a query Studio
/// polls and the retention sweep calls, which made an over-quota project the slowest
/// one to ask about.
///
/// # Errors
///
/// If the database fails.
pub async fn usage(pool: &PgPool, project: &str) -> Result<(i64, i64), StoreError> {
    let row = sqlx::query!(
        "SELECT record_count, record_bytes FROM nineveh.record_cursors WHERE project = $1",
        project
    )
    .fetch_optional(pool)
    .await?;
    Ok(row.map_or((0, 0), |r| (r.record_count, r.record_bytes)))
}
