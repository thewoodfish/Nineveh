use std::collections::{BTreeMap, BTreeSet, HashMap};
use std::fmt::Write as _;

use nineveh_config::{Named, Project};
use nineveh_core::{Value, Version};
use nineveh_decode::Lockfile;
use nineveh_engine::{
    ChangeKind, ChangeSet, Key, Lookup, Row, SEMANTICS_VERSION, StateView, TableId,
};
use sha2::{Digest, Sha256};
use sqlx::{PgPool, Postgres, Transaction};

use crate::cells::{self, Cells, Mismatch};
use crate::codec;
use crate::error::StoreError;
use crate::layout::{LAYOUT_VERSION, Table, ident};

static MIGRATOR: sqlx::migrate::Migrator = sqlx::migrate!();

/// The channel a commit notifies, with the schema name as payload. It's a wake-up
/// only: consumers read the outbox (ADR 0006).
pub const NOTIFY_CHANNEL: &str = "nineveh_changes";

/// Create or upgrade Nineveh's own tables. [`Store::open`] runs this too; it's safe to
/// run concurrently and more than once.
///
/// # Errors
///
/// If the database refuses the migration.
pub async fn migrate(pool: &PgPool) -> Result<(), StoreError> {
    let mut conn = pool.acquire().await?;
    // sqlx keeps its ledger of applied migrations in an unqualified `_sqlx_migrations`,
    // found through the search path. The default path starts with "$user": for a role
    // named `nineveh`, the schema the first migration creates would come first, and
    // every later run would find an empty ledger there and apply everything again.
    // So the ledger is always in `public`.
    sqlx::query("SET search_path TO public")
        .execute(&mut *conn)
        .await?;
    let migrated = MIGRATOR.run_direct(&mut *conn).await;
    sqlx::query("RESET search_path").execute(&mut *conn).await?;
    migrated?;
    Ok(())
}

/// One project's state in Postgres: its schema, and the only writer to it.
///
/// [`Store::load`] serves the fold the rows a batch reads, and [`Store::commit`] writes
/// the fold's [`ChangeSet`] in one transaction: rows, outbox and cursor together
/// (ADR 0005). After a crash, [`Store::open`] resumes from the committed cursor.
#[derive(Debug)]
pub struct Store {
    pool: PgPool,
    schema: String,
    tables: Vec<Table>,
    cursor: Option<Version>,
}

impl Store {
    /// Open the project's state in `schema`, creating the schema and its tables if
    /// they don't exist.
    ///
    /// # Errors
    ///
    /// [`StoreError::Rebuild`] if the schema was built from a different config, lock
    /// or Nineveh version: rebuild it into [`shadow_name`] and [`Store::swap`] it in
    /// (ADR 0016). Otherwise, if the database fails.
    pub async fn open(
        pool: PgPool,
        schema: &str,
        project: &Project,
        lock: &Lockfile,
    ) -> Result<Self, StoreError> {
        check_schema(schema)?;
        migrate(&pool).await?;
        let tables = Table::all(project);
        let fingerprint = fingerprint(project, lock)?;

        let mut tx = pool.begin().await?;
        lock_schema(&mut tx, schema).await?;
        let existing = sqlx::query!(
            "SELECT fingerprint, cursor FROM nineveh.projects WHERE schema_name = $1",
            schema
        )
        .fetch_optional(&mut *tx)
        .await?;
        let cursor = match existing {
            Some(row) if row.fingerprint == fingerprint => {
                row.cursor.map(|v| from_i64(schema, v)).transpose()?
            }
            Some(_) => {
                return Err(StoreError::Rebuild {
                    schema: schema.to_owned(),
                });
            }
            None => {
                let mut ddl = vec![format!("CREATE SCHEMA {}", ident(schema))];
                for table in &tables {
                    ddl.extend(table.create(schema));
                }
                for statement in &ddl {
                    unprepared(&mut tx, statement).await?;
                }
                let config = project.config();
                sqlx::query!(
                    "INSERT INTO nineveh.projects (schema_name, project, network, fingerprint)
                     VALUES ($1, $2, $3, $4)",
                    schema,
                    config.name.as_str(),
                    config.network.as_str(),
                    fingerprint,
                )
                .execute(&mut *tx)
                .await?;
                None
            }
        };
        tx.commit().await?;
        Ok(Self {
            pool,
            schema: schema.to_owned(),
            tables,
            cursor,
        })
    }

