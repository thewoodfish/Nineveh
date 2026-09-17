//! The control plane over HTTP (ADRs 0017, 0018):
//!
//! - `GET  /control/v1/me`: the mode, and in hosted mode the signed-in account.
//! - `POST /control/v1/logout`: end the session.
//! - `GET  /control/v1/inspect?network=&address=`: a contract's catalog.
//! - `POST /control/v1/scaffold`: a config from picks; see [`ScaffoldRequest`].
//! - `GET  /control/v1/projects`, `POST /control/v1/projects` (`{"config": yaml}`).
//! - `GET`, `PUT` (`{"config": yaml}`) and `DELETE /control/v1/projects/{name}`.
//! - `POST /control/v1/projects/{name}/start` and `/stop`.
//! - `GET  /control/v1/projects/{name}/sources`: what a rule on each source can read.
//! - `POST /control/v1/projects/{name}/check` (`{"config": yaml}`): check a config
//!   against the project's pinned layouts without saving it.
//! - `POST /control/v1/projects/{name}/preview` (`{"config": yaml, "table": name}`):
//!   the rows that table's rules would produce, folded over recent transactions.
//! - `GET  /control/v1/projects/{name}/state/{table}`: a saved state table in the
//!   shape the editor edits.
//! - `GET`, `POST` (`{"label"}`) `/control/v1/projects/{name}/keys`, and `DELETE
//!   /control/v1/projects/{name}/keys/{id}`: API keys. A new key is shown only once.
//! - `GET /auth/github`, `GET /auth/github/callback`: signing in, in hosted mode.
//! - `/projects/{name}/v1/…`: the project's state API and change feed, the routes
//!   `nineveh serve` has at `/v1/…`.
//!
//! In hosted mode the control API needs a session (`Authorization: Bearer nvs_…`), and
//! a project's routes need its key (`Authorization: Bearer nvk_…`, an `apikey` header or
//! an `apikey` query parameter) or its owner's session. Local mode needs neither.
//!
//! Errors are `{"error": message}`, plus `"details"` with located diagnostics when a
//! config has problems (422).

use std::sync::Arc;

use axum::body::Body;
use axum::extract::{Path, Query, Request, State};
use axum::http::{HeaderMap, StatusCode, Uri, header};
use axum::response::{IntoResponse, Redirect, Response};
use axum::routing::{any, delete, get, post};
use axum::{Json, Router};
use nineveh_core::Network;
use nineveh_store::accounts::{self, Account, ApiKey};
use serde::{Deserialize, Serialize};
use serde_json::json;
use tower::ServiceExt as _;
use tracing::{error, info, warn};

use crate::auth::{self, Access, SESSION_DAYS, SESSION_PREFIX};
use crate::chain::Chain;
use crate::plane::{Caller, ControlError, ControlPlane, ScaffoldRequest};

/// The control plane and how it's reached.
struct Server<C> {
    plane: Arc<ControlPlane<C>>,
    access: Access,
}

type Shared<C> = State<Arc<Server<C>>>;

/// The control API, sign-in, and every project's API, on one router.
pub fn router<C: Chain>(plane: Arc<ControlPlane<C>>, access: Access) -> Router {
    Router::new()
        .route("/control/v1/me", get(me::<C>))
        .route("/control/v1/logout", post(logout::<C>))
        .route("/control/v1/inspect", get(inspect::<C>))
        .route("/control/v1/scaffold", post(scaffold::<C>))
        .route("/control/v1/projects", get(list::<C>).post(create::<C>))
        .route(
            "/control/v1/projects/{name}",
            get(show::<C>).put(update::<C>).delete(remove::<C>),
        )
        .route("/control/v1/projects/{name}/sources", get(sources::<C>))
        .route("/control/v1/projects/{name}/check", post(check::<C>))
        .route("/control/v1/projects/{name}/preview", post(preview::<C>))
        .route(
            "/control/v1/projects/{name}/state/{table}",
            get(state_table::<C>),
        )
        .route("/control/v1/projects/{name}/start", post(start::<C>))
        .route("/control/v1/projects/{name}/stop", post(stop::<C>))
        .route(
            "/control/v1/projects/{name}/keys",
            get(keys::<C>).post(create_key::<C>),
        )
        .route(
            "/control/v1/projects/{name}/keys/{id}",
            delete(revoke_key::<C>),
        )
        .route("/auth/github", get(sign_in::<C>))
        .route("/auth/github/callback", get(callback::<C>))
        .route("/projects/{name}/{*rest}", any(project::<C>))
        .with_state(Arc::new(Server { plane, access }))
}

