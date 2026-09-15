//! `nineveh up`: the control plane Studio drives (ADR 0017). Every project registered
//! in the database is served, and run if it should be, until Ctrl-C.

use std::net::SocketAddr;
use std::sync::Arc;

use anyhow::{Context, Result};
use nineveh_control::{ControlPlane, Hosted, RunOptions};
use secrecy::{ExposeSecret, SecretString};
use sqlx::postgres::PgPoolOptions;
use tokio::net::TcpListener;
use tower_http::cors::CorsLayer;
use tracing::{error, info};

pub(crate) struct UpOptions {
    pub(crate) database_url: SecretString,
    pub(crate) listen: SocketAddr,
    pub(crate) api_key: Option<SecretString>,
    pub(crate) streams: usize,
    pub(crate) chunk: u64,
}

pub(crate) async fn up(options: UpOptions) -> Result<()> {
    let pool = PgPoolOptions::new()
        .max_connections(16)
        .connect(options.database_url.expose_secret())
        .await
        .context("connecting to Postgres")?;
    let plane = ControlPlane::start(
        Arc::new(Hosted::new(options.api_key)),
        pool,
        RunOptions {
            until: None,
            streams: options.streams,
            chunk: options.chunk,
        },
    )
    .await
    .context("loading the registered projects")?;
    // Studio runs on its own origin. There's no auth yet, so the default is local.
    let app = nineveh_control::router(Arc::clone(&plane)).layer(CorsLayer::permissive());
    let listener = TcpListener::bind(options.listen)
        .await
        .with_context(|| format!("listening on {}", options.listen))?;
    info!(
        "control plane on http://{}/control/v1/projects; open Studio to create a project",
        options.listen
    );
    // Not a graceful shutdown: change-feed streams never end by themselves.
    tokio::spawn(async move {
        if let Err(e) = axum::serve(listener, app).await {
            error!(error = %e, "the server stopped");
        }
    });
    tokio::signal::ctrl_c()
        .await
        .context("waiting for Ctrl-C")?;
    info!("stopping every project after its current commit");
    plane.shutdown().await;
    Ok(())
}