    /// Drop a project's state: its schema, cursor and change feed. The next
    /// [`Store::open`] builds it afresh from `start_version`.
    ///
    /// # Errors
    ///
    /// If the database fails.
    pub async fn reset(pool: &PgPool, schema: &str) -> Result<(), StoreError> {
        check_schema(schema)?;
        migrate(pool).await?;
        let mut tx = pool.begin().await?;
        lock_schema(&mut tx, schema).await?;
        let drop = format!("DROP SCHEMA IF EXISTS {} CASCADE", ident(schema));
        unprepared(&mut tx, &drop).await?;
        sqlx::query!(
            "DELETE FROM nineveh.projects WHERE schema_name = $1",
            schema
        )
        .execute(&mut *tx)
        .await?;
        tx.commit().await?;
        Ok(())
    }

    /// Replace the build in `live` with the one in `shadow`, in one transaction: the
    /// shadow's tables, cursor and change feed move to `live`, and the old build is
    /// dropped (ADR 0016). Readers see the old build or the new one, never a mix.
    ///
    /// # Errors
    ///
    /// [`StoreError::Missing`] if `shadow` has no build, or if the database fails.
    pub async fn swap(pool: &PgPool, live: &str, shadow: &str) -> Result<(), StoreError> {
        check_schema(live)?;
        check_schema(shadow)?;
        migrate(pool).await?;
        let mut tx = pool.begin().await?;
        // Always in this order, so two swaps can't deadlock.
        lock_schema(&mut tx, live).await?;
        lock_schema(&mut tx, shadow).await?;
        sqlx::query_scalar!(
            "SELECT schema_name FROM nineveh.projects WHERE schema_name = $1 FOR UPDATE",
            shadow
        )
        .fetch_optional(&mut *tx)
        .await?
        .ok_or_else(|| StoreError::Missing(shadow.to_owned()))?;

        let drop = format!("DROP SCHEMA IF EXISTS {} CASCADE", ident(live));
        unprepared(&mut tx, &drop).await?;
        let rename = format!("ALTER SCHEMA {} RENAME TO {}", ident(shadow), ident(live));
        unprepared(&mut tx, &rename).await?;
        // The old build's row goes, and its change feed with it (ON DELETE CASCADE).
        // The shadow's row and feed are re-keyed to `live`.
        sqlx::query!("DELETE FROM nineveh.projects WHERE schema_name = $1", live)
            .execute(&mut *tx)
            .await?;
        sqlx::query!(
            "INSERT INTO nineveh.projects
                 (schema_name, project, network, fingerprint, cursor, created_at, updated_at)
             SELECT $1, project, network, fingerprint, cursor, created_at, now()
             FROM nineveh.projects WHERE schema_name = $2",
            live,
            shadow
        )
        .execute(&mut *tx)
        .await?;
        sqlx::query!(
            "UPDATE nineveh.changes SET schema_name = $1 WHERE schema_name = $2",
            live,
            shadow
        )
        .execute(&mut *tx)
        .await?;
        sqlx::query!(
            "DELETE FROM nineveh.projects WHERE schema_name = $1",
            shadow
        )
        .execute(&mut *tx)
        .await?;
        sqlx::query("SELECT pg_notify($1, $2)")
            .bind(NOTIFY_CHANNEL)
            .bind(live)
            .execute(&mut *tx)
            .await?;
        tx.commit().await?;
        Ok(())
    }

    #[must_use]
    pub fn schema(&self) -> &str {
        &self.schema
    }

    /// The last committed version, or `None` before the first commit.
    #[must_use]
    pub fn cursor(&self) -> Option<Version> {
        self.cursor
    }

