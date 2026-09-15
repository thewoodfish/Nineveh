//! `nineveh run` and `nineveh replay`: build a project's state and keep it current.
//!
//! When the config or lock changes what's built, `run` rebuilds into a shadow schema
//! while the old build stays served, and swaps it in once it has caught up with the
//! chain (ADR 0016). `replay` does the same with an unchanged config.

use std::net::SocketAddr;
use std::sync::Arc;
use std::time::{Duration, Instant, SystemTime, UNIX_EPOCH};

use anyhow::{Context, Result, bail};
use nineveh_api::Health;
use nineveh_config::Project;
use nineveh_core::Version;
use nineveh_decode::Lockfile;
use nineveh_ingest::{RestClient, StreamConfig};
use nineveh_pipeline::{
    Outcome, Parallel, Pipeline, PipelineConfig, PipelineError, Status, StreamSource, stream_filter,
};
use nineveh_store::{Store, StoreError, shadow_name};
use secrecy::{ExposeSecret, SecretString};
use sqlx::PgPool;
use sqlx::postgres::PgPoolOptions;
use tokio::sync::watch;
use tracing::{info, warn};

use crate::project::{Paths, load};

/// How to run a project.
#[derive(Debug, Clone)]
pub(crate) struct RunOptions {
    pub(crate) database_url: SecretString,
    /// The Postgres schema to build into; the project's name by default.
    pub(crate) schema: Option<String>,
    /// Stop once this version is committed.
    pub(crate) until: Option<u64>,
    /// Streams for backfill: ranges up to the chain's current version are read this
    /// many at a time.
    pub(crate) streams: usize,
    /// Versions per backfill range.
    pub(crate) chunk: u64,
    pub(crate) api_key: Option<SecretString>,
    /// Serve the API and change feed here while running.
    pub(crate) serve: Option<SocketAddr>,
}

pub(crate) async fn run(paths: &Paths, options: &RunOptions) -> Result<()> {
    Runner::new(paths, options).await?.run(false).await
}

/// `nineveh replay`: build the project again from its start, beside the served build,
/// and swap it in.
pub(crate) async fn replay(paths: &Paths, options: &RunOptions, confirmed: bool) -> Result<()> {
    if !confirmed {
        let loaded = load(paths)?;
        let schema = options
            .schema
            .clone()
            .unwrap_or_else(|| loaded.project.config().name.name.clone());
        bail!(
            "replay rebuilds `{schema}` from version {} and then replaces its tables, cursor \
             and change feed; pass --yes to do it",
            loaded.start
        );
    }
    Runner::new(paths, options).await?.run(true).await
}

/// Everything a run needs, loaded once.
struct Runner {
    project: Arc<Project>,
    lock: Arc<Lockfile>,
    start: Version,
    schema: String,
    pool: PgPool,
    /// The chain's version when the run began: where backfill ends and a rebuild
    /// swaps in.
    tip: Version,
    options: RunOptions,
    /// Set by Ctrl-C.
    stop: watch::Receiver<bool>,
    /// The pipeline's health, for the API when serving.
    health: watch::Sender<Option<Health>>,
}

impl Runner {
    async fn new(paths: &Paths, options: &RunOptions) -> Result<Self> {
        let loaded = load(paths)?;
        let network = loaded.project.config().network;
        let schema = options
            .schema
            .clone()
            .unwrap_or_else(|| loaded.project.config().name.name.clone());

        let rest = RestClient::hosted(network, options.api_key.as_ref())?;
        let ledger = rest
            .ledger()
            .await
            .context("reading the ledger's version")?;
        if let Some(expected) = network.chain_id()
            && ledger.chain_id != expected
        {
            bail!(
                "the {network} REST API reports chain {}, not {expected}",
                ledger.chain_id
            );
        }
        let pool = PgPoolOptions::new()
            .max_connections(4)
            .connect(options.database_url.expose_secret())
            .await
            .context("connecting to Postgres")?;

        let (health, health_receiver) = watch::channel(None);
        if let Some(listen) = options.serve {
            crate::serve::start(
                pool.clone(),
                &schema,
                &loaded.project,
                health_receiver,
                listen,
            )
            .await?;
        }

        let (stop, stopped) = watch::channel(false);
        tokio::spawn(async move {
            if tokio::signal::ctrl_c().await.is_ok() {
                info!("stopping after the current commit");
                let _ = stop.send(true);
            }
        });

        Ok(Self {
            project: Arc::new(loaded.project),
            lock: Arc::new(loaded.lock),
            start: loaded.start,
            schema,
            pool,
            tip: ledger.ledger_version,
            options: options.clone(),
            stop: stopped,
            health,
        })
    }

    async fn run(&self, replay: bool) -> Result<()> {
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
            let target = self.options.until.map_or(self.tip, Version::new);
            warn!(
                schema = %self.schema,
                %shadow,
                through = target.get(),
                "rebuilding: the current build stays served until the new one catches up"
            );
            if let Outcome::Finished { .. } = self.pipeline(&shadow, Some(target)).await? {
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
        let until = self.options.until.map(Version::new);
        self.pipeline(&self.schema, until).await?;
        Ok(())
    }

    /// Run a pipeline into `schema` until `until`, Ctrl-C, or a failure.
    async fn pipeline(&self, schema: &str, until: Option<Version>) -> Result<Outcome> {
        let network = self.project.config().network;
        let mut stream = StreamConfig::hosted(network, self.start);
        stream.api_key.clone_from(&self.options.api_key);
        stream.filter = stream_filter(&self.project);
        let filtered = stream.filter.is_some();

        let mut config = PipelineConfig::new(self.start);
        config.until = until;
        if self.options.streams > 1 {
            let mut parallel = Parallel::new(self.options.streams, self.tip);
            parallel.chunk_versions = self.options.chunk;
            config.parallel = Some(parallel);
        }
        let pipeline = Pipeline::new(
            StreamSource::new(stream),
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
            filtered,
            streams = self.options.streams,
            "running"
        );
        let reporter = tokio::spawn(report_progress(
            pipeline.status(),
            self.tip,
            schema.to_owned(),
            self.health.clone(),
        ));
        let mut stop = self.stop.clone();
        let shutdown = async move {
            let _ = stop.wait_for(|stopped| *stopped).await;
        };
        let result = pipeline.run(shutdown).await;
        reporter.abort();
        // The reporter is gone: publish where the pipeline ended.
        let last = pipeline.status().borrow().clone();
        self.health.send_modify(|health| {
            if let Some(health) = health {
                health.phase = format!("{:?}", last.phase).to_lowercase();
                health.cursor = last.cursor.map(|c| c.get().to_string());
                health.last_error.clone_from(&last.last_error);
            }
        });

        match result {
            Ok(outcome) => {
                let cursor = match outcome {
                    Outcome::Finished { cursor } | Outcome::Stopped { cursor } => cursor,
                    _ => None,
                };
                info!(%schema, cursor = ?cursor.map(Version::get), "stopped");
                Ok(outcome)
            }
            Err(error) => {
                if let Some(version) = error.halted_at() {
                    bail!("halted at version {version}: {error}");
                }
                if let PipelineError::Store(StoreError::Rebuild { .. }) = error {
                    bail!("schema `{schema}` changed while running; run again to rebuild it");
                }
                Err(error.into())
            }
        }
    }
}

/// Publish the pipeline's health every second, and log it every ten.
async fn report_progress(
    mut status: watch::Receiver<Status>,
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
