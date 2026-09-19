//! Running a project: build its state and keep it current, rebuilding into a shadow
//! schema when its config changes what's built (ADR 0016). `nineveh run` and the
//! control plane both run projects through [`Runner`].

use std::sync::Arc;
use std::time::{Duration, Instant, SystemTime, UNIX_EPOCH};

use nineveh_api::Health;
use nineveh_config::Project;
use nineveh_core::Version;
use nineveh_decode::Lockfile;
use nineveh_pipeline::{
    Outcome, Parallel, Pipeline, PipelineConfig, PipelineError, Status, replay,
};
use nineveh_store::{Store, StoreError, shadow_name};
use sqlx::PgPool;
use tokio::sync::watch;
use tracing::{info, warn};

use crate::chain::{Chain, ChainError};

/// How to run a project.
#[derive(Debug, Clone)]
pub struct RunOptions {
    /// Stop once this version is committed.
    pub until: Option<Version>,
    /// Streams for backfill: ranges up to the chain's current version are read this
    /// many at a time.
    pub streams: usize,
    /// Versions per backfill range.
    pub chunk: u64,
    /// Keep records without folding them into state (ADR 0023).
    ///
    /// Set for a project nothing is reading. The records still accrue — that is what
    /// keeps waking it a local replay rather than hours of re-streaming — and the
    /// state stays where it was until something asks for it.
    pub log_only: bool,
}

impl Default for RunOptions {
    fn default() -> Self {
        Self {
            until: None,
            log_only: false,
            streams: 4,
            chunk: 1_000_000,
        }
    }
}

