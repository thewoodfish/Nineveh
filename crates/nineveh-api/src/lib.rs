//! Nineveh's state API: REST over a project's state tables, generated from its config.
//!
//! It reads the tables the store writes and never the chain. Routes:
//!
//! - `GET /v1/status`: the project, its build (cursor, fingerprint, times), a rebuild
//!   in progress, and the pipeline's health when the pipeline runs in this process.
//! - `GET /v1/tables`: every state table's kind, key and columns.
//! - `GET /v1/tables/{name}`: rows. `limit` (default 50, at most 1000) and `offset`
//!   page; `order=column` or `order=column.desc` sorts (default: most recently changed
//!   first); any other parameter `column=value` filters on equality; `count=exact`
//!   also counts the matching rows.
//!
//! Rows are JSON in the change feed's shape (ADR 0008), plus `_version`, the version
//! of the row's last change. Wide integers are strings.

use std::sync::Arc;

use axum::extract::{Path, Query as QueryParams, State};
use axum::http::StatusCode;
use axum::response::{IntoResponse, Response};
use axum::routing::get;
use axum::{Json, Router};
use nineveh_config::Project;
use serde::Serialize;
use serde_json::{Value as Json_, json};
use sqlx::PgPool;
use tokio::sync::watch;
use tracing::error;

mod tables;

pub use tables::{ColumnInfo, TableInfo};

use tables::{DEFAULT_LIMIT, MAX_LIMIT, Query};

/// The suffix of a rebuild's schema (ADR 0016), to report its progress.
const SHADOW_SUFFIX: &str = "__next";

/// The pipeline's health, when it runs in the same process as the API.
#[derive(Debug, Clone, Default, Serialize)]
pub struct Health {
    /// `starting`, `running`, `retrying`, `halted` or `stopped`.
    pub phase: String,
    /// The chain's version when the pipeline started, as a decimal string.
    pub chain_version: Option<String>,
    /// Seconds between now and the last committed transaction's block time.
    pub lag_secs: Option<u64>,
    pub versions_per_sec: Option<u64>,
    pub retries: u32,
    pub last_error: Option<String>,
}

/// One project's API.
#[derive(Debug)]
pub struct Api {
    pool: PgPool,
    schema: String,
    project: String,
    network: String,
    tables: Vec<TableInfo>,
    health: watch::Receiver<Option<Health>>,
}

impl Api {
    /// Serve `project`'s state from `schema`. `health` carries the pipeline's health
    /// if it runs in this process; send nothing on it otherwise.
    #[must_use]
    pub fn new(
        pool: PgPool,
        schema: impl Into<String>,
        project: &Project,
        health: watch::Receiver<Option<Health>>,
    ) -> Self {
        let config = project.config();
        Self {
            pool,
            schema: schema.into(),
            project: config.name.name.clone(),
            network: config.network.to_string(),
            tables: TableInfo::all(project),
            health,
        }
    }

    #[must_use]
    pub fn tables(&self) -> &[TableInfo] {
        &self.tables
    }

    #[must_use]
    pub fn pool(&self) -> &PgPool {
        &self.pool
    }

    #[must_use]
    pub fn schema(&self) -> &str {
        &self.schema
    }
}

/// The API's routes.
pub fn router(api: Arc<Api>) -> Router {
    Router::new()
        .route("/v1/status", get(status))
        .route("/v1/tables", get(tables))
        .route("/v1/tables/{name}", get(rows))
        .with_state(api)
}