impl IntoResponse for ControlError {
    fn into_response(self) -> Response {
        let status = match &self {
            Self::Invalid { .. } => StatusCode::UNPROCESSABLE_ENTITY,
            Self::BadRequest(_) | Self::Scaffold(_) => StatusCode::BAD_REQUEST,
            Self::NotFound(_) => StatusCode::NOT_FOUND,
            Self::Conflict(_) => StatusCode::CONFLICT,
            Self::Unauthorized(_) => StatusCode::UNAUTHORIZED,
            // The chain's answer is what's wrong: a missing module or struct, or an
            // address nothing has touched.
            Self::Pin(e) if !e.is_retryable() => StatusCode::UNPROCESSABLE_ENTITY,
            Self::Chain(_) | Self::Pin(_) => StatusCode::BAD_GATEWAY,
            Self::Store(e) => {
                error!(error = %e, "control plane database failure");
                StatusCode::INTERNAL_SERVER_ERROR
            }
            // `ControlError` is non-exhaustive only for other crates.
            #[allow(unreachable_patterns)]
            _ => StatusCode::INTERNAL_SERVER_ERROR,
        };
        let body = match &self {
            Self::Invalid { message, details } => json!({ "error": message, "details": details }),
            Self::Unauthorized(message) => json!({ "error": message, "sign_in": "/auth/github" }),
            other => json!({ "error": other.to_string() }),
        };
        (status, Json(body)).into_response()
    }
}

/// A bearer token from `Authorization`.
fn bearer(headers: &HeaderMap) -> Option<&str> {
    headers
        .get(header::AUTHORIZATION)?
        .to_str()
        .ok()?
        .strip_prefix("Bearer ")
        .map(str::trim)
}

impl<C: Chain> Server<C> {
    /// The account whose session `headers` carry.
    async fn account(&self, headers: &HeaderMap) -> Result<Option<Account>, ControlError> {
        let Some(token) = bearer(headers).filter(|t| t.starts_with(SESSION_PREFIX)) else {
            return Ok(None);
        };
        Ok(accounts::session_account(self.plane.pool(), &auth::hash(token)).await?)
    }

    /// Who's calling the control API.
    async fn caller(&self, headers: &HeaderMap) -> Result<Caller, ControlError> {
        match &self.access {
            Access::Local => Ok(Caller::Local),
            Access::Hosted { .. } => self
                .account(headers)
                .await?
                .map(|a| Caller::Account(a.id))
                .ok_or_else(|| ControlError::Unauthorized("sign in to use Nineveh".into())),
        }
    }
}

#[derive(Debug, Serialize)]
struct Me {
    mode: &'static str,
    account: Option<AccountView>,
}

#[derive(Debug, Serialize)]
struct AccountView {
    login: String,
    name: Option<String>,
    avatar_url: Option<String>,
}

async fn me<C: Chain>(
    State(server): Shared<C>,
    headers: HeaderMap,
) -> Result<Json<Me>, ControlError> {
    Ok(Json(match &server.access {
        Access::Local => Me {
            mode: "local",
            account: None,
        },
        Access::Hosted { .. } => {
            let account = server
                .account(&headers)
                .await?
                .ok_or_else(|| ControlError::Unauthorized("sign in to use Nineveh".into()))?;
            Me {
                mode: "hosted",
                account: Some(AccountView {
                    login: account.login,
                    name: account.name,
                    avatar_url: account.avatar_url,
                }),
            }
        }
    }))
}

async fn logout<C: Chain>(
    State(server): Shared<C>,
    headers: HeaderMap,
) -> Result<StatusCode, ControlError> {
    if let Some(token) = bearer(&headers).filter(|t| t.starts_with(SESSION_PREFIX)) {
        accounts::end_session(server.plane.pool(), &auth::hash(token)).await?;
    }
    Ok(StatusCode::NO_CONTENT)
}

/// Send the browser to the identity provider, with a single-use `state`.
async fn sign_in<C: Chain>(State(server): Shared<C>) -> Result<Response, ControlError> {
    let Access::Hosted { provider, .. } = &server.access else {
        return Err(ControlError::NotFound(
            "sign-in is off: this control plane runs in local mode".into(),
        ));
    };
    let (state, _) = auth::new_token("").map_err(|e| ControlError::BadRequest(e.to_string()))?;
    accounts::put_oauth_state(server.plane.pool(), &state).await?;
    Ok(Redirect::to(&provider.authorize_url(&state)).into_response())
}

