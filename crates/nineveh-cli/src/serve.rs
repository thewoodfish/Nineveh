//! Serving a project: the state API and the change feed on one listener.

use std::net::SocketAddr;
use std::sync::Arc;

use anyhow::{Context, Result};
use nineveh_api::{Api, Health};
use nineveh_config::Project;
use sqlx::PgPool;
use tokio::net::TcpListener;
use tokio::sync::watch;
use tower_http::cors::CorsLayer;
use tracing::{error, info};

use crate::project::{Paths, load};

/// Where to serve by default: local only, since there's no auth yet.
pub(crate) const DEFAULT_LISTEN: &str = "127.0.0.1:4000";

/// This process's webhook senders, kept for as long as it serves.
static SENDERS: std::sync::OnceLock<nineveh_control::Deliveries> = std::sync::OnceLock::new();

/// Start serving `project`'s state from `schema` in the background.
pub(crate) async fn start(
    pool: PgPool,
    schema: &str,
    project: &Project,
    health: watch::Receiver<Option<Health>>,
    listen: SocketAddr,
) -> Result<()> {
    // Nineveh's own tables, so a database nothing has built into yet answers "no
    // build" rather than failing.
    nineveh_store::migrate(&pool)
        .await
        .context("creating Nineveh's tables")?;
    let api = Arc::new(Api::new(pool.clone(), schema, project, health));
    let feed = nineveh_realtime::Feed::start(pool.clone(), schema)
        .await
        .context("listening for commits")?;
    // Webhook senders run as long as the server does: this process serves one project
    // until it's killed, so they're held for its lifetime (ADR 0020).
    let hooks = &project.config().webhooks;
    if !hooks.is_empty() {
        let _ = SENDERS.set(nineveh_control::Deliveries::start(
            &pool,
            schema,
            hooks,
            Some(&feed.wake()),
        ));
    }
    // Studio runs on its own origin. The API is read-only and, by default, local.
    let app = nineveh_api::router(api)
        .merge(nineveh_realtime::router(feed))
        .layer(CorsLayer::permissive());
    let listener = TcpListener::bind(listen)
        .await
        .with_context(|| format!("listening on {listen}"))?;
    info!("serving http://{listen}/v1/status");
    tokio::spawn(async move {
        if let Err(e) = axum::serve(listener, app).await {
            error!(error = %e, "the server stopped");
        }
    });
    Ok(())
}

/// `nineveh serve`: the API and feed without a pipeline, until Ctrl-C.
pub(crate) async fn serve(
    paths: &Paths,
    database_url: &str,
    schema: Option<String>,
    listen: SocketAddr,
) -> Result<()> {
    let loaded = load(paths)?;
    let schema = schema.unwrap_or_else(|| loaded.project.config().name.name.clone());
    let pool = sqlx::postgres::PgPoolOptions::new()
        .max_connections(8)
        .connect(database_url)
        .await
        .context("connecting to Postgres")?;
    let (_health, receiver) = watch::channel(None);
    start(pool, &schema, &loaded.project, receiver, listen).await?;
    crate::shutdown::requested().await?;
    Ok(())
}
