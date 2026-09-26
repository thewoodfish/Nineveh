//! What the control-plane tests share: the vault contract on a scripted chain, a
//! test database, and requests to the router.

#![allow(
    dead_code,
    unreachable_pub,
    clippy::unwrap_used,
    clippy::panic,
    clippy::print_stderr,
    reason = "test helpers: each test binary uses some of them"
)]

use std::future::{Future, pending, ready};
use std::sync::Arc;
use std::sync::atomic::{AtomicUsize, Ordering};

use axum::Router;
use axum::body::{Body, to_bytes};
use axum::http::{HeaderMap, Method, Request, StatusCode};
use nineveh_config::Project;
use nineveh_control::{Chain, ChainError, ModuleInfo, RunOptions};
use nineveh_core::{Address, ChainId, Network, Version};
use nineveh_decode::ModuleAbi;
use nineveh_ingest::{Batch, IngestError};
use nineveh_pipeline::{BatchStream, Source};
use nineveh_proto::transaction::Transaction;
use nineveh_testkit::vault::{self, Model, Op};
use serde_json::Value;
use sqlx::PgPool;
use sqlx::postgres::{PgConnectOptions, PgPoolOptions};
use tower::ServiceExt as _;

/// The vault contract on a chain that holds exactly `transactions`.
pub struct Scripted {
    pub transactions: Arc<Vec<Transaction>>,
    /// Streams opened over this chain. The count is what ADR 0021 is about: a plane
    /// serving many projects must not open one per project.
    pub opens: Arc<AtomicUsize>,
}

impl Scripted {
    pub fn opens(&self) -> usize {
        self.opens.load(Ordering::Relaxed)
    }
}

impl Chain for Scripted {
    type Source = Replay;

    /// A scripted chain answers for whatever network a test asks about.
    fn networks(&self) -> Vec<Network> {
        Network::ALL.to_vec()
    }

    fn tip(&self, _: Network) -> impl Future<Output = Result<Version, ChainError>> + Send {
        ready(Ok(Version::new(
            self.transactions.last().map_or(0, |t| t.version),
        )))
    }

    fn modules(
        &self,
        _: Network,
        address: Address,
    ) -> impl Future<Output = Result<Vec<ModuleAbi>, ChainError>> + Send {
        ready(Ok(vault::modules()
            .into_iter()
            .filter(|m| m.address == address)
            .collect()))
    }

    fn module(
        &self,
        _: Network,
        address: Address,
        name: &str,
    ) -> impl Future<Output = Result<Option<ModuleInfo>, ChainError>> + Send {
        ready(Ok(vault::modules()
            .into_iter()
            .find(|m| m.address == address && m.name.as_str() == name)
            .map(|abi| ModuleInfo {
                abi,
                groups: Vec::new(),
            })))
    }

    fn first_transaction(
        &self,
        _: Network,
        _: Address,
    ) -> impl Future<Output = Result<Option<Version>, ChainError>> + Send {
        ready(Ok(self
            .transactions
            .first()
            .map(|t| Version::new(t.version))))
    }

    fn source(&self, _: Network, _: Version, _: &Project) -> Replay {
        Replay {
            transactions: Arc::clone(&self.transactions),
            opens: Arc::clone(&self.opens),
        }
    }

    fn network_source(&self, _: Network, _: Version) -> Replay {
        Replay {
            transactions: Arc::clone(&self.transactions),
            opens: Arc::clone(&self.opens),
        }
    }
}

pub struct Replay {
    transactions: Arc<Vec<Transaction>>,
    opens: Arc<AtomicUsize>,
}

impl Source for Replay {
    type Stream = ReplayStream;

    fn open(
        &self,
        from: Version,
        until: Option<Version>,
    ) -> impl Future<Output = Result<ReplayStream, IngestError>> + Send {
        let last = until.map_or(u64::MAX, Version::get);
        let transactions: Vec<Transaction> = self
            .transactions
            .iter()
            .filter(|t| t.version >= from.get() && t.version <= last)
            .cloned()
            .collect();
        let end = transactions.last().map_or(from.get(), |t| t.version);
        self.opens.fetch_add(1, Ordering::Relaxed);
        ready(Ok(ReplayStream {
            batch: Some(Batch {
                chain_id: ChainId::try_from(2u64).unwrap(),
                transactions,
                processed_range: Some(from..=Version::new(end)),
            }),
            // A bounded range ends when it has covered its versions, as the real
            // stream does; only the tail stays open. A catch-up that never ended
            // would leave a project stuck between its own stream and the shared one.
            bounded: until.is_some(),
        }))
    }
}