    /// Read the rows at `keys`. Each key comes back present or absent; the fold
    /// reports any other key it needs as not loaded (ADR 0013).
    ///
    /// # Errors
    ///
    /// If the database fails, or a key names a table the fold doesn't read.
    pub async fn load(
        &self,
        keys: impl IntoIterator<Item = (TableId, Key)>,
    ) -> Result<Loaded, StoreError> {
        let mut by_table: BTreeMap<TableId, BTreeSet<Key>> = BTreeMap::new();
        for (table, key) in keys {
            by_table.entry(table).or_default().insert(key);
        }
        let mut loaded = Loaded::default();
        for (id, keys) in by_table {
            let table = self.readable(id)?;
            let encoded: Vec<Vec<u8>> = keys.iter().map(|k| codec::encode(k)).collect();
            let found: HashMap<Vec<u8>, Vec<u8>> = sqlx::query_as(&table.select(&self.schema))
                .bind(&encoded)
                .fetch_all(&self.pool)
                .await?
                .into_iter()
                .collect();
            for (key, bytes) in keys.into_iter().zip(&encoded) {
                let row = found.get(bytes).map(|row| decode(table, row)).transpose()?;
                loaded.rows.insert((id, key), row);
            }
        }
        Ok(loaded)
    }

    /// The newest version that changed a row in any of this project's state tables,
    /// or `None` if none of them holds a row.
    ///
    /// Where the contract was last doing something, as far as this project is
    /// concerned: what a preview reads a window around, rather than guessing one at
    /// the chain's tip.
    ///
    /// # Errors
    ///
    /// If the database fails.
    pub async fn latest_change(&self) -> Result<Option<Version>, StoreError> {
        let mut newest: Option<i64> = None;
        for table in self.tables.iter().filter(|t| t.schema.is_some()) {
            let sql = format!(
                "SELECT max({}) FROM {}",
                ident("_version"),
                crate::layout::qualified(&self.schema, &table.name)
            );
            let version: Option<i64> = sqlx::query_scalar(&sql).fetch_one(&self.pool).await?;
            newest = newest.max(version);
        }
        Ok(newest.and_then(|v| u64::try_from(v).ok()).map(Version::new))
    }

    /// Every row of a table, in no particular order, with the engine's row for tables
    /// the fold reads (`None` for `log` tables, which don't keep it). Reads the whole
    /// table into memory: for tests and tools, not the pipeline.
    ///
    /// # Errors
    ///
    /// If the database fails, or `table` isn't one of the project's.
    pub async fn scan(&self, id: TableId) -> Result<Vec<(Key, Option<Row>)>, StoreError> {
        let table = self.table(id)?;
        let rows: Vec<(Vec<u8>, Option<Vec<u8>>)> = if table.readable {
            sqlx::query_as(&format!(
                "SELECT {}, {} FROM {}",
                ident("_key"),
                ident("_row"),
                crate::layout::qualified(&self.schema, &table.name)
            ))
            .fetch_all(&self.pool)
            .await?
        } else {
            sqlx::query_as(&format!(
                "SELECT {}, NULL::bytea FROM {}",
                ident("_key"),
                crate::layout::qualified(&self.schema, &table.name)
            ))
            .fetch_all(&self.pool)
            .await?
        };
        rows.into_iter()
            .map(|(key, row)| {
                Ok((
                    decode(table, &key)?,
                    row.map(|r| decode(table, &r)).transpose()?,
                ))
            })
            .collect()
    }