#[derive(Debug, Deserialize)]
struct CallbackParams {
    code: Option<String>,
    state: Option<String>,
    error_description: Option<String>,
}

/// Finish signing in: spend the state, learn who it is, and hand Studio a session.
async fn callback<C: Chain>(
    State(server): Shared<C>,
    Query(params): Query<CallbackParams>,
) -> Result<Response, ControlError> {
    let Access::Hosted {
        provider,
        studio_url,
    } = &server.access
    else {
        return Err(ControlError::NotFound(
            "sign-in is off: this control plane runs in local mode".into(),
        ));
    };
    let back =
        |fragment: String| Redirect::to(&format!("{studio_url}/auth#{fragment}")).into_response();
    let failed = |message: &str| back(format!("error={}", fragment_encode(message)));
    if let Some(description) = params.error_description {
        return Ok(failed(&description));
    }
    let (Some(code), Some(state)) = (params.code, params.state) else {
        return Ok(failed("the sign-in came back incomplete; try again"));
    };
    if !accounts::take_oauth_state(server.plane.pool(), &state).await? {
        return Ok(failed(
            "that sign-in expired or was already used; try again",
        ));
    }
    let user = match provider.user(&code).await {
        Ok(user) => user,
        Err(e) => {
            warn!(error = %e, "sign-in failed");
            return Ok(failed(&e.to_string()));
        }
    };
    let account = accounts::sign_in(
        server.plane.pool(),
        user.id,
        &user.login,
        user.name.as_deref(),
        user.avatar_url.as_deref(),
    )
    .await?;
    let (token, hash) =
        auth::new_token(SESSION_PREFIX).map_err(|e| ControlError::BadRequest(e.to_string()))?;
    accounts::create_session(server.plane.pool(), account.id, &hash, SESSION_DAYS).await?;
    info!(account = account.id, login = %account.login, "signed in");
    Ok(back(format!("token={token}")))
}

/// Percent-encode text for a URL fragment.
fn fragment_encode(text: &str) -> String {
    use std::fmt::Write as _;
    text.bytes().fold(String::new(), |mut out, b| {
        if b.is_ascii_alphanumeric() || matches!(b, b'-' | b'_' | b'.' | b'~') {
            out.push(char::from(b));
        } else {
            let _ = write!(out, "%{b:02X}");
        }
        out
    })
}

#[derive(Debug, Deserialize)]
struct InspectParams {
    network: String,
    address: String,
}

fn network(name: &str) -> Result<Network, ControlError> {
    name.parse().map_err(|_| {
        ControlError::BadRequest(format!(
            "`{name}` isn't a network: use mainnet, testnet or devnet"
        ))
    })
}

async fn inspect<C: Chain>(
    State(server): Shared<C>,
    headers: HeaderMap,
    Query(params): Query<InspectParams>,
) -> Result<impl IntoResponse, ControlError> {
    server.caller(&headers).await?;
    Ok(Json(
        server
            .plane
            .inspect(network(&params.network)?, &params.address)
            .await?,
    ))
}

async fn scaffold<C: Chain>(
    State(server): Shared<C>,
    headers: HeaderMap,
    Json(request): Json<ScaffoldRequest>,
) -> Result<impl IntoResponse, ControlError> {
    server.caller(&headers).await?;
    let config = server.plane.scaffold(request).await?;
    Ok(Json(json!({ "config": config })))
}

#[derive(Debug, Deserialize)]
struct ConfigBody {
    config: String,
}

#[derive(Debug, Deserialize)]
struct PreviewBody {
    config: String,
    /// The state table to fold and show.
    table: String,
}

async fn list<C: Chain>(
    State(server): Shared<C>,
    headers: HeaderMap,
) -> Result<impl IntoResponse, ControlError> {
    let caller = server.caller(&headers).await?;
    Ok(Json(server.plane.list(caller).await))
}

async fn create<C: Chain>(
    State(server): Shared<C>,
    headers: HeaderMap,
    Json(body): Json<ConfigBody>,
) -> Result<impl IntoResponse, ControlError> {
    let caller = server.caller(&headers).await?;
    Ok((
        StatusCode::CREATED,
        Json(server.plane.create(caller, &body.config).await?),
    ))
}

