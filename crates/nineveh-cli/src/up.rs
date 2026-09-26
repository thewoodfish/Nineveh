//! `nineveh up`: the control plane Studio drives (ADR 0017). Every project registered
//! in the database is served, and run if it should be, until Ctrl-C.
//!
//! With a GitHub OAuth app it's hosted: people sign in, and projects are reached with
//! API keys (ADR 0018). Without one it's local: no sign-in, on loopback only.

use std::net::SocketAddr;
use std::sync::Arc;

use anyhow::{Context, Result, bail};
use nineveh_control::{Access, Chain as _, ControlPlane, GitHub, Hosted, RunOptions};
use nineveh_core::Network;
use secrecy::{ExposeSecret, SecretString};
use sqlx::postgres::PgPoolOptions;
use tokio::net::TcpListener;
use tower_http::cors::CorsLayer;
use tracing::{error, info, warn};

pub(crate) struct UpOptions {
    pub(crate) database_url: SecretString,
    pub(crate) listen: SocketAddr,
    pub(crate) api_key: Option<SecretString>,
    /// Keys for particular networks, over `api_key`.
    pub(crate) network_keys: Vec<(Network, SecretString)>,
    pub(crate) streams: usize,
    pub(crate) chunk: u64,
    /// A GitHub OAuth app, for hosted mode.
    pub(crate) github: Option<(String, SecretString)>,
    /// Where this control plane is reached from browsers: GitHub's callback goes here.
    pub(crate) public_url: Option<String>,
    /// Where Studio is: sign-ins end there.
    pub(crate) studio_url: String,
}

pub(crate) async fn up(options: UpOptions) -> Result<()> {
    let access = if let Some((client_id, client_secret)) = options.github {
        let public_url = options
            .public_url
            .clone()
            .unwrap_or_else(|| format!("http://{}", options.listen));
        let github = GitHub::new(client_id, client_secret, &public_url)
            .context("setting up GitHub sign-in")?;
        info!(%public_url, studio = %options.studio_url, "hosted: sign-in with GitHub");
        Access::Hosted {
            provider: Arc::new(github),
            studio_url: options.studio_url.trim_end_matches('/').to_owned(),
        }
    } else {
        if !options.listen.ip().is_loopback() {
            bail!(
                "without sign-in, the control plane only listens on loopback: anyone who \
                 reaches it can read and delete every project. Configure GitHub sign-in \
                 (--github-client-id and NINEVEH_GITHUB_CLIENT_SECRET) to listen on {}",
                options.listen
            );
        }
        Access::Local
    };
    let pool = PgPoolOptions::new()
        .max_connections(16)
        .connect(options.database_url.expose_secret())
        .await
        .context("connecting to Postgres")?;
    let chain = options
        .network_keys
        .into_iter()
        .fold(Hosted::new(options.api_key), |chain, (network, key)| {
            chain.with_key(network, key)
        });
    // Said at boot rather than left for whoever picks the wrong network first. A Geomi
    // key is per network, so a plane with a key for one of them can only stream that
    // one, and the symptom otherwise is an `Unauthenticated` from the Transaction
    // Stream long after the project was created, naming nothing that would help.
    let served = chain.networks();
    if served.is_empty() {
        warn!(
            "no Geomi key: set APTOS_API_KEY, or APTOS_API_KEY_TESTNET and friends. \
             No project will be able to stream"
        );
    } else {
        info!(
            networks = served
                .iter()
                .copied()
                .map(Network::as_str)
                .collect::<Vec<_>>()
                .join(", "),
            "ready to stream"
        );
        for network in Network::ALL {
            if !served.contains(&network) && nineveh_control::tier::FREE.allows(network.as_str()) {
                warn!(
                    "no key for {network}, which the free tier allows: set \
                     APTOS_API_KEY_{} to offer it in Studio",
                    network.as_str().to_uppercase()
                );
            }
        }
    }
    let plane = ControlPlane::start(
        Arc::new(chain),
        pool,
        RunOptions {
            until: None,
            // The default for a project the plane starts. Its sweep decides which ones
            // stop folding, per project (ADR 0023).
            log_only: false,
            streams: options.streams,
            chunk: options.chunk,
        },
    )
    .await
    .context("loading the registered projects")?;
    // Studio runs on its own origin. Credentials travel as bearer tokens, never
    // cookies, so allowing any origin exposes nothing a caller doesn't already hold.
    let app = nineveh_control::router(Arc::clone(&plane), access).layer(CorsLayer::permissive());
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
    crate::shutdown::requested().await?;
    info!("stopping every project after its current commit");
    plane.shutdown().await;
    Ok(())
}