/// Everything in one batch, then an open stream with nothing more: the chain's tip.
pub struct ReplayStream {
    batch: Option<Batch>,
    bounded: bool,
}

impl BatchStream for ReplayStream {
    async fn next(&mut self) -> Result<Option<Arc<Batch>>, IngestError> {
        match self.batch.take() {
            Some(batch) => Ok(Some(Arc::new(batch))),
            None if self.bounded => Ok(None),
            None => pending().await,
        }
    }
}

pub async fn pool(test: &str) -> Option<PgPool> {
    let Ok(url) = std::env::var("NINEVEH_TEST_DATABASE_URL") else {
        assert!(
            std::env::var_os("CI").is_none(),
            "CI must set NINEVEH_TEST_DATABASE_URL"
        );
        eprintln!("skipping: set NINEVEH_TEST_DATABASE_URL to run the control plane's tests");
        return None;
    };
    // A database of this test's own. A control plane runs every project in its
    // database's registry (ADR 0017), which is right in production and wrong here:
    // two planes sharing a database would each adopt the other's projects and fight
    // over the same schemas. It also keeps tests off a developer's own projects.
    let options: PgConnectOptions = url.parse().unwrap();
    let database = format!("nineveh_test_{test}");
    let server = PgPoolOptions::new()
        .max_connections(1)
        .connect_with(options.clone())
        .await
        .unwrap();
    for statement in [
        format!("DROP DATABASE IF EXISTS \"{database}\" WITH (FORCE)"),
        format!("CREATE DATABASE \"{database}\""),
    ] {
        sqlx::query(&statement).execute(&server).await.unwrap();
    }
    server.close().await;
    let pool = PgPoolOptions::new()
        .max_connections(8)
        .connect_with(options.database(&database))
        .await
        .unwrap();
    nineveh_store::migrate(&pool).await.unwrap();
    Some(pool)
}

/// Print what the plane and its pipelines do, so a test that fails in CI says why
/// rather than only what it saw. `RUST_LOG` overrides the default.
pub fn logs() {
    let filter = tracing_subscriber::EnvFilter::try_from_default_env().unwrap_or_else(|_| {
        tracing_subscriber::EnvFilter::new(
            "nineveh_control=debug,nineveh_pipeline=debug,nineveh_store=debug",
        )
    });
    let _ = tracing_subscriber::fmt()
        .with_env_filter(filter)
        .with_test_writer()
        .try_init();
}

/// A request to `app` with an optional bearer token, and its JSON answer.
pub async fn call_as(
    app: &Router,
    token: Option<&str>,
    method: Method,
    uri: &str,
    body: Option<Value>,
) -> (StatusCode, HeaderMap, Value) {
    let mut request = Request::builder().method(method).uri(uri);
    if body.is_some() {
        request = request.header("content-type", "application/json");
    }
    if let Some(token) = token {
        request = request.header("authorization", format!("Bearer {token}"));
    }
    let body = body.map_or_else(Body::empty, |b| Body::from(b.to_string()));
    let response = app
        .clone()
        .oneshot(request.body(body).unwrap())
        .await
        .unwrap();
    let status = response.status();
    let headers = response.headers().clone();
    let bytes = to_bytes(response.into_body(), 1 << 20).await.unwrap();
    let json = serde_json::from_slice(&bytes).unwrap_or(Value::Null);
    (status, headers, json)
}

/// A request to `app`, and its JSON answer.
pub async fn call(
    app: &Router,
    method: Method,
    uri: &str,
    body: Option<Value>,
) -> (StatusCode, Value) {
    let (status, _, json) = call_as(app, None, method, uri, body).await;
    (status, json)
}

/// The vault's transactions for `ops`, on a scripted chain.
pub fn chain(ops: &[Op]) -> (Arc<Scripted>, Model) {
    let (transactions, model) = vault::transactions(ops);
    (
        Arc::new(Scripted {
            transactions: Arc::new(transactions),
            opens: Arc::new(AtomicUsize::new(0)),
        }),
        model,
    )
}

/// One stream per project: the scripted chain has no parallel backfill to test.
pub fn options() -> RunOptions {
    RunOptions {
        streams: 1,
        ..RunOptions::default()
    }
}