    /// Commit a folded batch in one transaction: its row writes, its changes to the
    /// outbox, and the cursor. Either all of it lands or none of it does, so a crash
    /// at any point resumes from a cursor that matches the state (ADR 0005).
    ///
    /// A batch with no transactions commits nothing.
    ///
    /// # Errors
    ///
    /// [`StoreError::CursorMoved`] if another writer committed to this schema since it
    /// was opened, [`StoreError::Stale`] if the batch doesn't come after the cursor,
    /// or a database error (see [`StoreError::is_retryable`]).
    pub async fn commit(&mut self, changes: &ChangeSet) -> Result<(), StoreError> {
        let Some(last) = changes.last_version else {
            return Ok(());
        };
        if let Some(cursor) = self.cursor
            && last <= cursor
        {
            return Err(StoreError::Stale { last, cursor });
        }
        let last_i64 = to_i64(last)?;
        let writes = self.writes(changes)?;
        let outbox = self.outbox(changes)?;

        let mut tx = self.pool.begin().await?;
        let found = sqlx::query_scalar!(
            "SELECT cursor FROM nineveh.projects WHERE schema_name = $1 FOR UPDATE",
            self.schema
        )
        .fetch_optional(&mut *tx)
        .await?
        .ok_or_else(|| StoreError::Missing(self.schema.clone()))?;
        let expected = self.cursor.map(to_i64).transpose()?;
        if found != expected {
            return Err(StoreError::CursorMoved {
                schema: self.schema.clone(),
                expected: self.cursor.map(Version::get),
                found: found.and_then(|v| u64::try_from(v).ok()),
            });
        }

        for (table, writes) in writes {
            self.write_table(&mut tx, table, writes).await?;
        }
        if !outbox.versions.is_empty() {
            sqlx::query!(
                "INSERT INTO nineveh.changes
                     (schema_name, version, seq, table_name, op, key, new_row)
                 SELECT $1::text, * FROM UNNEST(
                     $2::bigint[], $3::integer[], $4::text[], $5::text[],
                     $6::text[]::jsonb[], $7::text[]::jsonb[])",
                self.schema,
                &outbox.versions,
                &outbox.seqs,
                &outbox.tables,
                &outbox.ops,
                &outbox.keys,
                &outbox.rows as &[Option<String>],
            )
            .execute(&mut *tx)
            .await?;
        }
        sqlx::query!(
            "UPDATE nineveh.projects SET cursor = $2, updated_at = now()
             WHERE schema_name = $1",
            self.schema,
            last_i64
        )
        .execute(&mut *tx)
        .await?;
        if !outbox.versions.is_empty() {
            sqlx::query("SELECT pg_notify($1, $2)")
                .bind(NOTIFY_CHANNEL)
                .bind(&self.schema)
                .execute(&mut *tx)
                .await?;
        }
        tx.commit().await?;
        self.cursor = Some(last);
        Ok(())
    }

    /// A change set's row writes, grouped by table and encoded.
    fn writes<'c>(
        &self,
        changes: &'c ChangeSet,
    ) -> Result<BTreeMap<TableId, TableWrites<'c>>, StoreError> {
        // Each state row is stamped with the version of its last change.
        let mut versions: HashMap<(u32, &Key), Version> = HashMap::new();
        for change in &changes.changes {
            versions.insert((change.table, &change.key), change.version);
        }
        let fallback = changes.last_version.unwrap_or(Version::GENESIS);
        let mut out: BTreeMap<TableId, TableWrites<'c>> = BTreeMap::new();
        for ((id, key), row) in &changes.writes {
            let writes = out.entry(*id).or_default();
            let encoded = codec::encode(key);
            match row {
                None => writes.deletes.push(encoded),
                Some(row) => {
                    let version = match id {
                        TableId::State(t) => versions.get(&(*t, key)).copied().unwrap_or(fallback),
                        _ => fallback,
                    };
                    writes.keys.push(encoded);
                    writes.rows.push(row);
                    writes.versions.push(to_i64(version)?);
                }
            }
        }
        // Every table must be one of the project's.
        for id in out.keys() {
            self.table(*id)?;
        }
        Ok(out)
    }

    async fn write_table(
        &self,
        tx: &mut Transaction<'static, Postgres>,
        id: TableId,
        writes: TableWrites<'_>,
    ) -> Result<(), StoreError> {
        let table = self.table(id)?;
        if !writes.deletes.is_empty() {
            sqlx::query(&table.delete(&self.schema))
                .bind(&writes.deletes)
                .execute(&mut **tx)
                .await?;
        }
        if writes.keys.is_empty() {
            return Ok(());
        }
        let sql = table.upsert(&self.schema);
        let mut query = sqlx::query(&sql).bind(writes.keys);
        if table.readable {
            let rows: Vec<Vec<u8>> = writes.rows.iter().map(|r| codec::encode(r)).collect();
            query = query.bind(rows);
        }
        if let Some(schema) = &table.schema {
            query = query.bind(writes.versions);
            let rows: Vec<&[Value]> = writes.rows.iter().map(|r| r.as_slice()).collect();
            let columns = cells::columns(schema, &rows).map_err(|m| mismatch(table, m))?;
            for cells in columns {
                query = match cells {
                    Cells::Bool(v) => query.bind(v),
                    Cells::Int4(v) => query.bind(v),
                    Cells::Int8(v) => query.bind(v),
                    Cells::Text(v) => query.bind(v),
                    Cells::Bytea(v) => query.bind(v),
                };
            }
        }
        query.execute(&mut **tx).await?;
        Ok(())
    }

    /// The outbox rows for a change set's changes. `seq` numbers each version's
    /// changes from zero, so a change's `(version, seq)` is the same however the stream
    /// was batched.
    fn outbox(&self, changes: &ChangeSet) -> Result<Outbox, StoreError> {
        let mut outbox = Outbox::default();
        let mut seq = 0i32;
        let mut previous = None;
        for change in &changes.changes {
            if previous != Some(change.version) {
                previous = Some(change.version);
                seq = 0;
            }
            let table = self.table(TableId::State(change.table))?;
            let Some(schema) = &table.schema else {
                return Err(StoreError::Unreadable(table.name.clone()));
            };
            outbox.versions.push(to_i64(change.version)?);
            outbox.seqs.push(seq);
            seq = seq
                .checked_add(1)
                .ok_or(StoreError::VersionRange(change.version))?;
            outbox.tables.push(table.name.clone());
            outbox.ops.push(
                match change.kind {
                    ChangeKind::Inserted => "insert",
                    ChangeKind::Updated => "update",
                    ChangeKind::Deleted => "delete",
                }
                .to_owned(),
            );
            let key = cells::key_json(schema, &change.key).map_err(|m| mismatch(table, m))?;
            outbox.keys.push(key.to_string());
            let row = change
                .row
                .as_ref()
                .map(|row| cells::row_json(schema, row))
                .transpose()
                .map_err(|m| mismatch(table, m))?;
            outbox.rows.push(row.map(|r| r.to_string()));
        }
        Ok(outbox)
    }

    fn table(&self, id: TableId) -> Result<&Table, StoreError> {
        self.tables
            .iter()
            .find(|t| t.id == id)
            .ok_or_else(|| StoreError::Unreadable(format!("{id:?}")))
    }

    fn readable(&self, id: TableId) -> Result<&Table, StoreError> {
        let table = self.table(id)?;
        if table.readable {
            Ok(table)
        } else {
            Err(StoreError::Unreadable(table.name.clone()))
        }
    }
}

