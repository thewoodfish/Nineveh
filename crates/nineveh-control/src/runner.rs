//! Running a project: build its state and keep it current, rebuilding into a shadow
//! schema when its config changes what's built (ADR 0016). `nineveh run` and the
//! control plane both run projects through [`Runner`].

use std::sync::Arc;
use std::time::{Duration, Instant, SystemTime, UNIX_EPOCH};

use nineveh_api::Health;
use nineveh_config::Project;
use nineveh_core::Version;
use nineveh_decode::Lockfile;
use nineveh_pipeline::{Outcome, Parallel, Pipeline, PipelineConfig, PipelineError, Status};
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
}

impl Default for RunOptions {
    fn default() -> Self {
        Self {
            until: None,
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
            if let Outcome::Finished { .. } =
                self.pipeline(&shadow, Some(target), stop.clone()).await?
            {
                Store::swap(&self.pool, &self.schema, &shadow).await?;
                info!(schema = %self.schema, "swapped in the rebuild");
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

/// Publish the pipeline's health every second, and log it every ten.
async fn report_progress(
    mut status: watch::Receiver<Status>,
    start: Version,
    chain: Version,
    schema: String,
    health: watch::Sender<Option<Health>>,
) {
    let mut last: Option<(Instant, u64)> = None;
    let mut rate = None;
    let mut interval = tokio::time::interval(Duration::from_secs(1));
    let mut tick: u64 = 0;
    loop {
        interval.tick().await;
        tick = tick.wrapping_add(1);
        let current = status.borrow_and_update().clone();
        let now = Instant::now();
        if tick.is_multiple_of(10) {
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
        if !tick.is_multiple_of(10) {
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