async fn show<C: Chain>(
    State(server): Shared<C>,
    headers: HeaderMap,
    Path(name): Path<String>,
) -> Result<impl IntoResponse, ControlError> {
    let caller = server.caller(&headers).await?;
    Ok(Json(server.plane.get(caller, &name).await?))
}

async fn update<C: Chain>(
    State(server): Shared<C>,
    headers: HeaderMap,
    Path(name): Path<String>,
    Json(body): Json<ConfigBody>,
) -> Result<impl IntoResponse, ControlError> {
    let caller = server.caller(&headers).await?;
    Ok(Json(
        server.plane.update(caller, &name, &body.config).await?,
    ))
}

async fn remove<C: Chain>(
    State(server): Shared<C>,
    headers: HeaderMap,
    Path(name): Path<String>,
) -> Result<impl IntoResponse, ControlError> {
    let caller = server.caller(&headers).await?;
    server.plane.delete(caller, &name).await?;
    Ok(StatusCode::NO_CONTENT)
}

async fn sources<C: Chain>(
    State(server): Shared<C>,
    headers: HeaderMap,
    Path(name): Path<String>,
) -> Result<impl IntoResponse, ControlError> {
    let caller = server.caller(&headers).await?;
    Ok(Json(server.plane.sources(caller, &name).await?))
}

async fn check<C: Chain>(
    State(server): Shared<C>,
    headers: HeaderMap,
    Path(name): Path<String>,
    Json(body): Json<ConfigBody>,
) -> Result<impl IntoResponse, ControlError> {
    let caller = server.caller(&headers).await?;
    server.plane.check(caller, &name, &body.config).await?;
    Ok(Json(json!({ "ok": true })))
}

async fn preview<C: Chain>(
    State(server): Shared<C>,
    headers: HeaderMap,
    Path(name): Path<String>,
    Json(body): Json<PreviewBody>,
) -> Result<impl IntoResponse, ControlError> {
    let caller = server.caller(&headers).await?;
    Ok(Json(
        server
            .plane
            .preview(caller, &name, &body.config, &body.table)
            .await?,
    ))
}

async fn state_table<C: Chain>(
    State(server): Shared<C>,
    headers: HeaderMap,
    Path((name, table)): Path<(String, String)>,
) -> Result<impl IntoResponse, ControlError> {
    let caller = server.caller(&headers).await?;
    Ok(Json(server.plane.state_table(caller, &name, &table).await?))
}

async fn start<C: Chain>(
    State(server): Shared<C>,
    headers: HeaderMap,
    Path(name): Path<String>,
) -> Result<impl IntoResponse, ControlError> {
    let caller = server.caller(&headers).await?;
    Ok(Json(server.plane.set_running(caller, &name, true).await?))
}

async fn stop<C: Chain>(
    State(server): Shared<C>,
    headers: HeaderMap,
    Path(name): Path<String>,
) -> Result<impl IntoResponse, ControlError> {
    let caller = server.caller(&headers).await?;
    Ok(Json(server.plane.set_running(caller, &name, false).await?))
}

/// An API key as its owner sees it; `key` only in the answer that creates it.
#[derive(Debug, Serialize)]
struct KeyView {
    id: i64,
    label: String,
    prefix: String,
    created_at: String,
    last_used_at: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    key: Option<String>,
}

impl KeyView {
    fn of(key: ApiKey, secret: Option<String>) -> Self {
        Self {
            id: key.id,
            label: key.label,
            prefix: key.prefix,
            created_at: key.created_at,
            last_used_at: key.last_used_at,
            key: secret,
        }
    }
}

#[derive(Debug, Deserialize)]
struct KeyBody {
    label: String,
}

async fn keys<C: Chain>(
    State(server): Shared<C>,
    headers: HeaderMap,
    Path(name): Path<String>,
) -> Result<impl IntoResponse, ControlError> {
    let caller = server.caller(&headers).await?;
    let keys = server.plane.keys(caller, &name).await?;
    Ok(Json(
        keys.into_iter()
            .map(|k| KeyView::of(k, None))
            .collect::<Vec<_>>(),
    ))
}