#[derive(Default)]
struct TableWrites<'c> {
    deletes: Vec<Vec<u8>>,
    keys: Vec<Vec<u8>>,
    rows: Vec<&'c Row>,
    versions: Vec<i64>,
}

#[derive(Default)]
struct Outbox {
    versions: Vec<i64>,
    seqs: Vec<i32>,
    tables: Vec<String>,
    ops: Vec<String>,
    keys: Vec<String>,
    rows: Vec<Option<String>>,
}

/// Rows read from the store, as a [`StateView`] for the fold.
///
/// Keys it was asked for are present or absent; any other key is not loaded, so the
/// fold reports it and the caller loads it and folds again (ADR 0013). Because the
/// store is the project's only writer, a `Loaded` kept up to date with
/// [`Loaded::apply`] stays exact across batches and can serve as a cache.
#[derive(Debug, Clone, Default)]
pub struct Loaded {
    rows: HashMap<(TableId, Key), Option<Row>>,
}

impl Loaded {
    /// Add rows from another load.
    pub fn merge(&mut self, other: Self) {
        self.rows.extend(other.rows);
    }

    /// Record a committed batch's writes, so the rows stay current.
    pub fn apply(&mut self, changes: &ChangeSet) {
        for (key, row) in &changes.writes {
            self.rows.insert(key.clone(), row.clone());
        }
    }

    /// How many keys are held, present or absent.
    #[must_use]
    pub fn len(&self) -> usize {
        self.rows.len()
    }

    #[must_use]
    pub fn is_empty(&self) -> bool {
        self.rows.is_empty()
    }

    pub fn clear(&mut self) {
        self.rows.clear();
    }
}

impl StateView for Loaded {
    fn get(&self, table: TableId, key: &[Value]) -> Lookup {
        match self.rows.get(&(table, key.to_vec())) {
            Some(Some(row)) => Lookup::Present(row.clone()),
            Some(None) => Lookup::Absent,
            None => Lookup::NotLoaded,
        }
    }
}