/// Why a request failed, as a JSON `{"error": …}` body.
#[derive(Debug, thiserror::Error)]
enum ApiError {
    #[error("{0}")]
    NotFound(String),
    #[error("{0}")]
    BadRequest(String),
    #[error(transparent)]
    Database(#[from] sqlx::Error),
}

impl IntoResponse for ApiError {
    fn into_response(self) -> Response {
        let (status, message) = match &self {
            Self::NotFound(m) => (StatusCode::NOT_FOUND, m.clone()),
            Self::BadRequest(m) => (StatusCode::BAD_REQUEST, m.clone()),
            // A value that doesn't cast to its column's type is the caller's mistake.
            Self::Database(sqlx::Error::Database(db))
                if db.code().is_some_and(|c| c.starts_with("22")) =>
            {
                (StatusCode::BAD_REQUEST, db.message().to_owned())
            }
            Self::Database(e) => {
                error!(error = %e, "query failed");
                (
                    StatusCode::INTERNAL_SERVER_ERROR,
                    "the database query failed".to_owned(),
                )
            }
        };
        (status, Json(json!({ "error": message }))).into_response()
    }
}

async fn status(State(api): State<Arc<Api>>) -> Result<Json<Json_>, ApiError> {
    let build = |row: Option<(Option<String>, String, String, String)>| {
        row.map(|(cursor, fingerprint, created, updated)| {
            json!({
                "cursor": cursor,
                "fingerprint": fingerprint,
                "created_at": created,
                "updated_at": updated,
            })
        })
    };
    let read = |schema: String| {
        let pool = api.pool.clone();
        async move {
            sqlx::query_as::<_, (Option<String>, String, String, String)>(
                "SELECT cursor::text, fingerprint,
                        to_char(created_at AT TIME ZONE 'UTC', 'YYYY-MM-DD\"T\"HH24:MI:SS.MS\"Z\"'),
                        to_char(updated_at AT TIME ZONE 'UTC', 'YYYY-MM-DD\"T\"HH24:MI:SS.MS\"Z\"')
                 FROM nineveh.projects WHERE schema_name = $1",
            )
            .bind(schema)
            .fetch_optional(&pool)
            .await
        }
    };
    let shadow = format!("{}{SHADOW_SUFFIX}", api.schema);
    let current = build(read(api.schema.clone()).await?);
    let rebuild = build(read(shadow.clone()).await?).map(|mut b| {
        b["schema"] = json!(shadow);
        b
    });
    let health = api.health.borrow().clone();
    Ok(Json(json!({
        "project": api.project,
        "network": api.network,
        "schema": api.schema,
        "build": current,
        "rebuild": rebuild,
        "pipeline": health,
    })))
}

async fn tables(State(api): State<Arc<Api>>) -> Json<Vec<TableInfo>> {
    Json(api.tables.clone())
}

async fn rows(
    State(api): State<Arc<Api>>,
    Path(name): Path<String>,
    QueryParams(params): QueryParams<Vec<(String, String)>>,
) -> Result<Json<Json_>, ApiError> {
    let table = api
        .tables
        .iter()
        .find(|t| t.name == name)
        .ok_or_else(|| ApiError::NotFound(format!("no state table `{name}`")))?;
    let mut query = Query {
        limit: DEFAULT_LIMIT,
        ..Query::default()
    };
    let mut count = false;
    for (key, value) in params {
        let number = |what: &str| {
            value
                .parse::<i64>()
                .ok()
                .filter(|n| *n >= 0)
                .ok_or_else(|| {
                    ApiError::BadRequest(format!("`{what}` must be a non-negative integer"))
                })
        };
        match key.as_str() {
            "limit" => query.limit = number("limit")?.min(MAX_LIMIT),
            "offset" => query.offset = number("offset")?,
            "order" => query.order = Some(value),
            "count" if value == "exact" => count = true,
            "count" => return Err(ApiError::BadRequest("`count` must be `exact`".into())),
            _ => query.filters.push((key, value)),
        }
    }
    let (select, count_sql) =
        tables::select(&api.schema, table, &query).map_err(ApiError::BadRequest)?;

    let mut rows_query = sqlx::query_scalar::<_, Json_>(&select.text);
    for bind in &select.binds {
        rows_query = rows_query.bind(bind);
    }
    let rows = rows_query.fetch_all(&api.pool).await?;
    let total = if count {
        let mut count_query = sqlx::query_scalar::<_, i64>(&count_sql.text);
        for bind in &count_sql.binds {
            count_query = count_query.bind(bind);
        }
        Some(count_query.fetch_one(&api.pool).await?)
    } else {
        None
    };
    Ok(Json(json!({
        "rows": rows,
        "count": total,
        "limit": query.limit,
        "offset": query.offset,
    })))
}