async fn create_key<C: Chain>(
    State(server): Shared<C>,
    headers: HeaderMap,
    Path(name): Path<String>,
    Json(body): Json<KeyBody>,
) -> Result<impl IntoResponse, ControlError> {
    let caller = server.caller(&headers).await?;
    let (secret, key) = server.plane.create_key(caller, &name, &body.label).await?;
    Ok((StatusCode::CREATED, Json(KeyView::of(key, Some(secret)))))
}

async fn revoke_key<C: Chain>(
    State(server): Shared<C>,
    headers: HeaderMap,
    Path((name, id)): Path<(String, i64)>,
) -> Result<impl IntoResponse, ControlError> {
    let caller = server.caller(&headers).await?;
    server.plane.revoke_key(caller, &name, id).await?;
    Ok(StatusCode::NO_CONTENT)
}

/// The credential a project request carries: a bearer token, an `apikey` header, or
/// an `apikey` query parameter, for a browser's `EventSource`.
fn credential<'r>(headers: &'r HeaderMap, query: &'r [(String, String)]) -> Option<&'r str> {
    bearer(headers)
        .or_else(|| headers.get("apikey").and_then(|v| v.to_str().ok()))
        .or_else(|| {
            query
                .iter()
                .find(|(k, _)| k == "apikey")
                .map(|(_, v)| v.as_str())
        })
}

impl<C: Chain> Server<C> {
    /// Whether a request carrying `credential` may reach project `name`: its key, or
    /// its owner's session. Always, in local mode.
    async fn may_reach(&self, name: &str, credential: Option<&str>) -> Result<bool, ControlError> {
        if matches!(self.access, Access::Local) {
            return Ok(true);
        }
        let Some(token) = credential else {
            return Ok(false);
        };
        let hash = auth::hash(token);
        if token.starts_with(auth::KEY_PREFIX) {
            let project = accounts::api_key_project(self.plane.pool(), &hash).await?;
            return Ok(project.as_deref() == Some(name));
        }
        if token.starts_with(SESSION_PREFIX) {
            let Some(account) = accounts::session_account(self.plane.pool(), &hash).await? else {
                return Ok(false);
            };
            return Ok(self.plane.owner(name).await == Some(Some(account.id)));
        }
        Ok(false)
    }
}

/// Hand `/projects/{name}/rest` to the project's own router as `/rest`, if the caller
/// may reach it.
async fn project<C: Chain>(
    State(server): Shared<C>,
    Path((name, rest)): Path<(String, String)>,
    request: Request,
) -> Response {
    let (mut parts, body) = request.into_parts();
    let query: Vec<(String, String)> = parts
        .uri
        .query()
        .map(|q| url_pairs(q).into_iter().collect())
        .unwrap_or_default();
    match server
        .may_reach(&name, credential(&parts.headers, &query))
        .await
    {
        Ok(true) => {}
        Ok(false) => {
            return ControlError::Unauthorized(format!(
                "`{name}` needs an API key: send `Authorization: Bearer nvk_…`, an `apikey` \
                 header, or `?apikey=`"
            ))
            .into_response();
        }
        Err(e) => return e.into_response(),
    }
    let Some(router) = server.plane.router(&name).await else {
        return ControlError::NotFound(format!("no project named `{name}`")).into_response();
    };
    // The key isn't a column filter: the project's API never sees it.
    let kept: Vec<&str> = parts
        .uri
        .query()
        .unwrap_or("")
        .split('&')
        .filter(|pair| !pair.is_empty() && !pair.starts_with("apikey="))
        .collect();
    let query = if kept.is_empty() {
        String::new()
    } else {
        format!("?{}", kept.join("&"))
    };
    let Ok(uri) = format!("/{rest}{query}").parse::<Uri>() else {
        return ControlError::BadRequest("that path isn't valid".into()).into_response();
    };
    parts.uri = uri;
    // The outer route's path parameters would reach the project's extractors too.
    parts.extensions = axum::http::Extensions::new();
    match router
        .oneshot(Request::from_parts(parts, Body::new(body)))
        .await
    {
        Ok(response) => response,
        Err(never) => match never {},
    }
}

/// A query string's decoded pairs.
fn url_pairs(query: &str) -> Vec<(String, String)> {
    Query::<Vec<(String, String)>>::try_from_uri(
        &format!("/?{query}")
            .parse()
            .unwrap_or_else(|_| Uri::from_static("/")),
    )
    .map(|Query(pairs)| pairs)
    .unwrap_or_default()
}