/// Everything that shapes a build's state, hashed: the store's layout, the fold's
/// semantics, the config's canonical form and the lock. Each part is length-prefixed
/// so no two builds hash the same input.
fn fingerprint(project: &Project, lock: &Lockfile) -> Result<String, StoreError> {
    let lock = lock.to_json().map_err(|e| StoreError::Corrupt {
        table: "nineveh.lock".into(),
        reason: e.to_string(),
    })?;
    let parts = [
        format!("layout {LAYOUT_VERSION}"),
        format!("semantics {SEMANTICS_VERSION}"),
        project.config().canonical(),
        lock,
    ];
    let mut hasher = Sha256::new();
    for part in &parts {
        hasher.update(u64::try_from(part.len()).unwrap_or(u64::MAX).to_be_bytes());
        hasher.update(part.as_bytes());
    }
    let mut hex = String::with_capacity(64);
    for byte in hasher.finalize() {
        // Writing to a String can't fail.
        let _ = write!(hex, "{byte:02x}");
    }
    Ok(hex)
}

/// Run one DDL statement without preparing it: each runs once per schema, so there's
/// nothing to cache. Not `sqlx::raw_sql`, whose future isn't `Send`, which would keep
/// the pipeline off spawned tasks.
pub(crate) async fn unprepared(
    tx: &mut Transaction<'static, Postgres>,
    statement: &str,
) -> Result<(), StoreError> {
    sqlx::query(statement)
        .persistent(false)
        .execute(&mut **tx)
        .await?;
    Ok(())
}

/// Serialize `open` and `reset` on one schema across processes.
pub(crate) async fn lock_schema(
    tx: &mut Transaction<'static, Postgres>,
    schema: &str,
) -> Result<(), StoreError> {
    sqlx::query!(
        "SELECT pg_advisory_xact_lock(hashtextextended('nineveh:' || $1, 0))",
        schema
    )
    .fetch_one(&mut **tx)
    .await?;
    Ok(())
}

/// The suffix of the schema a rebuild of `schema` goes into (ADR 0016).
const SHADOW_SUFFIX: &str = "__next";

/// The schema a rebuild of `live` builds into before [`Store::swap`] moves it in.
///
/// # Errors
///
/// If `live` isn't a valid schema name, is itself a shadow, or is too long to take
/// the suffix.
pub fn shadow_name(live: &str) -> Result<String, StoreError> {
    check_schema(live)?;
    if live.ends_with(SHADOW_SUFFIX) {
        return Err(StoreError::ReservedSchema(live.to_owned()));
    }
    let shadow = format!("{live}{SHADOW_SUFFIX}");
    check_schema(&shadow)?;
    Ok(shadow)
}

/// Refuse names that aren't identifiers, and schemas that belong to Nineveh or
/// Postgres: `reset` drops the schema it's given, cascading.
pub(crate) fn check_schema(schema: &str) -> Result<(), StoreError> {
    if !Named::is_valid(schema) {
        return Err(StoreError::InvalidSchema(schema.to_owned()));
    }
    if matches!(schema, "nineveh" | "public" | "information_schema") || schema.starts_with("pg_") {
        return Err(StoreError::ReservedSchema(schema.to_owned()));
    }
    Ok(())
}

fn decode(table: &Table, bytes: &[u8]) -> Result<Vec<Value>, StoreError> {
    codec::decode(bytes).map_err(|e| StoreError::Corrupt {
        table: table.name.clone(),
        reason: e.to_string(),
    })
}

fn mismatch(table: &Table, m: Mismatch) -> StoreError {
    StoreError::Mismatch {
        table: table.name.clone(),
        column: m.column,
        reason: m.reason,
    }
}

fn to_i64(version: Version) -> Result<i64, StoreError> {
    i64::try_from(version.get()).map_err(|_| StoreError::VersionRange(version))
}

fn from_i64(schema: &str, version: i64) -> Result<Version, StoreError> {
    u64::try_from(version)
        .map(Version::new)
        .map_err(|_| StoreError::Corrupt {
            table: format!("nineveh.projects ({schema})"),
            reason: format!("negative cursor {version}"),
        })
}
