//! Rebuilding a project from its own record log instead of from the chain (ADR 0022).
//!
//! A rebuild exists because state is derived: change the rules and the rows have to be
//! computed again from the same history. Getting that history back from the
//! Transaction Stream is the expensive part — a 90-day-old mainnet contract takes days
//! and holds one of Geomi's limited concurrent streams throughout — and it is charged
//! on the most ordinary action there is, editing a rule.
//!
//! The records are already kept, so the history doesn't have to be bought twice. This
//! reads them back and folds them through the same engine the pipeline uses, into the
//! shadow schema ADR 0016 swaps in.
//!
//! It refuses rather than guesses. The log has to reach back to the project's start and
//! forward to the target, and it has to have been decoded against the layouts in use
//! now; anything else falls back to the stream.

use nineveh_config::Project;
use nineveh_core::Version;
use nineveh_decode::{DecodedTransaction, Record};
use nineveh_engine::{ChangeSet, Engine};
use nineveh_store::records::{self, Logged};
use nineveh_store::{Loaded, Store};
use sqlx::PgPool;
use tracing::{debug, info};

use crate::error::PipelineError;
use crate::pipeline::fold;

/// Versions read from the log in one pass before folding and committing.
const BATCH_VERSIONS: u64 = 50_000;

/// Why the log can't serve a rebuild, when it can't.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Unavailable {
    /// Nothing has been logged for this project yet.
    Empty,
    /// The log starts after the version the rebuild has to start at.
    StartsLate {
        logged_from: Version,
        needed: Version,
    },
    /// The log stops before the version the rebuild has to reach.
    EndsEarly { logged_to: Version, needed: Version },
    /// The records were decoded against other layouts, so they can't be trusted to
    /// mean what this config says they mean.
    LockChanged,
    /// The config follows a source the log has no history for. Adding one is the
    /// exception to replaying locally: there is nothing logged for something that was
    /// never followed.
    SourceAdded { name: String },
}

impl std::fmt::Display for Unavailable {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            Self::Empty => write!(f, "nothing is logged for this project"),
            Self::StartsLate {
                logged_from,
                needed,
            } => {
                write!(f, "the log starts at {logged_from}, after {needed}")
            }
            Self::EndsEarly { logged_to, needed } => {
                write!(f, "the log reaches {logged_to}, short of {needed}")
            }
            Self::LockChanged => write!(f, "the records were decoded against other layouts"),
            Self::SourceAdded { name } => write!(f, "`{name}` was added and has no history here"),
        }
    }
}

/// Whether the log can rebuild `project` over `from..=through`.
///
/// # Errors
///
/// If the database fails.
pub async fn available(
    pool: &PgPool,
    project: &str,
    lock_hash: &str,
    sources: &[String],
    from: Version,
    through: Version,
) -> Result<Result<(), Unavailable>, PipelineError> {
    let Some(state) = records::state(pool, project).await? else {
        return Ok(Err(Unavailable::Empty));
    };
    if state.lock_hash != lock_hash {
        return Ok(Err(Unavailable::LockChanged));
    }
    // Removing a source is fine: its records are simply not replayed. Adding one is
    // not, because nothing followed it when these records were written.
    if let Some(added) = sources.iter().find(|s| !state.sources.contains(s)) {
        return Ok(Err(Unavailable::SourceAdded {
            name: added.clone(),
        }));
    }
    let Some(cursor) = state.cursor else {
        return Ok(Err(Unavailable::Empty));
    };
    if cursor < through {
        return Ok(Err(Unavailable::EndsEarly {
            logged_to: cursor,
            needed: through,
        }));
    }
    match records::first_version(pool, project).await? {
        None => Ok(Err(Unavailable::Empty)),
        Some(first) if first > from => Ok(Err(Unavailable::StartsLate {
            logged_from: first,
            needed: from,
        })),
        Some(_) => Ok(Ok(())),
    }
}

/// Fold `project`'s logged records over `from..=through` into `store`.
///
/// The caller has already checked [`available`]; this fails rather than silently
/// producing a partial build if the log turns out not to cover the range.
///
/// # Errors
///
/// If the database fails, a logged record doesn't decode, or a rule fails — the same
/// halt a run against the stream would give, at the same version.
pub async fn rebuild(
    pool: &PgPool,
    store: &mut Store,
    project: &Project,
    name: &str,
    from: Version,
    through: Version,
    cache_rows: usize,
) -> Result<(), PipelineError> {
    let engine = Engine::new(project);
    let mut cache = Loaded::default();
    let mut at = from;

    loop {
        let end = Version::new(at.get().saturating_add(BATCH_VERSIONS - 1)).min(through);
        let logged = records::read(pool, name, at, end).await?;
        let transactions = group(project, &logged)?;

        if transactions.is_empty() {
            // A stretch with nothing logged still has to move the cursor, or the next
            // run would read it again.
            let changes = ChangeSet {
                last_version: Some(end),
                ..ChangeSet::default()
            };
            store.commit(&changes).await?;
        } else {
            let mut changes = fold(&engine, store, &mut cache, &transactions).await?;
            changes.last_version = Some(end);
            store.commit(&changes).await?;
            cache.apply(&changes);
            if cache.len() > cache_rows {
                cache.clear();
            }
            debug!(
                %name,
                through = end.get(),
                records = logged.len(),
                "replayed a batch from the log"
            );
        }

        if end >= through {
            break;
        }
        at = end.next().ok_or(PipelineError::VersionOverflow)?;
    }
    info!(%name, through = through.get(), "rebuilt from the record log");
    Ok(())
}

/// Logged records back into the transactions the engine folds, one per version.
///
/// The log is read in `(version, ord)` order, which is the order the decoder emitted
/// them, so grouping consecutive runs of a version rebuilds exactly what the fold saw.
fn group(project: &Project, logged: &[Logged]) -> Result<Vec<DecodedTransaction>, PipelineError> {
    let mut out: Vec<DecodedTransaction> = Vec::new();
    for entry in logged {
        let Some(source) = project.source_by_name(&entry.record.source) else {
            // The config no longer has this source, so nothing folds its records. That
            // is a rebuild under a config that dropped a source, which is legitimate.
            continue;
        };
        let record = Record::from_stored(entry.record.clone(), source)
            .map_err(|e| PipelineError::Task(format!("logged record at {}: {e}", entry.version)))?;
        match out.last_mut() {
            Some(tx) if tx.version == entry.version => tx.records.push(record),
            _ => out.push(DecodedTransaction {
                version: entry.version,
                timestamp_micros: entry.timestamp_micros,
                success: entry.success,
                sender: None,
                records: vec![record],
            }),
        }
    }
    Ok(out)
}
