//! The control plane end to end, over HTTP and a real Postgres, with a scripted chain:
//! inspect the vault contract, scaffold a project from picks, create it, watch its
//! tables fill, change its config (a rebuild), stop and start it, restart the control
//! plane, and delete the project.
//!
//! Needs `NINEVEH_TEST_DATABASE_URL`; skips without it, except in CI.

#![allow(
    clippy::unwrap_used,
    clippy::panic,
    clippy::print_stderr,
    reason = "test-only crate: helpers panic on unexpected results and note skips"
)]

use std::future::{Future, pending, ready};
use std::sync::Arc;
use std::time::Duration;

use axum::Router;
use axum::body::{Body, to_bytes};
use axum::http::{Method, Request, StatusCode};
use nineveh_config::Project;
use nineveh_control::{Chain, ChainError, ControlPlane, ModuleInfo, RunOptions, router};
use nineveh_core::{Address, ChainId, Network, Version};
use nineveh_decode::ModuleAbi;
use nineveh_ingest::{Batch, IngestError};
use nineveh_pipeline::{BatchStream, Source};
use nineveh_proto::transaction::Transaction;
use nineveh_testkit::vault::{self, Op};
use serde_json::{Value, json};
use sqlx::PgPool;
use sqlx::postgres::PgPoolOptions;
use tower::ServiceExt as _;

/// The vault contract on a chain that holds exactly `transactions`.
struct Scripted {
    transactions: Arc<Vec<Transaction>>,
}

impl Chain for Scripted {
    type Source = Replay;

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
        }
    }
}

struct Replay {
    transactions: Arc<Vec<Transaction>>,
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
        ready(Ok(ReplayStream {
            batch: Some(Batch {
                chain_id: ChainId::try_from(2u64).unwrap(),
                transactions,
                processed_range: Some(from..=Version::new(end)),
            }),
        }))
    }
}

/// Everything in one batch, then an open stream with nothing more: the chain's tip.
struct ReplayStream {
    batch: Option<Batch>,
}

impl BatchStream for ReplayStream {
    async fn next(&mut self) -> Result<Option<Batch>, IngestError> {
        match self.batch.take() {
            Some(batch) => Ok(Some(batch)),
            None => pending().await,
        }
    }
}

async fn pool() -> Option<PgPool> {
    let Ok(url) = std::env::var("NINEVEH_TEST_DATABASE_URL") else {
        assert!(
            std::env::var_os("CI").is_none(),
            "CI must set NINEVEH_TEST_DATABASE_URL"
        );
        eprintln!("skipping: set NINEVEH_TEST_DATABASE_URL to run the control plane's tests");
        return None;
    };
    Some(
        PgPoolOptions::new()
            .max_connections(8)
            .connect(&url)
            .await
            .unwrap(),
    )
}

async fn call(app: &Router, method: Method, uri: &str, body: Option<Value>) -> (StatusCode, Value) {
    let mut request = Request::builder().method(method).uri(uri);
    if body.is_some() {
        request = request.header("content-type", "application/json");
    }
    let body = body.map_or_else(Body::empty, |b| Body::from(b.to_string()));
    let response = app
        .clone()
        .oneshot(request.body(body).unwrap())
        .await
        .unwrap();
    let status = response.status();
    let bytes = to_bytes(response.into_body(), 1 << 20).await.unwrap();
    let json = serde_json::from_slice(&bytes).unwrap_or(Value::Null);
    (status, json)
}

/// Poll a project's table until it has `rows` rows.
async fn wait_for_rows(app: &Router, name: &str, table: &str, rows: i64) {
    let uri = format!("/projects/{name}/v1/tables/{table}?count=exact");
    let mut last = (StatusCode::OK, Value::Null);
    for _ in 0..100 {
        let (status, body) = call(app, Method::GET, &uri, None).await;
        if status == StatusCode::OK && body["count"] == json!(rows) {
            return;
        }
        last = (status, body);
        tokio::time::sleep(Duration::from_millis(100)).await;
    }
    let project = state(app, name).await;
    panic!(
        "`{table}` never reached {rows} rows; last answer: {} {}; project: {project}",
        last.0, last.1
    );
}

/// Poll a project until its state is `expected`.
async fn wait_for_state(app: &Router, name: &str, expected: &str) {
    let mut last = Value::Null;
    for _ in 0..100 {
        last = state(app, name).await;
        if last["state"] == json!(expected) {
            return;
        }
        tokio::time::sleep(Duration::from_millis(100)).await;
    }
    panic!("`{name}` never became {expected}: {last}");
}

