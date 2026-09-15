use nineveh_core::Version;

/// Why a store operation failed.
#[derive(Debug, thiserror::Error)]
#[non_exhaustive]
pub enum StoreError {
    #[error("database error: {0}")]
    Database(#[from] sqlx::Error),

    #[error("migrating Nineveh's tables failed: {0}")]
    Migrate(#[from] sqlx::migrate::MigrateError),

    #[error("`{0}` isn't a valid schema name (lower snake case, at most 63 characters)")]
    InvalidSchema(String),

    #[error("schema `{0}` is reserved for Nineveh or Postgres; choose another name")]
    ReservedSchema(String),

    /// The schema was built from a different config, lock, fold semantics or store
    /// layout. Extending it would mix two builds, so it has to be rebuilt.
    #[error(
        "schema `{schema}` was built from a different config, lock or Nineveh version; \
         replay the project to rebuild it"
    )]
    Rebuild { schema: String },

    #[error("schema `{0}` has no project row; it was reset while this store was open")]
    Missing(String),

    /// Something else committed to this schema. There must be one writer per project
    /// (ADR 0005), so this stops rather than risk interleaving two folds.
    #[error(
        "the cursor for `{schema}` moved from {expected:?} to {found:?}: another writer is \
         committing to this project"
    )]
    CursorMoved {
        schema: String,
        expected: Option<u64>,
        found: Option<u64>,
    },

    #[error("the batch ends at version {last}, which isn't after the cursor {cursor}")]
    Stale { last: Version, cursor: Version },

    #[error("version {0} doesn't fit a Postgres bigint")]
    VersionRange(Version),

    /// A row doesn't fit its table's columns. Config resolution rules this out, so it
    /// means a bug.
    #[error("internal error: table `{table}`, column `{column}`: {reason}")]
    Mismatch {
        table: String,
        column: String,
        reason: String,
    },

    /// Stored bytes didn't decode: the table was changed by something other than
    /// Nineveh.
    #[error("table `{table}` holds a row Nineveh didn't write: {reason}")]
    Corrupt { table: String, reason: String },

    #[error("internal error: table `{0}` isn't read by the fold")]
    Unreadable(String),
}

impl StoreError {
    /// Whether the same operation can succeed if tried again: connection trouble,
    /// serialization conflicts and a database that's shutting down or out of
    /// resources. Everything else fails the same way again.
    #[must_use]
    pub fn is_retryable(&self) -> bool {
        match self {
            Self::Database(e) | Self::Migrate(sqlx::migrate::MigrateError::Execute(e)) => {
                retryable(e)
            }
            _ => false,
        }
    }
}

fn retryable(e: &sqlx::Error) -> bool {
    match e {
        sqlx::Error::Io(_) | sqlx::Error::PoolTimedOut | sqlx::Error::WorkerCrashed => true,
        sqlx::Error::Database(db) => db.code().is_some_and(|code| {
            // Connection exceptions, transaction rollbacks (serialization failures,
            // deadlocks), insufficient resources, and operator intervention
            // (shutdowns, "cannot connect now").
            ["08", "40", "53", "57P"]
                .iter()
                .any(|class| code.starts_with(class))
        }),
        _ => false,
    }
}