/// Why a run ended without being stopped.
#[derive(Debug, thiserror::Error)]
#[non_exhaustive]
pub enum RunError {
    #[error(transparent)]
    Chain(#[from] ChainError),

    #[error(transparent)]
    Store(#[from] StoreError),

    /// A deterministic failure in the data: everything before `version` is committed.
    #[error("halted at version {version}: {source}")]
    Halted {
        version: Version,
        #[source]
        source: PipelineError,
    },

    #[error(transparent)]
    Pipeline(PipelineError),

    #[error("schema `{0}` changed while running; run again to rebuild it")]
    Changed(String),
}

impl RunError {
    /// Whether running again can get further: only transient chain or database
    /// failures. The pipeline retries its own transient failures before giving up.
    #[must_use]
    pub fn is_retryable(&self) -> bool {
        match self {
            Self::Chain(e) => e.is_retryable(),
            Self::Store(e) => e.is_retryable(),
            Self::Pipeline(e) => e.is_retryable(),
            Self::Halted { .. } | Self::Changed(_) => false,
        }
    }
}

impl From<PipelineError> for RunError {
    fn from(error: PipelineError) -> Self {
        if let Some(version) = error.halted_at() {
            return Self::Halted {
                version,
                source: error,
            };
        }
        match error {
            PipelineError::Store(StoreError::Rebuild { schema }) => Self::Changed(schema),
            other => Self::Pipeline(other),
        }
    }
}

/// A project ready to run: its config resolved against its lock, and where a new
/// build starts.
pub struct Runner<C> {
    chain: Arc<C>,
    project: Arc<Project>,
    lock: Arc<Lockfile>,
    start: Version,
    schema: String,
    pool: PgPool,
    /// The chain's version when the run began: where backfill ends and a rebuild
    /// swaps in.
    tip: Version,
    options: RunOptions,
    /// The pipeline's health, for the API.
    health: watch::Sender<Option<Health>>,
}

impl<C> std::fmt::Debug for Runner<C> {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("Runner")
            .field("project", &self.project.config().name.name)
            .field("schema", &self.schema)
            .field("start", &self.start)
            .field("tip", &self.tip)
            .field("options", &self.options)
            .finish_non_exhaustive()
    }
}

impl<C: Chain> Runner<C> {
    /// Get ready to run `project` into `schema`, reading the chain's current version.
    ///
    /// # Errors
    ///
    /// If the chain can't be read.
    #[allow(
        clippy::too_many_arguments,
        reason = "each is a distinct part of a run"
    )]
    pub async fn new(
        chain: Arc<C>,
        project: Arc<Project>,
        lock: Arc<Lockfile>,
        start: Version,
        schema: impl Into<String>,
        pool: PgPool,
        options: RunOptions,
        health: watch::Sender<Option<Health>>,
    ) -> Result<Self, RunError> {
        let network = project.config().network;
        let tip = crate::pin::retry(|| chain.tip(network)).await?;
        Ok(Self {
            chain,
            project,
            lock,
            start,
            schema: schema.into(),
            pool,
            tip,
            options,
            health,
        })
    }

    /// Run until `stop` turns true, `until`, or a failure. With `replay`, build the
    /// project again from its start beside the served build, and swap it in.
    ///
    /// # Errors
    ///
    /// If the run fails: see [`RunError`].
    pub async fn run(&self, replay: bool, stop: watch::Receiver<bool>) -> Result<(), RunError> {
        if !self.options.log_only {
            self.catch_up_fold().await?;
        }
        let rebuild = replay
            || matches!(
                Store::open(self.pool.clone(), &self.schema, &self.project, &self.lock).await,
                Err(StoreError::Rebuild { .. })
            );
        if rebuild {
            let shadow = shadow_name(&self.schema)?;
            if replay {
                Store::reset(&self.pool, &shadow).await?;
            } else if let Err(StoreError::Rebuild { .. }) =
                Store::open(self.pool.clone(), &shadow, &self.project, &self.lock).await
            {
                // A rebuild under an earlier config: start it over.
                Store::reset(&self.pool, &shadow).await?;
            }
            let target = self.options.until.unwrap_or(self.tip);
            warn!(
                schema = %self.schema,
                %shadow,
                through = target.get(),
                "rebuilding: the current build stays served until the new one catches up"
            );
            let finished = match self.replay_shadow(&shadow, target).await {
                Ok(true) => true,
                Ok(false) => matches!(
                    self.pipeline(&shadow, Some(target), stop.clone()).await?,
                    Outcome::Finished { .. }
                ),
                Err(error) => return Err(error),
            };
            if finished {
                Store::swap(&self.pool, &self.schema, &shadow).await?;
                info!(schema = %self.schema, "swapped in the rebuild");
                // The rebuild's outbox is a different feed. Webhook endpoints move to
                // the end of it rather than delivering the project's history again
                // (ADR 0020), as the change feed's own `reset` does for browsers.
                let newest: Option<(i64, i32)> = sqlx::query_as(
                    "SELECT version, seq FROM nineveh.changes WHERE schema_name = $1
                     ORDER BY version DESC, seq DESC LIMIT 1",
                )
                .bind(&self.schema)
                .fetch_optional(&self.pool)
                .await
                .ok()
                .flatten();
                if let Err(error) = nineveh_store::webhooks::skip_to(
                    &self.pool,
                    &self.schema,
                    newest.map(|n| n.0),
                    newest.map(|n| n.1),
                )
                .await
                {
                    warn!(%error, schema = %self.schema, "couldn't move webhooks to the rebuild");
                }
            } else {
                info!(%shadow, "stopped; running again resumes the rebuild");
                return Ok(());
            }
            if self.options.until.is_some() {
                return Ok(());
            }
        }
        self.pipeline(&self.schema, self.options.until, stop)
            .await?;
        Ok(())
    }

    /// Run a pipeline into `schema` until `until`, `stop`, or a failure.
    /// Fold whatever the log has that the state hasn't (ADR 0023).
    ///
    /// A project that went idle kept its records without folding them, so its state
    /// cursor sits behind its record cursor. Waking it is that gap, folded — a local
    /// pass at thousands of records a second, rather than the hours of re-streaming it
    /// would take to fetch the same history again.
    ///
    /// Silent when there is no gap, which is every ordinary run.
    async fn catch_up_fold(&self) -> Result<(), RunError> {
        let name = self.project.config().name.as_str();
        let Some(logged) = nineveh_store::records::state(&self.pool, name)
            .await?
            .and_then(|s| s.cursor)
        else {
            return Ok(());
        };
        let mut store =
            match Store::open(self.pool.clone(), &self.schema, &self.project, &self.lock).await {
                Ok(store) => store,
                // A config change rebuilds instead, and the rebuild replays the log
                // itself.
                Err(StoreError::Rebuild { .. }) => return Ok(()),
                Err(e) => return Err(e.into()),
            };
        if store.cursor() >= Some(logged) {
            return Ok(());
        }
        let from = match store.cursor() {
            Some(cursor) => cursor.next().ok_or(PipelineError::VersionOverflow)?,
            None => self.start,
        };
        info!(
            schema = %self.schema,
            from = from.get(),
            through = logged.get(),
            "catching the fold up from the record log"
        );
        replay::rebuild(
            &self.pool,
            &mut store,
            &self.project,
            name,
            from,
            logged,
            PipelineConfig::new(self.start).cache_rows,
        )
        .await?;
        // Say so, so a read waiting on the catch-up learns it is over without polling
        // the database for a cursor it can be told about (ADR 0023).
        self.health.send_modify(|health| {
            if let Some(health) = health.as_mut() {
                health.cursor = Some(logged.get().to_string());
            }
        });
        Ok(())
    }

    /// Rebuild the shadow from the project's own record log, if the log can serve it.
    ///
    /// Returns whether it did. A rebuild exists because rows are derived, not because
    /// the chain has to be read again — and reading it again is the expensive part
    /// (ADR 0022): days of a scarce stream for a contract a few months old, charged on
    /// the most ordinary action there is, editing a rule. When the records are already
    /// kept, the history doesn't have to be bought twice.
    ///
    /// Anything the log can't cover falls back to the stream, which is always correct
    /// and only slower.
    async fn replay_shadow(&self, shadow: &str, target: Version) -> Result<bool, RunError> {
        let name = self.project.config().name.as_str();
        let lock_hash = nineveh_store::lock_hash(&self.lock)?;
        if let Err(why) = replay::available(
            &self.pool,
            name,
            &lock_hash,
            &self.project.source_names(),
            self.start,
            target,
        )
        .await?
        {
            info!(
                schema = %self.schema,
                %why,
                "rebuilding from the chain: the record log can't serve this one"
            );
            return Ok(false);
        }
        let mut store = Store::open(self.pool.clone(), shadow, &self.project, &self.lock).await?;
        if store.cursor().is_some_and(|c| c >= target) {
            return Ok(true);
        }
        let from = match store.cursor() {
            Some(cursor) => cursor
                .next()
                .ok_or(RunError::Pipeline(PipelineError::VersionOverflow))?,
            None => self.start,
        };
        info!(
            schema = %self.schema,
            %shadow,
            from = from.get(),
            through = target.get(),
            "rebuilding from the record log, without reading the chain"
        );
        replay::rebuild(
            &self.pool,
            &mut store,
            &self.project,
            name,
            from,
            target,
            PipelineConfig::new(self.start).cache_rows,
        )
        .await?;
        Ok(true)
    }

    async fn pipeline(
        &self,
        schema: &str,
        until: Option<Version>,
        mut stop: watch::Receiver<bool>,
    ) -> Result<Outcome, RunError> {
        let network = self.project.config().network;
        let source = self.chain.source(network, self.start, &self.project);

        let mut config = PipelineConfig::new(self.start);
        config.until = until;
        config.fold = !self.options.log_only;
        if self.options.streams > 1 {
            let mut parallel = Parallel::new(self.options.streams, self.tip);
            parallel.chunk_versions = self.options.chunk;
            config.parallel = Some(parallel);
        }
        let pipeline = Pipeline::new(
            source,
            self.pool.clone(),
            schema,
            Arc::clone(&self.project),
            Arc::clone(&self.lock),
            config,
        );
        info!(
            %schema,
            %network,
            start = self.start.get(),
            chain = self.tip.get(),
            streams = self.options.streams,
            "running"
        );
        let reporter = tokio::spawn(report_progress(
            pipeline.status(),
            self.start,
            self.tip,
            schema.to_owned(),
            self.health.clone(),
        ));
        let shutdown = async move {
            let _ = stop.wait_for(|stopped| *stopped).await;
        };
        let result = pipeline.run(shutdown).await;
        reporter.abort();
        // The reporter is gone: publish where the pipeline ended.
        let last = pipeline.status().borrow().clone();
        self.health.send_modify(|health| {
            let health = health.get_or_insert_with(|| Health {
                schema: schema.to_owned(),
                start_version: Some(self.start.get().to_string()),
                chain_version: Some(self.tip.get().to_string()),
                ..Health::default()
            });
            health.phase = format!("{:?}", last.phase).to_lowercase();
            health.cursor = last.cursor.map(|c| c.get().to_string());
            health.last_error.clone_from(&last.last_error);
        });
        let outcome = result?;
        let cursor = match outcome {
            Outcome::Finished { cursor } | Outcome::Stopped { cursor } => cursor,
            _ => None,
        };
        info!(%schema, cursor = ?cursor.map(Version::get), "stopped");
        Ok(outcome)
    }
}

