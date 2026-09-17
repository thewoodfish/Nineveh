//! Hosted mode end to end (ADR 0018), with a fake GitHub: signing in, one account not
//! reaching another's projects, project API keys by header, `apikey` header and query
//! parameter, revoking a key, and signing out. And local mode: no sign-in.
//!
//! Needs `NINEVEH_TEST_DATABASE_URL`; skips without it, except in CI.

#![allow(
    clippy::unwrap_used,
    clippy::panic,
    clippy::print_stderr,
    reason = "test-only crate: helpers panic on unexpected results and note skips"
)]

use std::future::Future;
use std::pin::Pin;
use std::sync::Arc;

use axum::Router;
use axum::http::{Method, StatusCode, header};
use nineveh_control::{Access, AuthError, ControlPlane, ExternalUser, IdentityProvider, router};
use nineveh_testkit::vault::{self, Op};
use serde_json::{Value, json};

mod common;

use common::{call, call_as, chain, logs, options, pool};

const STUDIO: &str = "https://studio.example";

/// GitHub, where each code signs in the user it names.
struct FakeGitHub;

impl IdentityProvider for FakeGitHub {
    fn authorize_url(&self, state: &str) -> String {
        format!("https://github.example/authorize?state={state}")
    }

    fn user<'a>(
        &'a self,
        code: &'a str,
    ) -> Pin<Box<dyn Future<Output = Result<ExternalUser, AuthError>> + Send + 'a>> {
        let user = match code {
            "refused" => Err(AuthError::Refused("The code passed is incorrect.".into())),
            login => Ok(ExternalUser {
                // Unique per test run and person.
                id: i64::from(std::process::id()) * 10 + i64::try_from(login.len()).unwrap(),
                login: login.to_owned(),
                name: None,
                avatar_url: None,
            }),
        };
        Box::pin(async move { user })
    }
}

/// Sign in as `login` the way a browser does, returning the session token.
async fn sign_in(app: &Router, login: &str) -> String {
    let (status, headers, _) = call_as(app, None, Method::GET, "/auth/github", None).await;
    assert_eq!(status, StatusCode::SEE_OTHER);
    let to_github = headers[header::LOCATION].to_str().unwrap().to_owned();
    let state = to_github.split("state=").nth(1).unwrap().to_owned();

    let callback = format!("/auth/github/callback?code={login}&state={state}");
    let (status, headers, _) = call_as(app, None, Method::GET, &callback, None).await;
    assert_eq!(status, StatusCode::SEE_OTHER);
    let to_studio = headers[header::LOCATION].to_str().unwrap().to_owned();
    let token = to_studio
        .strip_prefix(&format!("{STUDIO}/auth#token="))
        .unwrap_or_else(|| panic!("{to_studio}"))
        .to_owned();
    assert!(token.starts_with("nvs_"));

    // The same callback again: the state is spent.
    let (_, headers, _) = call_as(app, None, Method::GET, &callback, None).await;
    let replayed = headers[header::LOCATION].to_str().unwrap();
    assert!(
        replayed.starts_with(&format!("{STUDIO}/auth#error=")),
        "{replayed}"
    );
    token
}

/// Create a vault project as the holder of `token`.
async fn create(app: &Router, token: &str, name: &str) {
    let deposits = format!("{}::vault::DepositEvent", vault::MODULE);
    let (status, scaffolded) = {
        let (s, _, b) = call_as(
            app,
            Some(token),
            Method::POST,
            "/control/v1/scaffold",
            Some(json!({ "name": name, "network": "testnet", "picks": [deposits] })),
        )
        .await;
        (s, b)
    };
    assert_eq!(status, StatusCode::OK, "{scaffolded}");
    let (status, _, created) = call_as(
        app,
        Some(token),
        Method::POST,
        "/control/v1/projects",
        Some(json!({ "config": scaffolded["config"] })),
    )
    .await;
    assert_eq!(status, StatusCode::CREATED, "{created}");
}

/// Poll `uri` until it answers 200: a project's tables exist once its pipeline has
/// opened its store, a moment after it's created.
async fn wait_for_ok(app: &Router, token: Option<&str>, uri: &str) -> Value {
    let mut last = (StatusCode::OK, Value::Null);
    for _ in 0..100 {
        let (status, _, body) = call_as(app, token, Method::GET, uri, None).await;
        if status == StatusCode::OK {
            return body;
        }
        last = (status, body);
        tokio::time::sleep(std::time::Duration::from_millis(100)).await;
    }
    panic!("{uri} never answered 200: {} {}", last.0, last.1);
}

