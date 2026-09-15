//! `nineveh run` and `nineveh replay`: build a project's state and keep it current.

use std::sync::Arc;
use std::time::{Duration, Instant, SystemTime, UNIX_EPOCH};

use anyhow::{Context, Result, bail};
use nineveh_core::Version;
use nineveh_ingest::{RestClient, StreamConfig};
use nineveh_pipeline::{
    Outcome, Parallel, Pipeline, PipelineConfig, PipelineError, Status, StreamSource, stream_filter,
};
use nineveh_store::{Store, StoreError};
use secrecy::SecretString;
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
}

pub(crate) async fn run(paths: &Paths, options: &RunOptions) -> Result<()> {
    let loaded = load(paths)?;
    let network = loaded.project.config().network;
    let schema = options
        .schema
        .clone()
        .unwrap_or_else(|| loaded.project.config().name.name.clone());

    // The chain's current version bounds parallel backfill: ranges past it would
    // only wait for the chain.
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
        .connect(secrecy::ExposeSecret::expose_secret(&options.database_url))
        .await
        .context("connecting to Postgres")?;

    let mut stream = StreamConfig::hosted(network, loaded.start);
    stream.api_key.clone_from(&options.api_key);
    stream.filter = stream_filter(&loaded.project);
    let filtered = stream.filter.is_some();

    let mut config = PipelineConfig::new(loaded.start);
    config.until = options.until.map(Version::new);
    if options.streams > 1 {
        let mut parallel = Parallel::new(options.streams, ledger.ledger_version);
        parallel.chunk_versions = options.chunk;
        config.parallel = Some(parallel);
    }

    let pipeline = Pipeline::new(
        StreamSource::new(stream),
        pool,
        &schema,
        Arc::new(loaded.project),
        Arc::new(loaded.lock),
        config,
    );
    info!(
        %schema,
        %network,
        start = loaded.start.get(),
        chain = ledger.ledger_version.get(),
        filtered,
        streams = options.streams,
        "running"
    );
    let reporter = tokio::spawn(report_progress(pipeline.status(), ledger.ledger_version));
    let shutdown = async {
        if tokio::signal::ctrl_c().await.is_ok() {
            info!("stopping after the current commit");
        }
    };
    let result = pipeline.run(shutdown).await;
    reporter.abort();

    match result {
        Ok(Outcome::Finished { cursor } | Outcome::Stopped { cursor }) => {
            info!(%schema, cursor = ?cursor.map(Version::get), "stopped");
            Ok(())
        }
        Ok(other) => {
            info!(?other, "stopped");
            Ok(())
        }
        Err(PipelineError::Store(StoreError::Rebuild { .. })) => bail!(
            "schema `{schema}` was built from a different config, lock or Nineveh version; \
             run `nineveh replay --yes` to rebuild it"
        ),
        Err(error) => {
            if let Some(version) = error.halted_at() {
                bail!("halted at version {version}: {error}");
            }
            Err(error.into())
        }
    }
}

/// `nineveh replay`: drop the project's state and build it again from its start.
pub(crate) async fn replay(paths: &Paths, options: &RunOptions, confirmed: bool) -> Result<()> {
    let loaded = load(paths)?;
    let schema = options
        .schema
        .clone()
        .unwrap_or_else(|| loaded.project.config().name.name.clone());
    if !confirmed {
        bail!(
            "replay drops schema `{schema}`, its cursor and its change feed, then rebuilds \
             from version {}; pass --yes to do it",
            loaded.start
        );
    }
    let pool = PgPoolOptions::new()
        .max_connections(1)
        .connect(secrecy::ExposeSecret::expose_secret(&options.database_url))
        .await
        .context("connecting to Postgres")?;
    Store::reset(&pool, &schema).await?;
    warn!(%schema, "dropped; rebuilding");
    run(paths, options).await
}

/// Log the cursor, lag and rate every few seconds while the pipeline runs.
async fn report_progress(mut status: watch::Receiver<Status>, chain: Version) {
    let mut last: Option<(Instant, u64)> = None;
    let mut interval = tokio::time::interval(Duration::from_secs(10));
    interval.tick().await;
    loop {
        interval.tick().await;
        let current = status.borrow_and_update().clone();
        let Some(cursor) = current.cursor else {
            continue;
        };
        let now = Instant::now();
        let rate = last.map(|(then, versions)| {
            let seconds = now.duration_since(then).as_secs().max(1);
            current.versions.saturating_sub(versions) / seconds
        });
        last = Some((now, current.versions));
        let lag = current.timestamp_micros.and_then(|micros| {
            let now = SystemTime::now().duration_since(UNIX_EPOCH).ok()?;
            let micros = u128::from(micros);
            u64::try_from(now.as_micros().saturating_sub(micros) / 1_000_000).ok()
        });
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