/// Publish the pipeline's health on every change and at least every second (for lag),
/// and log it every ten seconds.
async fn report_progress(
    mut status: watch::Receiver<Status>,
    start: Version,
    chain: Version,
    schema: String,
    health: watch::Sender<Option<Health>>,
) {
    const SAMPLE: Duration = Duration::from_secs(10);
    let mut last: Option<(Instant, u64)> = None;
    let mut rate = None;
    let mut interval = tokio::time::interval(Duration::from_secs(1));
    loop {
        tokio::select! {
            _ = interval.tick() => {}
            changed = status.changed() => {
                if changed.is_err() {
                    return;
                }
            }
        }
        let current = status.borrow_and_update().clone();
        let now = Instant::now();
        let sampled = last.is_none_or(|(then, _)| now.duration_since(then) >= SAMPLE);
        if sampled {
            rate = last.map(|(then, versions): (Instant, u64)| {
                let seconds = now.duration_since(then).as_secs().max(1);
                current.versions.saturating_sub(versions) / seconds
            });
            last = Some((now, current.versions));
        }
        let lag = current.timestamp_micros.and_then(|micros| {
            let now = SystemTime::now().duration_since(UNIX_EPOCH).ok()?;
            let micros = u128::from(micros);
            u64::try_from(now.as_micros().saturating_sub(micros) / 1_000_000).ok()
        });
        health.send_replace(Some(Health {
            phase: format!("{:?}", current.phase).to_lowercase(),
            schema: schema.clone(),
            cursor: current.cursor.map(|c| c.get().to_string()),
            start_version: Some(start.get().to_string()),
            chain_version: Some(chain.get().to_string()),
            lag_secs: lag,
            versions_per_sec: rate,
            retries: current.retries,
            last_error: current.last_error.clone(),
        }));
        let Some(cursor) = current.cursor else {
            continue;
        };
        if !sampled {
            continue;
        }
        info!(
            %schema,
            cursor = cursor.get(),
            behind = chain.get().saturating_sub(cursor.get()),
            lag_secs = ?lag,
            versions_per_sec = ?rate,
            phase = ?current.phase,
            retries = current.retries,
            "progress"
        );
    }
}