async fn state(app: &Router, name: &str) -> Value {
    let (status, body) = call(
        app,
        Method::GET,
        &format!("/control/v1/projects/{name}"),
        None,
    )
    .await;
    assert_eq!(status, StatusCode::OK, "{body}");
    body
}

#[tokio::test(flavor = "multi_thread")]
#[allow(clippy::too_many_lines, reason = "one project's life, step by step")]
async fn inspects_creates_runs_changes_and_deletes_a_project() {
    let Some(pool) = pool().await else { return };
    let name = format!("ctl_{}", std::process::id());
    // Anything an earlier, failed run left registered would start with this plane.
    for stale in nineveh_store::registry::list(&pool).await.unwrap() {
        if stale.name.starts_with("ctl_") {
            nineveh_store::registry::delete(&pool, &stale.name)
                .await
                .unwrap();
        }
    }

    let ops = [
        Op::Deposit { user: 0, amount: 5 },
        Op::Deposit { user: 1, amount: 7 },
        Op::CreateVault { vault: 0 },
        Op::SetPosition {
            vault: 0,
            user: 1,
            size: 7,
        },
        Op::SetPosition {
            vault: 0,
            user: 2,
            size: 9,
        },
        Op::DeletePosition { vault: 0, user: 1 },
        Op::SetShare {
            vault: 0,
            user: 0,
            amount: 2,
        },
        Op::Deposit { user: 2, amount: 1 },
    ];
    let (transactions, model) = vault::transactions(&ops);
    let count = |n: usize| i64::try_from(n).unwrap();
    // A SmartTable mirror has a row per entry.
    let shares = count(
        model
            .shares
            .values()
            .map(std::collections::BTreeMap::len)
            .sum(),
    );
    let chain = Arc::new(Scripted {
        transactions: Arc::new(transactions),
    });
    let options = RunOptions {
        streams: 1,
        ..RunOptions::default()
    };
    let plane = ControlPlane::start(Arc::clone(&chain), pool.clone(), options.clone())
        .await
        .unwrap();
    let app = router(Arc::clone(&plane));

    // Inspect the contract.
    let (status, catalog) = call(
        &app,
        Method::GET,
        "/control/v1/inspect?network=testnet&address=0xcafe",
        None,
    )
    .await;
    assert_eq!(status, StatusCode::OK, "{catalog}");
    let ids: Vec<&str> = catalog["items"]
        .as_array()
        .unwrap()
        .iter()
        .map(|i| i["id"].as_str().unwrap())
        .collect();
    let id = |s: &str| format!("{}::vault::{s}", vault::MODULE);
    for expected in [
        "DepositEvent",
        "WithdrawEvent",
        "Vault",
        "Vault.positions",
        "Vault.shares",
    ] {
        assert!(
            ids.contains(&id(expected).as_str()),
            "{expected} in {ids:?}"
        );
    }
    let (status, _) = call(
        &app,
        Method::GET,
        "/control/v1/inspect?network=testnet&address=0xbeef",
        None,
    )
    .await;
    assert_eq!(status, StatusCode::NOT_FOUND);
    let (status, _) = call(
        &app,
        Method::GET,
        "/control/v1/inspect?network=testnet&address=nope",
        None,
    )
    .await;
    assert_eq!(status, StatusCode::BAD_REQUEST);

    // Scaffold from picks, and create the project.
    let (status, scaffolded) = call(
        &app,
        Method::POST,
        "/control/v1/scaffold",
        Some(json!({
            "name": name,
            "network": "testnet",
            "picks": [id("DepositEvent"), id("Vault.positions")],
        })),
    )
    .await;
    assert_eq!(status, StatusCode::OK, "{scaffolded}");
    let config = scaffolded["config"].as_str().unwrap().to_owned();
    let (status, created) = call(
        &app,
        Method::POST,
        "/control/v1/projects",
        Some(json!({ "config": config })),
    )
    .await;
    assert_eq!(status, StatusCode::CREATED, "{created}");
    assert_eq!(created["api"], json!(format!("/projects/{name}")));
    let (status, _) = call(
        &app,
        Method::POST,
        "/control/v1/projects",
        Some(json!({ "config": config })),
    )
    .await;
    assert_eq!(status, StatusCode::CONFLICT);

    // Its tables fill, served under the project's path.
    wait_for_rows(&app, &name, "deposit_event", count(model.deposits)).await;
    wait_for_rows(&app, &name, "vault_positions", count(model.positions.len())).await;
    wait_for_state(&app, &name, "running").await;
    let (status, listed) = call(&app, Method::GET, "/control/v1/projects", None).await;
    assert_eq!(status, StatusCode::OK);
    let listed = listed
        .as_array()
        .unwrap()
        .iter()
        .find(|p| p["name"] == json!(name))
        .unwrap()
        .clone();
    assert_eq!(listed["running"], json!(true));
    assert_eq!(
        listed["pipeline"]["chain_version"],
        json!("1008"),
        "{listed}"
    );

    // A config with problems is refused with located diagnostics, and a rename too.
    let broken = config.replace("log: deposit_event", "log: nothing");
    let (status, refused) = call(
        &app,
        Method::PUT,
        &format!("/control/v1/projects/{name}"),
        Some(json!({ "config": broken })),
    )
    .await;
    assert_eq!(status, StatusCode::UNPROCESSABLE_ENTITY, "{refused}");
    assert!(
        refused["details"]
            .as_str()
            .unwrap()
            .contains("nineveh.yaml:"),
        "{refused}"
    );
    let renamed = config.replace(&format!("name: {name}"), "name: other");
    let (status, _) = call(
        &app,
        Method::PUT,
        &format!("/control/v1/projects/{name}"),
        Some(json!({ "config": renamed })),
    )
    .await;
    assert_eq!(status, StatusCode::BAD_REQUEST);

    // Following the shares table too rebuilds the project, and the old tables survive.
    let changed = config
        .replace(
            "\nstate:\n",
            &format!(
                "  vault_shares:\n    table: \"{}\"\n\nstate:\n",
                id("Vault.shares")
            ),
        )
        .trim_end()
        .to_owned()
        + "\n  vault_shares:\n    mirror: vault_shares\n";
    let (status, updated) = call(
        &app,
        Method::PUT,
        &format!("/control/v1/projects/{name}"),
        Some(json!({ "config": changed })),
    )
    .await;
    assert_eq!(status, StatusCode::OK, "{updated}");
    wait_for_rows(&app, &name, "vault_shares", shares).await;
    wait_for_rows(&app, &name, "deposit_event", count(model.deposits)).await;

    // Stop and start.
    let (status, stopped) = call(
        &app,
        Method::POST,
        &format!("/control/v1/projects/{name}/stop"),
        None,
    )
    .await;
    assert_eq!(status, StatusCode::OK, "{stopped}");
    assert_eq!(stopped["state"], json!("stopped"));
    assert_eq!(state(&app, &name).await["running"], json!(false));
    let (status, started) = call(
        &app,
        Method::POST,
        &format!("/control/v1/projects/{name}/start"),
        None,
    )
    .await;
    assert_eq!(status, StatusCode::OK, "{started}");
    assert_eq!(started["running"], json!(true));

    // A new control plane picks it up from the registry, config and all.
    plane.shutdown().await;
    drop(app);
    let plane = ControlPlane::start(chain, pool.clone(), options)
        .await
        .unwrap();
    let app = router(Arc::clone(&plane));
    let again = state(&app, &name).await;
    assert_eq!(again["config"], json!(changed));
    assert_eq!(again["running"], json!(true));
    wait_for_rows(&app, &name, "vault_shares", shares).await;

    // Delete it: the project, its routes and its state go.
    let (status, _) = call(
        &app,
        Method::DELETE,
        &format!("/control/v1/projects/{name}"),
        None,
    )
    .await;
    assert_eq!(status, StatusCode::NO_CONTENT);
    let (status, _) = call(
        &app,
        Method::GET,
        &format!("/projects/{name}/v1/status"),
        None,
    )
    .await;
    assert_eq!(status, StatusCode::NOT_FOUND);
    let (status, _) = call(
        &app,
        Method::DELETE,
        &format!("/control/v1/projects/{name}"),
        None,
    )
    .await;
    assert_eq!(status, StatusCode::NOT_FOUND);
    let exists: bool =
        sqlx::query_scalar("SELECT EXISTS (SELECT 1 FROM pg_namespace WHERE nspname = $1)")
            .bind(&name)
            .fetch_one(&pool)
            .await
            .unwrap();
    assert!(!exists, "the schema was dropped");
    plane.shutdown().await;
}
