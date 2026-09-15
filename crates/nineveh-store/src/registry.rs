//! The control plane's registry of projects (ADR 0017): each one's config and lock,
//! and whether it should run. Its state lives in the schema of its name.

use sqlx::PgPool;

use crate::error::StoreError;
use crate::layout::ident;
use crate::store::{check_schema, lock_schema, migrate, shadow_name, unprepared};

/// A registered project.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Registered {
    pub name: String,
    pub network: String,
    pub config: String,
    pub lock: String,
    pub running: bool,
    /// The account that owns it; `None` for a project made in local mode.
    pub owner_id: Option<i64>,
    /// RFC 3339, UTC.
    pub created_at: String,
    pub updated_at: String,
}

/// Every registered project, by name.
///
/// # Errors
///
/// If the database fails.
pub async fn list(pool: &PgPool) -> Result<Vec<Registered>, StoreError> {
    migrate(pool).await?;
    let rows = sqlx::query!(
        r#"SELECT name, network, config, lock, running, owner_id,
                  to_char(created_at AT TIME ZONE 'UTC', 'YYYY-MM-DD"T"HH24:MI:SS.MS"Z"') AS "created_at!",
                  to_char(updated_at AT TIME ZONE 'UTC', 'YYYY-MM-DD"T"HH24:MI:SS.MS"Z"') AS "updated_at!"
           FROM nineveh.control_projects ORDER BY name"#
    )
    .fetch_all(pool)
    .await?;
    Ok(rows
        .into_iter()
        .map(|r| Registered {
            name: r.name,
            network: r.network,
            config: r.config,
            lock: r.lock,
            running: r.running,
            owner_id: r.owner_id,
            created_at: r.created_at,
            updated_at: r.updated_at,
        })
        .collect())
}

/// Register a new project, owned by `owner` (`None` in local mode).
///
/// # Errors
///
/// [`StoreError::ProjectExists`] if the name is registered, or a build of that name
/// exists outside the registry. Otherwise, if the database fails.
pub async fn insert(
    pool: &PgPool,
    name: &str,
    network: &str,
    config: &str,
    lock: &str,
    owner: Option<i64>,
) -> Result<(), StoreError> {
    check_schema(name)?;
    let shadow = shadow_name(name)?;
    migrate(pool).await?;
    let mut tx = pool.begin().await?;
    lock_schema(&mut tx, name).await?;
    let taken = sqlx::query_scalar!(
        r#"SELECT EXISTS (SELECT 1 FROM nineveh.control_projects WHERE name = $1)
               OR EXISTS (SELECT 1 FROM nineveh.projects WHERE schema_name IN ($1, $2))
               OR EXISTS (SELECT 1 FROM pg_namespace WHERE nspname IN ($1, $2)) AS "taken!""#,
        name,
        shadow
    )
    .fetch_one(&mut *tx)
    .await?;
    if taken {
        return Err(StoreError::ProjectExists(name.to_owned()));
    }
    sqlx::query!(
        "INSERT INTO nineveh.control_projects (name, network, config, lock, owner_id)
         VALUES ($1, $2, $3, $4, $5)",
        name,
        network,
        config,
        lock,
        owner
    )
    .execute(&mut *tx)
    .await?;
    tx.commit().await?;
    Ok(())
}

/// Replace a project's config and lock.
///
/// # Errors
///
/// [`StoreError::NoProject`] if it isn't registered, or if the database fails.
pub async fn update(pool: &PgPool, name: &str, config: &str, lock: &str) -> Result<(), StoreError> {
    let updated = sqlx::query!(
        "UPDATE nineveh.control_projects SET config = $2, lock = $3, updated_at = now()
         WHERE name = $1",
        name,
        config,
        lock
    )
    .execute(pool)
    .await?;
    if updated.rows_affected() == 0 {
        return Err(StoreError::NoProject(name.to_owned()));
    }
    Ok(())
}

/// Record whether a project should run.
///
/// # Errors
///
/// [`StoreError::NoProject`] if it isn't registered, or if the database fails.
pub async fn set_running(pool: &PgPool, name: &str, running: bool) -> Result<(), StoreError> {
    let updated = sqlx::query!(
        "UPDATE nineveh.control_projects SET running = $2, updated_at = now() WHERE name = $1",
        name,
        running
    )
    .execute(pool)
    .await?;
    if updated.rows_affected() == 0 {
        return Err(StoreError::NoProject(name.to_owned()));
    }
    Ok(())
}

/// Unregister a project and drop its state: its schema, any rebuild in progress, their
/// cursors and change feeds. Stop its pipeline first.
///
/// # Errors
///
/// [`StoreError::NoProject`] if it isn't registered, or if the database fails.
pub async fn delete(pool: &PgPool, name: &str) -> Result<(), StoreError> {
    check_schema(name)?;
    let shadow = shadow_name(name)?;
    let mut tx = pool.begin().await?;
    lock_schema(&mut tx, name).await?;
    lock_schema(&mut tx, &shadow).await?;
    let deleted = sqlx::query!("DELETE FROM nineveh.control_projects WHERE name = $1", name)
        .execute(&mut *tx)
        .await?;
    if deleted.rows_affected() == 0 {
        return Err(StoreError::NoProject(name.to_owned()));
    }
    for schema in [name, shadow.as_str()] {
        let drop = format!("DROP SCHEMA IF EXISTS {} CASCADE", ident(schema));
        unprepared(&mut tx, &drop).await?;
    }
    // The feeds go with the build rows (ON DELETE CASCADE).
    sqlx::query!(
        "DELETE FROM nineveh.projects WHERE schema_name IN ($1, $2)",
        name,
        shadow
    )
    .execute(&mut *tx)
    .await?;
    tx.commit().await?;
    Ok(())
}
