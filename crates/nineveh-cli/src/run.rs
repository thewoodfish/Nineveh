//! `nineveh run` and `nineveh replay`: build a project's state and keep it current.
//!
//! When the config or lock changes what's built, `run` rebuilds into a shadow schema
//! while the old build stays served, and swaps it in once it has caught up with the
//! chain (ADR 0016). `replay` does the same with an unchanged config. Both run through
//! `nineveh_control::Runner`, as the control plane does.

use std::net::SocketAddr;
use std::sync::Arc;

use anyhow::{Context, Result, bail};
use nineveh_control::{Hosted, RunOptions as Run, Runner};
use nineveh_core::Version;
use secrecy::{ExposeSecret, SecretString};
use sqlx::postgres::PgPoolOptions;
use tokio::sync::watch;
use tracing::info;

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
    start(paths, options, false).await
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
    start(paths, options, true).await
}

async fn start(paths: &Paths, options: &RunOptions, replay: bool) -> Result<()> {
    let loaded = load(paths)?;
    let schema = options
        .schema
        .clone()
        .unwrap_or_else(|| loaded.project.config().name.name.clone());
    let pool = PgPoolOptions::new()
        .max_connections(4)
        .connect(options.database_url.expose_secret())
        .await
        .context("connecting to Postgres")?;

    let (health, health_receiver) = watch::channel(None);
    let project = Arc::new(loaded.project);
    let runner = Runner::new(
        Arc::new(Hosted::new(options.api_key.clone())),
        Arc::clone(&project),
        Arc::new(loaded.lock),
        loaded.start,
        schema.clone(),
        pool.clone(),
        Run {
            until: options.until.map(Version::new),
            // `nineveh run` is somebody sitting at a terminal waiting for the tables:
            // it always folds.
            log_only: false,
            streams: options.streams,
            chunk: options.chunk,
        },
        health,
    )
    .await
    .context("reading the chain's current version")?;
    if let Some(listen) = options.serve {
        crate::serve::start(pool, &schema, &project, health_receiver, listen).await?;
    }

    let (stop, stopped) = watch::channel(false);
    tokio::spawn(async move {
        if crate::shutdown::requested().await.is_ok() {
            info!("stopping after the current commit");
            let _ = stop.send(true);
        }
    });
    runner.run(replay, stopped).await?;
    Ok(())
}
