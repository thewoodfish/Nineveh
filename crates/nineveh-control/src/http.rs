//! The control plane over HTTP (ADR 0017):
//!
//! - `GET  /control/v1/inspect?network=&address=`: a contract's catalog.
//! - `POST /control/v1/scaffold`: a config from picks; see [`ScaffoldRequest`].
//! - `GET  /control/v1/projects`, `POST /control/v1/projects` (`{"config": yaml}`).
//! - `GET`, `PUT` (`{"config": yaml}`) and `DELETE /control/v1/projects/{name}`.
//! - `POST /control/v1/projects/{name}/start` and `/stop`.
//! - `/projects/{name}/v1/…`: the project's state API and change feed, the routes
//!   `nineveh serve` has at `/v1/…`.
//!
//! Errors are `{"error": message}`, plus `"details"` with located diagnostics when a
//! config has problems (422).

use std::sync::Arc;

use axum::body::Body;
use axum::extract::{Path, Query, Request, State};
use axum::http::{StatusCode, Uri};
use axum::response::{IntoResponse, Response};
use axum::routing::{any, get, post};
use axum::{Json, Router};
use nineveh_core::Network;
use serde::Deserialize;
use serde_json::json;
use tower::ServiceExt as _;
use tracing::error;

use crate::chain::Chain;
use crate::plane::{ControlError, ControlPlane, ScaffoldRequest};

/// The control API and every project's API, on one router.
pub fn router<C: Chain>(plane: Arc<ControlPlane<C>>) -> Router {
    Router::new()
        .route("/control/v1/inspect", get(inspect::<C>))
        .route("/control/v1/scaffold", post(scaffold::<C>))
        .route("/control/v1/projects", get(list::<C>).post(create::<C>))
        .route(
            "/control/v1/projects/{name}",
            get(show::<C>).put(update::<C>).delete(remove::<C>),
        )
        .route("/control/v1/projects/{name}/start", post(start::<C>))
        .route("/control/v1/projects/{name}/stop", post(stop::<C>))
        .route("/projects/{name}/{*rest}", any(project::<C>))
        .with_state(plane)
}

impl IntoResponse for ControlError {
    fn into_response(self) -> Response {
        let status = match &self {
            Self::Invalid { .. } => StatusCode::UNPROCESSABLE_ENTITY,
            Self::BadRequest(_) | Self::Scaffold(_) => StatusCode::BAD_REQUEST,
            Self::NotFound(_) => StatusCode::NOT_FOUND,
            Self::Conflict(_) => StatusCode::CONFLICT,
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
            other => json!({ "error": other.to_string() }),
        };
        (status, Json(body)).into_response()
    }
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
    State(plane): State<Arc<ControlPlane<C>>>,
    Query(params): Query<InspectParams>,
) -> Result<impl IntoResponse, ControlError> {
    Ok(Json(
        plane
            .inspect(network(&params.network)?, &params.address)
            .await?,
    ))
}

async fn scaffold<C: Chain>(
    State(plane): State<Arc<ControlPlane<C>>>,
    Json(request): Json<ScaffoldRequest>,
) -> Result<impl IntoResponse, ControlError> {
    let config = plane.scaffold(request).await?;
    Ok(Json(json!({ "config": config })))
}

#[derive(Debug, Deserialize)]
struct ConfigBody {
    config: String,
}

async fn list<C: Chain>(State(plane): State<Arc<ControlPlane<C>>>) -> impl IntoResponse {
    Json(plane.list().await)
}

async fn create<C: Chain>(
    State(plane): State<Arc<ControlPlane<C>>>,
    Json(body): Json<ConfigBody>,
) -> Result<impl IntoResponse, ControlError> {
    Ok((StatusCode::CREATED, Json(plane.create(&body.config).await?)))
}

async fn show<C: Chain>(
    State(plane): State<Arc<ControlPlane<C>>>,
    Path(name): Path<String>,
) -> Result<impl IntoResponse, ControlError> {
    Ok(Json(plane.get(&name).await?))
}

async fn update<C: Chain>(
    State(plane): State<Arc<ControlPlane<C>>>,
    Path(name): Path<String>,
    Json(body): Json<ConfigBody>,
) -> Result<impl IntoResponse, ControlError> {
    Ok(Json(plane.update(&name, &body.config).await?))
}

async fn remove<C: Chain>(
    State(plane): State<Arc<ControlPlane<C>>>,
    Path(name): Path<String>,
) -> Result<impl IntoResponse, ControlError> {
    plane.delete(&name).await?;
    Ok(StatusCode::NO_CONTENT)
}

async fn start<C: Chain>(
    State(plane): State<Arc<ControlPlane<C>>>,
    Path(name): Path<String>,
) -> Result<impl IntoResponse, ControlError> {
    Ok(Json(plane.set_running(&name, true).await?))
}

async fn stop<C: Chain>(
    State(plane): State<Arc<ControlPlane<C>>>,
    Path(name): Path<String>,
) -> Result<impl IntoResponse, ControlError> {
    Ok(Json(plane.set_running(&name, false).await?))
}

/// Hand `/projects/{name}/rest` to the project's own router as `/rest`.
async fn project<C: Chain>(
    State(plane): State<Arc<ControlPlane<C>>>,
    Path((name, rest)): Path<(String, String)>,
    request: Request,
) -> Response {
    let Some(router) = plane.router(&name).await else {
        return ControlError::NotFound(format!("no project named `{name}`")).into_response();
    };
    let (mut parts, body) = request.into_parts();
    let query = parts
        .uri
        .query()
        .map_or_else(String::new, |q| format!("?{q}"));
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
