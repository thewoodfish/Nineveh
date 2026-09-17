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

use std::fmt::Write as _;
use std::sync::Arc;
use std::time::Duration;

use axum::Router;
use axum::http::{Method, StatusCode};
use nineveh_control::{Access, ControlPlane, router};
use nineveh_testkit::vault::{self, Op};
use serde_json::{Value, json};

mod common;

use common::{call, chain, logs, options, pool};

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
    let Some(pool) = pool("control").await else {
        return;
    };
    logs();
    let name = format!("ctl_{}", std::process::id());

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
    let (chain, model) = chain(&ops);
    let count = |n: usize| i64::try_from(n).unwrap();
    // A SmartTable mirror has a row per entry.
    let shares = count(
        model
            .shares
            .values()
            .map(std::collections::BTreeMap::len)
            .sum(),
    );
    let options = options();
    let plane = ControlPlane::start(Arc::clone(&chain), pool.clone(), options.clone())
        .await
        .unwrap();
    let app = router(Arc::clone(&plane), Access::Local);

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

    // What a rule on each source can read, for the state-table editor.
    let (status, sources) = call(
        &app,
        Method::GET,
        &format!("/control/v1/projects/{name}/sources"),
        None,
    )
    .await;
    assert_eq!(status, StatusCode::OK, "{sources}");
    let source = |name: &str| {
        sources
            .as_array()
            .unwrap()
            .iter()
            .find(|s| s["name"] == json!(name))
            .unwrap_or_else(|| panic!("no source {name} in {sources}"))
            .clone()
    };
    let fields = |source: &Value, key: &str| {
        source[key]
            .as_array()
            .unwrap()
            .iter()
            .map(|f| {
                (
                    f["name"].as_str().unwrap().to_owned(),
                    f["type"].as_str().unwrap().to_owned(),
                )
            })
            .collect::<Vec<_>>()
    };
    let deposits = source("deposit_event");
    assert_eq!(deposits["kind"], json!("event"));
    assert_eq!(deposits["deletes"], json!(false));
    assert_eq!(
        fields(&deposits, "fields"),
        [
            ("user".to_owned(), "address".to_owned()),
            ("amount".to_owned(), "u64".to_owned()),
        ]
    );
    let positions = source("vault_positions");
    assert_eq!(positions["kind"], json!("table"));
    assert_eq!(positions["deletes"], json!(true), "tables delete");
    assert_eq!(
        fields(&positions, "fields"),
        [
            ("handle".to_owned(), "address".to_owned()),
            ("key".to_owned(), "address".to_owned()),
            ("value".to_owned(), "json".to_owned()),
        ]
    );
    // A `.deleted` rule sees only what identifies the row.
    assert_eq!(
        fields(&positions, "delete_fields"),
        [
            ("handle".to_owned(), "address".to_owned()),
            ("key".to_owned(), "address".to_owned()),
        ]
    );

    // A state table is checked against the pinned layouts without saving it.
    let state_block = |rule: &str| {
        let mut yaml = String::new();
        yaml.push_str("  depositors:\n");
        yaml.push_str("    key: [user]\n");
        yaml.push_str("    columns:\n");
        yaml.push_str("      user: address\n");
        yaml.push_str("      deposits: { type: u64, default: 0 }\n");
        yaml.push_str("      total: { type: u128, default: 0 }\n");
        yaml.push_str("    reduce:\n");
        writeln!(yaml, "      - {{ on: deposit_event, set: {{ {rule} }} }}").unwrap();
        yaml
    };
    let with_state = |rule: &str| format!("{config}{}", state_block(rule));
    let depositors = "deposits: \"deposits + 1\", total: \"total + u128(amount)\"";
    let good = with_state(depositors);
    let (status, checked) = call(
        &app,
        Method::POST,
        &format!("/control/v1/projects/{name}/check"),
        Some(json!({ "config": good })),
    )
    .await;
    assert_eq!(status, StatusCode::OK, "{checked}");
    let bad = with_state("deposits: \"deposits + nope\"");
    let (status, refused) = call(
        &app,
        Method::POST,
        &format!("/control/v1/projects/{name}/check"),
        Some(json!({ "config": bad })),
    )
    .await;
    assert_eq!(status, StatusCode::UNPROCESSABLE_ENTITY, "{refused}");
    assert!(
        refused["details"].as_str().unwrap().contains("nope"),
        "the editor sees where: {refused}"
    );
    // The same table, folded over the chain's own transactions without saving it:
    // what the editor shows before you commit to a rule.
    let (status, preview) = call(
        &app,
        Method::POST,
        &format!("/control/v1/projects/{name}/preview"),
        Some(json!({ "config": good, "table": "depositors" })),
    )
    .await;
    assert_eq!(status, StatusCode::OK, "{preview}");
    assert_eq!(
        preview["row_count"],
        json!(3),
        "one row per depositor: {preview}"
    );
    let totals: Vec<&str> = preview["rows"]
        .as_array()
        .unwrap()
        .iter()
        .map(|row| row["total"].as_str().unwrap())
        .collect();
    assert_eq!(totals, ["5", "7", "1"], "the amounts deposited: {preview}");

    // A rule that typechecks but can't survive real data fails here rather than
    // halting the project after it's saved.
    let underflow = with_state("deposits: \"deposits + 1\", total: \"total - u128(amount)\"");
    let (status, checked) = call(
        &app,
        Method::POST,
        &format!("/control/v1/projects/{name}/check"),
        Some(json!({ "config": underflow })),
    )
    .await;
    assert_eq!(status, StatusCode::OK, "it's well typed: {checked}");
    let (status, refused) = call(
        &app,
        Method::POST,
        &format!("/control/v1/projects/{name}/preview"),
        Some(json!({ "config": underflow, "table": "depositors" })),
    )
    .await;
    assert_eq!(status, StatusCode::BAD_REQUEST, "{refused}");
    let error = refused["error"].as_str().unwrap();
    assert!(
        error.contains("overflowed"),
        "it says what would happen: {error}"
    );
    assert!(error.contains("depositors"), "and where: {error}");

    // Checking doesn't save: the project still has its two tables.
    let (_, listed) = call(
        &app,
        Method::GET,
        &format!("/projects/{name}/v1/tables"),
        None,
    )
    .await;
    assert_eq!(listed.as_array().unwrap().len(), 2, "{listed}");

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
        + "\n  vault_shares:\n    mirror: vault_shares\n"
        + &state_block(depositors)
        + "\nwebhooks:\n  my_backend:\n    url: https://example.invalid/hook\n    on: [deposit_event.inserted]\n";
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
    wait_for_rows(&app, &name, "depositors", 3).await;

    // A saved table comes back in the shape the editor edits, so opening one to
    // change it isn't a one-way trip into YAML.
    let (status, editing) = call(
        &app,
        Method::GET,
        &format!("/control/v1/projects/{name}/state/depositors"),
        None,
    )
    .await;
    assert_eq!(status, StatusCode::OK, "{editing}");
    assert_eq!(editing["kind"], json!("reduce"));
    assert_eq!(
        editing["columns"],
        json!([
            { "name": "user", "type": "address", "default": "", "nullable": false, "key": true },
            { "name": "deposits", "type": "u64", "default": "0", "nullable": false, "key": false },
            { "name": "total", "type": "u128", "default": "0", "nullable": false, "key": false },
        ]),
        "{editing}"
    );
    assert_eq!(
        editing["rules"],
        json!([{
            "on": "deposit_event",
            "deleted": false,
            "when": "",
            "keys": [],
            "sets": [
                { "column": "deposits", "expression": "deposits + 1" },
                { "column": "total", "expression": "total + u128(amount)" },
            ],
            "removes": false,
        }]),
        "{editing}"
    );
    // Its webhook endpoints, with the secret a receiver checks signatures with.
    let (status, hooks) = call(
        &app,
        Method::GET,
        &format!("/control/v1/projects/{name}/webhooks"),
        None,
    )
    .await;
    assert_eq!(status, StatusCode::OK, "{hooks}");
    assert_eq!(hooks.as_array().unwrap().len(), 1, "{hooks}");
    let hook = &hooks[0];
    assert_eq!(hook["name"], json!("my_backend"));
    assert_eq!(hook["on"], json!(["deposit_event.inserted"]));
    assert_eq!(hook["rows"], json!(true));
    assert!(
        hook["secret"].as_str().unwrap().starts_with("whsec_"),
        "{hook}"
    );
    let secret = hook["secret"].as_str().unwrap().to_owned();

    // Rotating gives it a new one.
    let (status, rotated) = call(
        &app,
        Method::POST,
        &format!("/control/v1/projects/{name}/webhooks/my_backend/rotate"),
        None,
    )
    .await;
    assert_eq!(status, StatusCode::OK, "{rotated}");
    assert_ne!(rotated["secret"].as_str().unwrap(), secret);
    let (status, refused) = call(
        &app,
        Method::POST,
        &format!("/control/v1/projects/{name}/webhooks/nobody/rotate"),
        None,
    )
    .await;
    assert_eq!(status, StatusCode::NOT_FOUND, "{refused}");

    // A mirror or log has no rules to edit, and says so.
    let (status, refused) = call(
        &app,
        Method::GET,
        &format!("/control/v1/projects/{name}/state/deposit_event"),
        None,
    )
    .await;
    assert_eq!(status, StatusCode::BAD_REQUEST, "{refused}");
    assert!(
        refused["error"].as_str().unwrap().contains("logs"),
        "{refused}"
    );

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
    let app = router(Arc::clone(&plane), Access::Local);
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