fn names(projects: &Value) -> Vec<String> {
    projects
        .as_array()
        .unwrap()
        .iter()
        .map(|p| p["name"].as_str().unwrap().to_owned())
        .collect()
}

#[tokio::test(flavor = "multi_thread")]
#[allow(clippy::too_many_lines, reason = "two accounts' lives, step by step")]
async fn accounts_reach_only_their_projects_and_keys_reach_one_project() {
    logs();
    let Some(pool) = pool("hosted_accounts").await else {
        return;
    };
    let (chain, _) = chain(&[
        Op::Deposit { user: 0, amount: 5 },
        Op::Deposit { user: 1, amount: 7 },
    ]);
    let plane = ControlPlane::start(chain, pool.clone(), options())
        .await
        .unwrap();
    let app = router(
        Arc::clone(&plane),
        Access::Hosted {
            provider: Arc::new(FakeGitHub),
            studio_url: STUDIO.into(),
        },
    );
    let pid = std::process::id();
    let (alices, bobs) = (format!("hst_{pid}_a"), format!("hst_{pid}_b"));

    // Nothing without signing in.
    let (status, me) = call(&app, Method::GET, "/control/v1/me", None).await;
    assert_eq!(status, StatusCode::UNAUTHORIZED);
    assert_eq!(me["sign_in"], "/auth/github");
    let (status, _) = call(&app, Method::GET, "/control/v1/projects", None).await;
    assert_eq!(status, StatusCode::UNAUTHORIZED);
    let (status, _, _) = call_as(
        &app,
        Some("nvs_forged"),
        Method::GET,
        "/control/v1/projects",
        None,
    )
    .await;
    assert_eq!(status, StatusCode::UNAUTHORIZED);

    // A refused sign-in goes back to Studio with the reason.
    let (_, headers, _) = call_as(&app, None, Method::GET, "/auth/github", None).await;
    let state = headers[header::LOCATION]
        .to_str()
        .unwrap()
        .split("state=")
        .nth(1)
        .unwrap()
        .to_owned();
    let (_, headers, _) = call_as(
        &app,
        None,
        Method::GET,
        &format!("/auth/github/callback?code=refused&state={state}"),
        None,
    )
    .await;
    assert!(
        headers[header::LOCATION]
            .to_str()
            .unwrap()
            .contains("#error=GitHub%20refused")
    );

    let alice = sign_in(&app, "alice").await;
    let bob = sign_in(&app, "bob").await;
    let (status, _, me) = call_as(&app, Some(&alice), Method::GET, "/control/v1/me", None).await;
    assert_eq!(status, StatusCode::OK);
    assert_eq!(
        (me["mode"].as_str(), me["account"]["login"].as_str()),
        (Some("hosted"), Some("alice"))
    );

    create(&app, &alice, &alices).await;
    create(&app, &bob, &bobs).await;

    // Each account lists and reaches only its own; another's is as if missing.
    let (_, _, listed) = call_as(
        &app,
        Some(&alice),
        Method::GET,
        "/control/v1/projects",
        None,
    )
    .await;
    assert_eq!(names(&listed), [alices.as_str()]);
    let (_, _, listed) = call_as(&app, Some(&bob), Method::GET, "/control/v1/projects", None).await;
    assert_eq!(names(&listed), [bobs.as_str()]);
    for (method, path) in [
        (Method::GET, format!("/control/v1/projects/{alices}")),
        (Method::POST, format!("/control/v1/projects/{alices}/stop")),
        (Method::GET, format!("/control/v1/projects/{alices}/keys")),
        (Method::DELETE, format!("/control/v1/projects/{alices}")),
    ] {
        let (status, _, _) = call_as(&app, Some(&bob), method.clone(), &path, None).await;
        assert_eq!(status, StatusCode::NOT_FOUND, "{method} {path}");
    }

    // A project's API: its owner's session, but no one else's, and nothing bare.
    let tables = format!("/projects/{alices}/v1/tables");
    let (status, _, _) = call_as(&app, Some(&alice), Method::GET, &tables, None).await;
    assert_eq!(status, StatusCode::OK);
    let (status, _, refused) = call_as(&app, Some(&bob), Method::GET, &tables, None).await;
    assert_eq!(status, StatusCode::UNAUTHORIZED);
    assert!(
        refused["error"].as_str().unwrap().contains("API key"),
        "{refused}"
    );
    let (status, _, _) = call_as(&app, None, Method::GET, &tables, None).await;
    assert_eq!(status, StatusCode::UNAUTHORIZED);

    // A key, shown once.
    let keys = format!("/control/v1/projects/{alices}/keys");
    let (status, _, created) = call_as(
        &app,
        Some(&alice),
        Method::POST,
        &keys,
        Some(json!({ "label": "web app" })),
    )
    .await;
    assert_eq!(status, StatusCode::CREATED, "{created}");
    let key = created["key"].as_str().unwrap().to_owned();
    assert!(
        key.starts_with("nvk_")
            && created["prefix"]
                .as_str()
                .is_some_and(|p| key.starts_with(p))
    );
    let (_, _, listed) = call_as(&app, Some(&alice), Method::GET, &keys, None).await;
    assert_eq!(listed.as_array().unwrap().len(), 1);
    assert!(
        listed[0].get("key").is_none(),
        "never shown again: {listed}"
    );
    let (status, _, _) = call_as(
        &app,
        Some(&alice),
        Method::POST,
        &keys,
        Some(json!({ "label": "  " })),
    )
    .await;
    assert_eq!(status, StatusCode::BAD_REQUEST);

    // The key reaches the project as a bearer token, an `apikey` header, or `?apikey=`,
    // which isn't taken for a column filter.
    let (status, _, _) = call_as(&app, Some(&key), Method::GET, &tables, None).await;
    assert_eq!(status, StatusCode::OK);
    let rows = format!("/projects/{alices}/v1/tables/deposit_event?count=exact&apikey={key}");
    let body = wait_for_ok(&app, None, &rows).await;
    assert!(body["count"].is_number(), "{body}");
    let header_request = axum::http::Request::builder()
        .uri(&tables)
        .header("apikey", &key)
        .body(axum::body::Body::empty())
        .unwrap();
    let response = tower::ServiceExt::oneshot(app.clone(), header_request)
        .await
        .unwrap();
    assert_eq!(response.status(), StatusCode::OK);
    // Only that project's.
    let (status, _, _) = call_as(
        &app,
        Some(&key),
        Method::GET,
        &format!("/projects/{bobs}/v1/tables"),
        None,
    )
    .await;
    assert_eq!(status, StatusCode::UNAUTHORIZED);
    // And never the control API.
    let (status, _, _) = call_as(&app, Some(&key), Method::GET, "/control/v1/projects", None).await;
    assert_eq!(status, StatusCode::UNAUTHORIZED);

    // Revoked, it reaches nothing.
    let id = created["id"].as_i64().unwrap();
    let (status, _, _) = call_as(
        &app,
        Some(&alice),
        Method::DELETE,
        &format!("{keys}/{id}"),
        None,
    )
    .await;
    assert_eq!(status, StatusCode::NO_CONTENT);
    let (status, _, _) = call_as(&app, Some(&key), Method::GET, &tables, None).await;
    assert_eq!(status, StatusCode::UNAUTHORIZED);

    // Signed out, the session is gone.
    let (status, _, _) = call_as(&app, Some(&bob), Method::POST, "/control/v1/logout", None).await;
    assert_eq!(status, StatusCode::NO_CONTENT);
    let (status, _, _) = call_as(&app, Some(&bob), Method::GET, "/control/v1/projects", None).await;
    assert_eq!(status, StatusCode::UNAUTHORIZED);

    for (token, name) in [(&alice, &alices)] {
        let (status, _, _) = call_as(
            &app,
            Some(token),
            Method::DELETE,
            &format!("/control/v1/projects/{name}"),
            None,
        )
        .await;
        assert_eq!(status, StatusCode::NO_CONTENT);
    }
    plane.shutdown().await;
}

#[tokio::test(flavor = "multi_thread")]
async fn local_mode_needs_no_sign_in() {
    logs();
    let Some(pool) = pool("hosted_local").await else {
        return;
    };
    let (chain, _) = chain(&[]);
    let plane = ControlPlane::start(chain, pool.clone(), options())
        .await
        .unwrap();
    let app = router(Arc::clone(&plane), Access::Local);
    let (status, me) = call(&app, Method::GET, "/control/v1/me", None).await;
    assert_eq!(status, StatusCode::OK);
    assert_eq!(me, json!({ "mode": "local", "account": null }));
    let (status, _) = call(&app, Method::GET, "/control/v1/projects", None).await;
    assert_eq!(status, StatusCode::OK);
    let (status, _) = call(&app, Method::GET, "/auth/github", None).await;
    assert_eq!(status, StatusCode::NOT_FOUND);
    plane.shutdown().await;
}
