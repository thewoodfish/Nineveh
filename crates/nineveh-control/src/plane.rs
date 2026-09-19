//! The control plane: many projects in one process, each with its pipeline supervised
//! and its state API and change feed served (ADR 0017).

use std::collections::{BTreeMap, BTreeSet};
use std::sync::{Arc, Mutex, PoisonError};
use std::time::{Duration, Instant};

use axum::Router;
use nineveh_api::{Api, Health};
use nineveh_config::{
    Action, Config, Diagnostics, Input, Project, SourceKind, StartVersion, TableKind, column_for,
    parse, record_scope,
};
use nineveh_core::{Address, Network, Value, Version};
use nineveh_decode::{Lockfile, TransactionDecoder};
use nineveh_engine::{ChangeSet, Engine, MemoryState, TableId};
use nineveh_pipeline::{BatchStream, Source};
use nineveh_realtime::Hub;
use nineveh_store::accounts::{self, ApiKey};
use nineveh_store::{Store, StoreError, registry, row_json, shadow_name};

use crate::idle::Demand;
use serde::Serialize;
use sqlx::PgPool;
use tokio::sync::{RwLock, watch};
use tokio::task::JoinHandle;
use tracing::{debug, error, info, warn};

use crate::auth;
use crate::catalog::{Catalog, catalog};
use crate::chain::{Chain, ChainError};
use crate::deliver::Deliveries;
use crate::pin::{PinError, pin, retry};
use crate::runner::{RunError, RunOptions, Runner};
use crate::scaffold::{Draft, ScaffoldError, Start, scaffold};

/// Why a control-plane request failed.
#[derive(Debug, thiserror::Error)]
#[non_exhaustive]
pub enum ControlError {
    /// The config has problems, rendered at their lines in `details`.
    #[error("{message}")]
    Invalid { message: String, details: String },

    #[error("{0}")]
    BadRequest(String),

    #[error("{0}")]
    NotFound(String),

    #[error("{0}")]
    Conflict(String),

    /// No session, or one that's expired: sign in (ADR 0018).
    #[error("{0}")]
    Unauthorized(String),

    #[error(transparent)]
    Chain(#[from] ChainError),

    #[error(transparent)]
    Pin(#[from] PinError),

    #[error(transparent)]
    Store(StoreError),

    #[error(transparent)]
    Scaffold(#[from] ScaffoldError),
}

impl ControlError {
    /// Only reads of the chain or the database can pass on a retry.
    #[must_use]
    pub fn is_retryable(&self) -> bool {
        match self {
            Self::Chain(e) => e.is_retryable(),
            Self::Pin(e) => e.is_retryable(),
            Self::Store(e) => e.is_retryable(),
            Self::Invalid { .. }
            | Self::BadRequest(_)
            | Self::NotFound(_)
            | Self::Conflict(_)
            | Self::Unauthorized(_)
            | Self::Scaffold(_) => false,
        }
    }

    fn invalid(diagnostics: &Diagnostics, text: &str) -> Self {
        let count = diagnostics.as_slice().len();
        Self::Invalid {
            message: format!(
                "the config has {count} problem{}",
                if count == 1 { "" } else { "s" }
            ),
            details: diagnostics.render("nineveh.yaml", text),
        }
    }
}

impl From<StoreError> for ControlError {
    fn from(error: StoreError) -> Self {
        match error {
            StoreError::ProjectExists(_) => Self::Conflict(error.to_string()),
            StoreError::NoProject(_) => Self::NotFound(error.to_string()),
            StoreError::InvalidSchema(_) | StoreError::ReservedSchema(_) => {
                Self::BadRequest(error.to_string())
            }
            other => Self::Store(other),
        }
    }
}

/// A project as the control API reports it.
#[derive(Debug, Clone, Serialize)]
pub struct Summary {
    pub name: String,
    pub network: String,
    /// Whether it should run.
    pub running: bool,
    /// `starting`, `running`, `retrying`, `stopped`, `halted` or `failed`.
    pub state: String,
    /// Why it halted, failed, or can't be loaded.
    pub error: Option<String>,
    pub pipeline: Option<Health>,
    /// Where its state API and change feed are served, relative to the control plane.
    pub api: String,
    pub created_at: String,
    pub updated_at: String,
}

/// A source as the state-table editor sees it: what a rule on it can read.
#[derive(Debug, Clone, Serialize)]
pub struct SourceInfo {
    pub name: String,
    /// `event`, `resource` or `table`.
    pub kind: &'static str,
    /// The Move type it follows.
    pub follows: String,
    /// Whether `<name>.deleted` rules are possible: resources and tables only.
    pub deletes: bool,
    /// What a rule on it reads, with `<name>.deleted`'s fields when it deletes.
    pub fields: Vec<FieldInfo>,
    pub delete_fields: Vec<FieldInfo>,
}

/// How a preview reads the chain: a window of this many versions ending where the
/// project last saw a change, stopping at [`PREVIEW_RECORDS`] or [`PREVIEW_WINDOW`],
/// whichever comes first. A preview answers "is this rule right?" in a few seconds;
/// it isn't building the table.
const PREVIEW_VERSIONS: u64 = 30_000;
const PREVIEW_WINDOW: Duration = Duration::from_secs(12);
const PREVIEW_RECORDS: usize = 2_000;
const PREVIEW_ROWS: usize = 50;

/// What a table's rules would produce, folded over recent transactions without
/// saving anything.
#[derive(Debug, Clone, Serialize)]
pub struct Preview {
    pub table: String,
    /// The rows themselves, as the API would serve them, up to [`PREVIEW_ROWS`].
    pub rows: Vec<serde_json::Value>,
    /// How many rows the fold produced, which can be more than `rows` holds.
    pub row_count: usize,
    /// Records that reached the project, and transactions read.
    pub records: usize,
    pub transactions: usize,
    /// The versions the preview looked over, ending at the chain's tip.
    pub from: String,
    pub to: String,
}

/// A webhook endpoint as Studio shows it: what the config says, plus how delivery is
/// going and the secret to check signatures with (ADR 0020).
#[derive(Debug, Clone, Serialize)]
pub struct WebhookInfo {
    pub name: String,
    pub url: String,
    /// The changes it asks for, as written: `balances.changed`.
    pub on: Vec<String>,
    /// Whether deliveries carry the changed row, not only its key.
    pub rows: bool,
    /// The secret every delivery is signed with.
    pub secret: String,
    /// The last change delivered, as `version.seq`, or `null` before the first.
    pub delivered: Option<String>,
    /// Failed attempts since the last delivery, and what the last one said.
    pub failures: i32,
    pub last_error: Option<String>,
}

/// A saved state table in the shape the editor edits it: its columns and, for a
/// `reduce` table, its rules with their expressions as written.
#[derive(Debug, Clone, Serialize)]
pub struct StateTableInfo {
    pub name: String,
    /// `reduce`, `mirror` or `log`.
    pub kind: &'static str,
    pub columns: Vec<ColumnInfo>,
    pub rules: Vec<RuleInfo>,
}

#[derive(Debug, Clone, Serialize)]
pub struct ColumnInfo {
    pub name: String,
    #[serde(rename = "type")]
    pub ty: &'static str,
    /// The default as written in the config, or empty for none.
    pub default: String,
    pub nullable: bool,
    pub key: bool,
}

#[derive(Debug, Clone, Serialize)]
pub struct RuleInfo {
    /// The source it fires on.
    pub on: String,
    /// Whether it's an `<source>.deleted` rule.
    pub deleted: bool,
    /// The `when` condition as written, or empty for none.
    pub when: String,
    pub keys: Vec<AssignmentInfo>,
    pub sets: Vec<AssignmentInfo>,
    /// Whether the rule deletes the row instead of setting columns.
    pub removes: bool,
}

#[derive(Debug, Clone, Serialize)]
pub struct AssignmentInfo {
    pub column: String,
    pub expression: String,
}

/// One readable field, typed as a column would be (ADR 0008).
#[derive(Debug, Clone, Serialize)]
pub struct FieldInfo {
    pub name: String,
    /// The column type it fits: `address`, `u64`, `string`, `json`…
    #[serde(rename = "type")]
    pub ty: &'static str,
    pub nullable: bool,
}

/// A project with its config, for `GET projects/{name}`.
#[derive(Debug, Clone, Serialize)]
pub struct Detail {
    #[serde(flatten)]
    pub summary: Summary,
    pub config: String,
}

/// A scaffold request: [`Draft`] with the start as the control API takes it.
#[derive(Debug, Clone, serde::Deserialize)]
pub struct ScaffoldRequest {
    pub name: String,
    pub network: Network,
    /// `"auto"` (the default), `"now"`, or a version.
    #[serde(default)]
    pub start: StartRequest,
    pub picks: Vec<String>,
}

#[derive(Debug, Clone, Default, serde::Deserialize)]
#[serde(untagged)]
pub enum StartRequest {
    #[default]
    #[serde(skip_deserializing)]
    Auto,
    Version(u64),
    Word(String),
}

/// Who a request comes from (ADR 0018).
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Caller {
    /// Local mode: no sign-in, every project reachable.
    Local,
    /// A signed-in account, which reaches only its own projects.
    Account(i64),
}

impl Caller {
    /// Whether this caller may see and change a project owned by `owner`.
    #[must_use]
    pub fn may(self, owner: Option<i64>) -> bool {
        match self {
            Self::Local => true,
            Self::Account(id) => owner == Some(id),
        }
    }

    /// The owner a project this caller creates gets.
    #[must_use]
    pub fn owner(self) -> Option<i64> {
        match self {
            Self::Local => None,
            Self::Account(id) => Some(id),
        }
    }
}

/// A project resolved and ready to run.
struct Loaded {
    project: Arc<Project>,
    lock: Arc<Lockfile>,
    start: Version,
}

/// How a project's pipeline task ended, if it has.
#[derive(Debug, Clone)]
enum Ended {
    Stopped,
    Halted(String),
    Failed(String),
}

struct Run {
    stop: watch::Sender<bool>,
    task: JoinHandle<()>,
    ended: Arc<Mutex<Option<Ended>>>,
}

struct Entry {
    owner_id: Option<i64>,
    network: String,
    config: String,
    running: bool,
    created_at: String,
    updated_at: String,
    /// The project, unless its stored config no longer loads.
    loaded: Result<Arc<Loaded>, String>,
    router: Router,
    health: watch::Sender<Option<Health>>,
    run: Option<Run>,
    /// The webhook senders of this project, running while it's served (ADR 0020):
    /// a stopped pipeline still delivers what it already committed.
    deliveries: Option<Deliveries>,
}

/// Every project this process manages.
pub struct ControlPlane<C> {
    chain: Arc<C>,
    pool: PgPool,
    options: RunOptions,
    /// One listener wakes every project's change feed.
    hub: Arc<Hub>,
    projects: RwLock<BTreeMap<String, Entry>>,
    /// Changes to projects happen one at a time.
    changes: tokio::sync::Mutex<()>,
    /// When each project's read was last written down. The question idleness asks is
    /// whether anyone read it today, so a write per request would buy nothing; this
    /// keeps the API's hot path off the database between them.
    read_at: Mutex<BTreeMap<String, Instant>>,
}

impl<C> std::fmt::Debug for ControlPlane<C> {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("ControlPlane")
            .field("options", &self.options)
            .finish_non_exhaustive()
    }
}

impl<C: Chain> ControlPlane<C> {
    /// How often a project's read is written down. Idleness is measured in hours, so
    /// a timestamp a minute out of date answers the question exactly as well.
    const READ_INTERVAL: Duration = Duration::from_secs(60);

    /// Note that someone read `project`, at most once a minute.
    ///
    /// Called from the API's hot path, so it does nothing but compare two instants
    /// unless the interval has passed, and the write it then does is spawned: a read
    /// must never wait on bookkeeping about itself.
    pub fn note_read(self: &Arc<Self>, project: &str) {
        {
            let now = Instant::now();
            let Ok(mut seen) = self.read_at.lock() else {
                return; // A poisoned lock costs us a timestamp, not a request.
            };
            match seen.get(project) {
                Some(at) if now.duration_since(*at) < Self::READ_INTERVAL => return,
                _ => seen.insert(project.to_owned(), now),
            };
        }
        let pool = self.pool.clone();
        let project = project.to_owned();
        tokio::spawn(async move {
            if let Err(error) = nineveh_store::reads::touch(&pool, &project).await {
                debug!(%error, %project, "couldn't note the read");
            }
        });
    }

    /// Load every registered project, serve it, and run those that should run.
    ///
    /// # Errors
    ///
    /// If the database can't be read.
    pub async fn start(
        chain: Arc<C>,
        pool: PgPool,
        options: RunOptions,
    ) -> Result<Arc<Self>, ControlError> {
        let hub = Hub::start(pool.clone())
            .await
            .map_err(|e| ControlError::Store(e.into()))?;
        let plane = Arc::new(Self {
            chain,
            pool,
            options,
            hub,
            projects: RwLock::new(BTreeMap::new()),
            changes: tokio::sync::Mutex::new(()),
            read_at: Mutex::new(BTreeMap::new()),
        });
        for record in registry::list(&plane.pool).await? {
            let loaded = load(&record.config, &record.lock).map(Arc::new);
            if let Err(e) = &loaded {
                warn!(project = %record.name, error = %e, "can't load its stored config");
            }
            let name = record.name.clone();
            let mut entry = plane.entry(record, loaded);
            if entry.running {
                plane.spawn(&name, &mut entry);
            }
            info!(project = %name, running = entry.running, "loaded");
            plane.projects.write().await.insert(name, entry);
        }
        Ok(plane)
    }

    /// The database the plane keeps its registry, accounts and state in.
    #[must_use]
    pub fn pool(&self) -> &PgPool {
        &self.pool
    }

    /// Stop every pipeline after its current commit.
    pub async fn shutdown(&self) {
        let (runs, deliveries): (Vec<Option<Run>>, Vec<Option<Deliveries>>) = {
            let mut projects = self.projects.write().await;
            projects
                .values_mut()
                .map(|entry| (entry.run.take(), entry.deliveries.take()))
                .collect()
        };
        for run in runs.into_iter().flatten() {
            stop(run).await;
        }
        for sender in deliveries.into_iter().flatten() {
            sender.stop().await;
        }
    }

    /// The catalog of `address` on `network`.
    ///
    /// # Errors
    ///
    /// [`ControlError::NotFound`] if no modules are published there, or if the chain
    /// can't be read.
    pub async fn inspect(&self, network: Network, address: &str) -> Result<Catalog, ControlError> {
        let address: Address = address
            .trim()
            .parse()
            .map_err(|_| ControlError::BadRequest(format!("`{address}` isn't an address")))?;
        let modules = retry(|| self.chain.modules(network, address)).await?;
        if modules.is_empty() {
            return Err(ControlError::NotFound(format!(
                "no modules are published at {} on {network}",
                address.to_standard_string()
            )));
        }
        Ok(catalog(address, &modules))
    }

    /// A config for the picks in `request`.
    ///
    /// # Errors
    ///
    /// If the name is taken or invalid, a pick can't be followed, or the chain can't be
    /// read.
    pub async fn scaffold(&self, request: ScaffoldRequest) -> Result<String, ControlError> {
        if self.projects.read().await.contains_key(&request.name) {
            return Err(ControlError::Conflict(format!(
                "a project named `{}` already exists",
                request.name
            )));
        }
        let addresses: BTreeSet<&str> = request
            .picks
            .iter()
            .filter_map(|id| id.split("::").next())
            .collect();
        let mut catalogs = Vec::new();
        for address in addresses {
            catalogs.push(self.inspect(request.network, address).await?);
        }
        let start = match &request.start {
            StartRequest::Auto => Start::Auto,
            StartRequest::Version(v) => Start::Version(*v),
            StartRequest::Word(w) if w == "auto" => Start::Auto,
            StartRequest::Word(w) if w == "now" => {
                let tip = retry(|| self.chain.tip(request.network)).await?;
                Start::Version(tip.get())
            }
            StartRequest::Word(w) => {
                return Err(ControlError::BadRequest(format!(
                    "`start` must be \"auto\", \"now\" or a version, not {w:?}"
                )));
            }
        };
        let draft = Draft {
            name: request.name,
            network: request.network,
            start,
            picks: request.picks,
        };
        Ok(scaffold(&draft, &catalogs)?)
    }

    /// Create a project from a config: pin its layouts, save it, and start it.
    ///
    /// # Errors
    ///
    /// [`ControlError::Invalid`] if the config has problems, [`ControlError::Conflict`]
    /// if the name is taken, or if the chain or the database fails.
    pub async fn create(&self, caller: Caller, text: &str) -> Result<Detail, ControlError> {
        let _changing = self.changes.lock().await;
        let config = parse(text).map_err(|d| ControlError::invalid(&d, text))?;
        let name = config.name.name.clone();
        shadow_name(&name)?;
        if self.projects.read().await.contains_key(&name) {
            return Err(ControlError::Conflict(format!(
                "a project named `{name}` already exists"
            )));
        }
        let (loaded, lock_text) = self.prepare(&config, text).await?;
        registry::insert(
            &self.pool,
            &name,
            config.network.as_str(),
            text,
            &lock_text,
            caller.owner(),
        )
        .await?;
        let record = self.record(&name).await?;
        let mut entry = self.entry(record, Ok(Arc::new(loaded)));
        self.spawn(&name, &mut entry);
        info!(project = %name, "created");
        let detail = detail(&name, &entry);
        self.projects.write().await.insert(name, entry);
        Ok(detail)
    }

    /// Replace a project's config. Its pipeline restarts, rebuilding into a shadow
    /// schema if the change alters what's built (ADR 0016).
    ///
    /// # Errors
    ///
    /// As [`ControlPlane::create`], and [`ControlError::NotFound`] if there's no such
    /// project.
    pub async fn update(
        &self,
        caller: Caller,
        name: &str,
        text: &str,
    ) -> Result<Detail, ControlError> {
        let _changing = self.changes.lock().await;
        let config = parse(text).map_err(|d| ControlError::invalid(&d, text))?;
        if config.name.name != name {
            return Err(ControlError::BadRequest(format!(
                "the config names the project `{}`; renaming isn't supported, so keep `name: {name}`",
                config.name.name
            )));
        }
        let running = self.get_entry(caller, name, |e| e.running).await?;
        let (loaded, lock_text) = self.prepare(&config, text).await?;
        let old = self.take_run(name).await;
        if let Some(run) = old {
            stop(run).await;
        }
        self.stop_deliveries(name).await;
        registry::update(&self.pool, name, text, &lock_text).await?;
        let record = self.record(name).await?;
        let mut entry = self.entry(record, Ok(Arc::new(loaded)));
        if running {
            self.spawn(name, &mut entry);
        }
        info!(project = %name, "config updated");
        let detail = detail(name, &entry);
        self.projects.write().await.insert(name.to_owned(), entry);
        Ok(detail)
    }

    /// Start or stop a project's pipeline. Its state stays served either way.
    ///
    /// # Errors
    ///
    /// If there's no such project, or the database fails.
    pub async fn set_running(
        &self,
        caller: Caller,
        name: &str,
        running: bool,
    ) -> Result<Summary, ControlError> {
        let _changing = self.changes.lock().await;
        self.get_entry(caller, name, |_| ()).await?;
        registry::set_running(&self.pool, name, running).await?;
        if running {
            let mut projects = self.projects.write().await;
            let entry = projects.get_mut(name).ok_or_else(|| missing(name))?;
            entry.running = true;
            let finished = entry.run.as_ref().is_none_or(|r| r.task.is_finished());
            if finished {
                entry.run = None;
                self.spawn(name, entry);
            }
        } else {
            if let Some(run) = self.take_run(name).await {
                stop(run).await;
            }
            if let Some(entry) = self.projects.write().await.get_mut(name) {
                entry.running = false;
            }
        }
        self.get_entry(caller, name, |e| summary(name, e)).await
    }

    /// Stop a project, drop its state, and forget it.
    ///
    /// # Errors
    ///
    /// If there's no such project, or the database fails.
    pub async fn delete(&self, caller: Caller, name: &str) -> Result<(), ControlError> {
        let _changing = self.changes.lock().await;
        self.get_entry(caller, name, |_| ()).await?;
        if let Some(run) = self.take_run(name).await {
            stop(run).await;
        }
        self.stop_deliveries(name).await;
        if let Err(error) = nineveh_store::webhooks::forget_others(&self.pool, name, &[]).await {
            warn!(%error, project = %name, "couldn't forget the project's webhooks");
        }
        // The record log and the read history are keyed by project, not by schema, so
        // dropping the schema doesn't take them (ADR 0022).
        if let Err(error) = nineveh_store::records::forget(&self.pool, name).await {
            warn!(%error, project = %name, "couldn't forget the project's records");
        }
        if let Err(error) = nineveh_store::reads::forget(&self.pool, name).await {
            warn!(%error, project = %name, "couldn't forget the project's read history");
        }
        registry::delete(&self.pool, name).await?;
        self.projects.write().await.remove(name);
        if let Ok(mut seen) = self.read_at.lock() {
            seen.remove(name);
        }
        info!(project = %name, "deleted");
        Ok(())
    }

    /// What is waiting on `project`'s folded state, and therefore whether its fold is
    /// worth running (ADR 0023).
    ///
    /// The two declarations come from what the project *is* — its config's endpoints,
    /// and who is listening to its feed — and only the last two are looked up, so a
    /// project with a webhook endpoint never depends on a query succeeding.
    ///
    /// # Errors
    ///
    /// If the database fails while counting the backlog or reading the timestamp.
    pub async fn demand(&self, name: &str) -> Result<Option<Demand>, StoreError> {
        let (webhooks, folded) = {
            let projects = self.projects.read().await;
            let Some(entry) = projects.get(name) else {
                return Ok(None);
            };
            let webhooks = entry
                .loaded
                .as_ref()
                .is_ok_and(|l| !l.project.config().webhooks.is_empty());
            // The health snapshot carries the cursor as a decimal string, since it is
            // what the API serves.
            let folded = entry.health.borrow().as_ref().and_then(|h| {
                h.cursor
                    .as_deref()
                    .and_then(|c| c.parse::<u64>().ok())
                    .map(Version::new)
            });
            (webhooks, folded)
        };
        Ok(Some(Demand {
            webhooks,
            listeners: self.hub.feed(name).listeners(),
            read_ago: nineveh_store::reads::seconds_since_read(&self.pool, name).await?,
            backlog: nineveh_store::records::pending(&self.pool, name, folded).await?,
        }))
    }

    /// Every project `caller` may see, by name.
    pub async fn list(&self, caller: Caller) -> Vec<Summary> {
        self.projects
            .read()
            .await
            .iter()
            .filter(|(_, entry)| caller.may(entry.owner_id))
            .map(|(name, entry)| summary(name, entry))
            .collect()
    }

    /// One project, with its config.
    ///
    /// # Errors
    ///
    /// If there's no such project.
    pub async fn get(&self, caller: Caller, name: &str) -> Result<Detail, ControlError> {
        self.get_entry(caller, name, |e| detail(name, e)).await
    }

    /// A project's owner: `Some(None)` for one made in local mode, `None` if there's
    /// no such project.
    pub async fn owner(&self, name: &str) -> Option<Option<i64>> {
        self.projects.read().await.get(name).map(|e| e.owner_id)
    }

    /// `caller`'s project `name`'s live API keys.
    ///
    /// # Errors
    ///
    /// If there's no such project for `caller`, or the database fails.
    pub async fn keys(&self, caller: Caller, name: &str) -> Result<Vec<ApiKey>, ControlError> {
        self.get_entry(caller, name, |_| ()).await?;
        Ok(accounts::api_keys(&self.pool, name).await?)
    }

    /// A new API key for `caller`'s project `name`: the key, which is shown only now,
    /// and its record.
    ///
    /// # Errors
    ///
    /// If there's no such project for `caller`, the label is empty, or the database
    /// fails.
    pub async fn create_key(
        &self,
        caller: Caller,
        name: &str,
        label: &str,
    ) -> Result<(String, ApiKey), ControlError> {
        self.get_entry(caller, name, |_| ()).await?;
        let label = label.trim();
        if label.is_empty() || label.chars().count() > 100 {
            return Err(ControlError::BadRequest(
                "a key needs a label of 1 to 100 characters, to tell it apart".into(),
            ));
        }
        let (key, hash) = auth::new_token(auth::KEY_PREFIX)
            .map_err(|e| ControlError::BadRequest(e.to_string()))?;
        let prefix: String = key.chars().take(auth::KEY_PREFIX.len() + 8).collect();
        let record = accounts::create_api_key(&self.pool, name, label, &prefix, &hash).await?;
        info!(project = %name, key = record.id, "API key created");
        Ok((key, record))
    }

    /// Revoke one of `caller`'s project `name`'s keys.
    ///
    /// # Errors
    ///
    /// If there's no such project or live key, or the database fails.
    pub async fn revoke_key(
        &self,
        caller: Caller,
        name: &str,
        id: i64,
    ) -> Result<(), ControlError> {
        self.get_entry(caller, name, |_| ()).await?;
        if !accounts::revoke_api_key(&self.pool, name, id).await? {
            return Err(ControlError::NotFound(format!(
                "no live key {id} on `{name}`"
            )));
        }
        info!(project = %name, key = id, "API key revoked");
        Ok(())
    }

    /// What each of a project's sources offers a rule, for the state-table editor.
    ///
    /// # Errors
    ///
    /// If there's no such project for `caller`, or its config no longer loads.
    pub async fn sources(
        &self,
        caller: Caller,
        name: &str,
    ) -> Result<Vec<SourceInfo>, ControlError> {
        let loaded = self
            .get_entry(caller, name, |e| e.loaded.clone())
            .await?
            .map_err(ControlError::BadRequest)?;
        let project = &loaded.project;
        let describe = |input: &Input, deleted: bool| {
            record_scope(&loaded.lock, input, deleted)
                .fields
                .iter()
                .map(|(field, ty)| {
                    let (column, nullable) = column_for(ty);
                    FieldInfo {
                        name: field.as_str().to_owned(),
                        ty: column.as_str(),
                        nullable,
                    }
                })
                .collect::<Vec<_>>()
        };
        Ok(project
            .config()
            .sources
            .iter()
            .filter_map(|source| {
                let input = project
                    .source_id(source.name.as_str())
                    .and_then(|id| project.input(id))?;
                let deletes = source.kind.has_deletes();
                Some(SourceInfo {
                    name: source.name.name.clone(),
                    kind: source.kind.keyword(),
                    follows: match &source.kind {
                        SourceKind::Event(tag) | SourceKind::Resource(tag) => tag.to_string(),
                        SourceKind::Table { parent, field } => format!("{parent}.{field}"),
                    },
                    deletes,
                    fields: describe(input, false),
                    delete_fields: if deletes {
                        describe(input, true)
                    } else {
                        Vec::new()
                    },
                })
            })
            .collect())
    }

    /// Check a config against the project's pinned layouts, without saving it: what
    /// the state-table editor shows while you type. New sources need saving, which
    /// pins their layouts.
    ///
    /// # Errors
    ///
    /// [`ControlError::Invalid`] with located diagnostics if it doesn't hold up.
    pub async fn check(&self, caller: Caller, name: &str, text: &str) -> Result<(), ControlError> {
        let loaded = self
            .get_entry(caller, name, |e| e.loaded.clone())
            .await?
            .map_err(ControlError::BadRequest)?;
        let config = parse(text).map_err(|d| ControlError::invalid(&d, text))?;
        if config.name.name != name {
            return Err(ControlError::BadRequest(format!(
                "the config names the project `{}`; keep `name: {name}`",
                config.name.name
            )));
        }
        config
            .resolve(&loaded.lock)
            .map(|_| ())
            .map_err(|d| ControlError::invalid(&d, text))
    }

    /// Fold `text`'s table `table` over recent transactions and return the rows it
    /// would produce, without saving anything or touching the project's state.
    ///
    /// The fold runs in memory from empty, over a window ending at the chain's tip, so
    /// counters count what happened in the window rather than all of history. Handles
    /// the project has already learned are seeded in, so table sources are attributed
    /// (ADR 0012).
    ///
    /// # Errors
    ///
    /// [`ControlError::Invalid`] if the config doesn't hold up, or
    /// [`ControlError::BadRequest`] if a rule fails on real data: an overflow names
    /// the version and rule that would halt the project.
    pub async fn preview(
        &self,
        caller: Caller,
        name: &str,
        text: &str,
        table: &str,
    ) -> Result<Preview, ControlError> {
        let loaded = self
            .get_entry(caller, name, |e| e.loaded.clone())
            .await?
            .map_err(ControlError::BadRequest)?;
        let config = parse(text).map_err(|d| ControlError::invalid(&d, text))?;
        if config.name.name != name {
            return Err(ControlError::BadRequest(format!(
                "the config names the project `{}`; keep `name: {name}`",
                config.name.name
            )));
        }
        let candidate = config
            .resolve(&loaded.lock)
            .map_err(|d| ControlError::invalid(&d, text))?;
        let index = candidate
            .config()
            .state
            .iter()
            .position(|t| t.name.name == table)
            .ok_or_else(|| {
                ControlError::BadRequest(format!("the config has no state table `{table}`"))
            })?;
        let network = candidate.config().network;
        let tip = self.chain.tip(network).await?;
        let decoder = TransactionDecoder::new(&loaded.lock, candidate.selection());
        let (seed, from, to) = self.preview_seed(name, &loaded, tip).await;

        // Read the window and keep the records in it. The fold applies them in version
        // order afterwards, which is the only order it accepts.
        let deadline = Instant::now() + PREVIEW_WINDOW;
        let mut found: Vec<nineveh_decode::DecodedTransaction> = Vec::new();
        let (mut records, mut transactions) = (0, 0);
        let source = self.chain.source(network, from, &candidate);
        let mut stream = source
            .open(from, Some(to))
            .await
            .map_err(|e| ControlError::BadRequest(e.to_string()))?;
        while records < PREVIEW_RECORDS {
            let Ok(batch) = tokio::time::timeout_at(deadline.into(), stream.next()).await else {
                break;
            };
            let Some(batch) = batch.map_err(|e| ControlError::BadRequest(e.to_string()))? else {
                break;
            };
            let last = batch.transactions.last().map(|t| t.version);
            for transaction in &batch.transactions {
                let one = decoder
                    .decode(transaction)
                    .map_err(|e| ControlError::BadRequest(e.to_string()))?;
                records += one.records.len();
                if !one.records.is_empty() {
                    found.push(one);
                }
            }
            transactions += batch.transactions.len();
            if last.is_some_and(|v| v >= to.get()) {
                break;
            }
        }
        found.sort_by_key(|decoded| decoded.version);

        let mut state = MemoryState::new();
        state.apply(&seed);
        let changes = Engine::new(&candidate)
            .fold(&state, &found)
            .map_err(|e| ControlError::BadRequest(e.to_string()))?;
        state.apply(&changes);

        let schema = candidate
            .schemas()
            .get(index)
            .ok_or_else(|| ControlError::BadRequest(format!("no schema for `{table}`")))?;
        let id = TableId::State(u32::try_from(index).unwrap_or(u32::MAX));
        let all: Vec<_> = state.rows(id).map(|(_, row)| row.clone()).collect();
        let rows = all
            .iter()
            .take(PREVIEW_ROWS)
            .map(|row| row_json(table, schema, row))
            .collect::<Result<Vec<_>, _>>()?;
        Ok(Preview {
            table: table.to_owned(),
            rows,
            row_count: all.len(),
            records,
            transactions,
            from: from.to_string(),
            to: to.to_string(),
        })
    }

    /// What a preview starts from: the project's own state, and the versions to read.
    ///
    /// The window ends at the newest version that changed one of the project's rows —
    /// where the contract was last doing something — so a contract last used hours ago
    /// still previews. A project with no rows yet reads the chain's tip instead. The
    /// window is capped at [`PREVIEW_VERSIONS`], because reading it has to finish
    /// while someone waits.
    ///
    /// The handles the project has learned are seeded in, so table sources are
    /// attributed (ADR 0012) rather than silently ignored. Both are best effort: a
    /// preview that can't read the project's state still reads the chain.
    async fn preview_seed(
        &self,
        name: &str,
        loaded: &Loaded,
        tip: Version,
    ) -> (ChangeSet, Version, Version) {
        let mut seed = ChangeSet::default();
        let mut newest = tip;
        if let Ok(store) = Store::open(self.pool.clone(), name, &loaded.project, &loaded.lock).await
        {
            if let Ok(handles) = store.scan(TableId::Handles).await {
                for (key, row) in handles {
                    seed.writes
                        .insert((TableId::Handles, key), Some(row.unwrap_or_default()));
                }
            }
            if let Ok(Some(latest)) = store.latest_change().await {
                newest = latest;
            }
        }
        let from = newest
            .get()
            .saturating_sub(PREVIEW_VERSIONS)
            .max(loaded.start.get());
        (seed, Version::new(from), newest)
    }

    /// A saved state table, in the shape the editor edits: what Studio loads to open
    /// an existing table instead of only creating new ones.
    ///
    /// # Errors
    ///
    /// [`ControlError::NotFound`] if the project or the table isn't there.
    pub async fn state_table(
        &self,
        caller: Caller,
        name: &str,
        table: &str,
    ) -> Result<StateTableInfo, ControlError> {
        let loaded = self
            .get_entry(caller, name, |e| e.loaded.clone())
            .await?
            .map_err(ControlError::BadRequest)?;
        let state =
            loaded.project.config().table(table).ok_or_else(|| {
                ControlError::NotFound(format!("`{name}` has no table `{table}`"))
            })?;
        let (kind, key, columns, rules) = match &state.kind {
            TableKind::Reduce {
                key,
                columns,
                rules,
            } => ("reduce", key, columns, rules),
            TableKind::Mirror { source } => {
                return Err(ControlError::BadRequest(format!(
                    "`{table}` mirrors `{source}`, so it has no rules to edit"
                )));
            }
            TableKind::Log { source } => {
                return Err(ControlError::BadRequest(format!(
                    "`{table}` logs `{source}`, so it has no rules to edit"
                )));
            }
        };
        Ok(StateTableInfo {
            name: state.name.name.clone(),
            kind,
            columns: columns
                .iter()
                .map(|c| ColumnInfo {
                    name: c.name.name.clone(),
                    ty: c.ty.as_str(),
                    default: c.default.as_ref().map(literal).unwrap_or_default(),
                    nullable: c.nullable,
                    key: key.iter().any(|k| k.name == c.name.name),
                })
                .collect(),
            rules: rules.iter().map(rule_info).collect(),
        })
    }

    /// A project's webhook endpoints: the config's, with each one's secret and how
    /// its deliveries are going (ADR 0020).
    ///
    /// # Errors
    ///
    /// If the project isn't there, or the database fails.
    pub async fn webhooks(
        &self,
        caller: Caller,
        name: &str,
    ) -> Result<Vec<WebhookInfo>, ControlError> {
        let loaded = self
            .get_entry(caller, name, |e| e.loaded.clone())
            .await?
            .map_err(ControlError::BadRequest)?;
        let stored = nineveh_store::webhooks::list(&self.pool, name).await?;
        Ok(loaded
            .project
            .config()
            .webhooks
            .iter()
            .map(|hook| {
                let known = stored.iter().find(|e| e.name == hook.name.name);
                WebhookInfo {
                    name: hook.name.name.clone(),
                    url: hook.url.clone(),
                    on: hook
                        .on
                        .iter()
                        .map(|s| format!("{}.{}", s.table, s.change.as_str()))
                        .collect(),
                    rows: hook.rows,
                    // A sender writes the row on its first look, so an endpoint saved
                    // a moment ago may not have one yet.
                    secret: known.map(|e| e.secret.clone()).unwrap_or_default(),
                    delivered: known
                        .and_then(|e| e.cursor)
                        .map(|(version, seq)| format!("{version}.{seq}")),
                    failures: known.map_or(0, |e| e.failures),
                    last_error: known.and_then(|e| e.last_error.clone()),
                }
            })
            .collect())
    }

    /// Give a webhook endpoint a new secret. Deliveries signed with the old one stop
    /// checking out, so a receiver takes the new secret first.
    ///
    /// # Errors
    ///
    /// If the project or the endpoint isn't there, or the database fails.
    pub async fn rotate_webhook(
        &self,
        caller: Caller,
        name: &str,
        endpoint: &str,
    ) -> Result<String, ControlError> {
        let loaded = self
            .get_entry(caller, name, |e| e.loaded.clone())
            .await?
            .map_err(ControlError::BadRequest)?;
        if !loaded
            .project
            .config()
            .webhooks
            .iter()
            .any(|h| h.name.name == endpoint)
        {
            return Err(ControlError::NotFound(format!(
                "`{name}` has no webhook `{endpoint}`"
            )));
        }
        Ok(nineveh_store::webhooks::rotate(&self.pool, name, endpoint).await?)
    }

    /// The router serving a project's state API and change feed.
    pub async fn router(&self, name: &str) -> Option<Router> {
        self.projects
            .read()
            .await
            .get(name)
            .map(|e| e.router.clone())
    }

    /// Pin `config`'s layouts and resolve it: everything short of saving it.
    async fn prepare(&self, config: &Config, text: &str) -> Result<(Loaded, String), ControlError> {
        let lock = pin(&*self.chain, config).await?;
        let lock_text = lock
            .to_json()
            .map_err(|e| ControlError::BadRequest(e.to_string()))?;
        let loaded = load(text, &lock_text).map_err(|message| ControlError::Invalid {
            details: message.clone(),
            message: "the config doesn't resolve against the chain's layouts".into(),
        })?;
        Ok((loaded, lock_text))
    }

    async fn record(&self, name: &str) -> Result<registry::Registered, ControlError> {
        registry::list(&self.pool)
            .await?
            .into_iter()
            .find(|r| r.name == name)
            .ok_or_else(|| missing(name))
    }

    /// `f` of `caller`'s project `name`. Someone else's project is missing, as if it
    /// didn't exist.
    async fn get_entry<T>(
        &self,
        caller: Caller,
        name: &str,
        f: impl FnOnce(&Entry) -> T,
    ) -> Result<T, ControlError> {
        self.projects
            .read()
            .await
            .get(name)
            .filter(|e| caller.may(e.owner_id))
            .map(f)
            .ok_or_else(|| missing(name))
    }

    async fn take_run(&self, name: &str) -> Option<Run> {
        self.projects
            .write()
            .await
            .get_mut(name)
            .and_then(|e| e.run.take())
    }

    /// Stop this project's webhook senders, for a config change or a delete.
    async fn stop_deliveries(&self, name: &str) {
        let deliveries = self
            .projects
            .write()
            .await
            .get_mut(name)
            .and_then(|e| e.deliveries.take());
        if let Some(deliveries) = deliveries {
            deliveries.stop().await;
        }
    }

    /// A served project: its API and change feed over the schema of its name.
    fn entry(&self, record: registry::Registered, loaded: Result<Arc<Loaded>, String>) -> Entry {
        let name = record.name.as_str();
        let (health, _) = watch::channel(None);
        let mut deliveries = None;
        let router = match &loaded {
            Ok(loaded) => {
                let api = Arc::new(Api::new(
                    self.pool.clone(),
                    name,
                    &loaded.project,
                    health.subscribe(),
                ));
                let feed = self.hub.feed(name);
                deliveries = Some(Deliveries::start(
                    &self.pool,
                    name,
                    &loaded.project.config().webhooks,
                    Some(&feed.wake()),
                ));
                nineveh_api::router(api).merge(nineveh_realtime::router(feed))
            }
            Err(_) => Router::new(),
        };
        Entry {
            owner_id: record.owner_id,
            network: record.network,
            config: record.config,
            running: record.running,
            created_at: record.created_at,
            updated_at: record.updated_at,
            loaded,
            router,
            health,
            run: None,
            deliveries,
        }
    }

    /// Run `entry`'s pipeline on a task of its own, until it's stopped or fails in a
    /// way retrying can't fix.
    fn spawn(&self, name: &str, entry: &mut Entry) {
        let Ok(loaded) = &entry.loaded else { return };
        let (stop, stopped) = watch::channel(false);
        let ended = Arc::new(Mutex::new(None));
        let task = tokio::spawn(supervise(
            Arc::clone(&self.chain),
            Arc::clone(loaded),
            name.to_owned(),
            self.pool.clone(),
            self.options.clone(),
            entry.health.clone(),
            stopped,
            Arc::clone(&ended),
        ));
        entry.run = Some(Run { stop, task, ended });
    }
}

/// Run a project, retrying what can pass, until `stopped` or a failure that can't.
#[allow(clippy::too_many_arguments, reason = "everything a run owns")]
async fn supervise<C: Chain>(
    chain: Arc<C>,
    loaded: Arc<Loaded>,
    name: String,
    pool: PgPool,
    options: RunOptions,
    health: watch::Sender<Option<Health>>,
    mut stopped: watch::Receiver<bool>,
    ended: Arc<Mutex<Option<Ended>>>,
) {
    let mut delay = Duration::from_secs(1);
    let outcome = loop {
        let attempt = async {
            let runner = Runner::new(
                Arc::clone(&chain),
                Arc::clone(&loaded.project),
                Arc::clone(&loaded.lock),
                loaded.start,
                name.clone(),
                pool.clone(),
                options.clone(),
                health.clone(),
            )
            .await?;
            runner.run(false, stopped.clone()).await
        };
        match attempt.await {
            Ok(()) => break Ended::Stopped,
            Err(e) if e.is_retryable() && !*stopped.borrow() => {
                warn!(project = %name, error = %e, ?delay, "run failed; retrying");
                health.send_modify(|h| {
                    let h = h.get_or_insert_with(Health::default);
                    h.phase = "retrying".into();
                    h.last_error = Some(e.to_string());
                });
                tokio::select! {
                    () = tokio::time::sleep(delay) => {}
                    _ = stopped.wait_for(|s| *s) => break Ended::Stopped,
                }
                delay = delay.saturating_mul(2).min(Duration::from_secs(60));
            }
            Err(e) => {
                error!(project = %name, error = %e, "run ended");
                let phase = if matches!(e, RunError::Halted { .. }) {
                    "halted"
                } else {
                    "failed"
                };
                health.send_modify(|h| {
                    let h = h.get_or_insert_with(Health::default);
                    h.phase = phase.into();
                    h.last_error = Some(e.to_string());
                });
                break match e {
                    RunError::Halted { .. } => Ended::Halted(e.to_string()),
                    other => Ended::Failed(other.to_string()),
                };
            }
        }
    };
    *ended.lock().unwrap_or_else(PoisonError::into_inner) = Some(outcome);
}

/// Stop a run after its current commit, and wait for it.
async fn stop(run: Run) {
    let _ = run.stop.send(true);
    if let Err(e) = run.task.await {
        error!(error = %e, "a pipeline task panicked");
    }
}

/// A rule as the editor holds it: expressions as their config text.
fn rule_info(rule: &nineveh_config::Rule) -> RuleInfo {
    let assignments = |pairs: &[(nineveh_config::Named, nineveh_config::Expr)]| {
        pairs
            .iter()
            .map(|(name, expr)| AssignmentInfo {
                column: name.name.clone(),
                expression: expr.text.clone(),
            })
            .collect()
    };
    RuleInfo {
        on: rule.on.source.name.clone(),
        deleted: rule.on.deleted,
        when: rule
            .when
            .as_ref()
            .map(|w| w.text.clone())
            .unwrap_or_default(),
        keys: assignments(&rule.key),
        sets: match &rule.action {
            Action::Set(set) => assignments(set),
            Action::Delete => Vec::new(),
        },
        removes: matches!(rule.action, Action::Delete),
    }
}

/// A column default as it would be written in the config: a bare number or `true`,
/// and anything else quoted.
fn literal(value: &Value) -> String {
    match value {
        Value::Bool(b) => b.to_string(),
        Value::U8(n) => n.to_string(),
        Value::U16(n) => n.to_string(),
        Value::U32(n) => n.to_string(),
        Value::U64(n) => n.to_string(),
        Value::U128(n) => n.to_string(),
        Value::U256(n) => n.to_string(),
        Value::I8(n) => n.to_string(),
        Value::I16(n) => n.to_string(),
        Value::I32(n) => n.to_string(),
        Value::I64(n) => n.to_string(),
        Value::I128(n) => n.to_string(),
        Value::I256(n) => n.to_string(),
        Value::Address(a) => format!("\"{}\"", a.to_standard_string()),
        Value::String(s) => format!("{s:?}"),
        other => format!("{}", serde_json::to_value(other).unwrap_or_default()),
    }
}

/// Resolve a stored config against its stored lock.
fn load(text: &str, lock_text: &str) -> Result<Loaded, String> {
    let config = parse(text).map_err(|d| d.render("nineveh.yaml", text))?;
    let lock = Lockfile::from_json(lock_text).map_err(|e| e.to_string())?;
    let start = match config.start_version {
        StartVersion::Version(v) => Version::new(v),
        StartVersion::Auto => lock
            .start_version()
            .ok_or("`start_version: auto`, but the lock pins no start; save the config again")?,
    };
    let project = config
        .resolve(&lock)
        .map_err(|d| d.render("nineveh.yaml", text))?;
    Ok(Loaded {
        project: Arc::new(project),
        lock: Arc::new(lock),
        start,
    })
}

fn missing(name: &str) -> ControlError {
    ControlError::NotFound(format!("no project named `{name}`"))
}

fn summary(name: &str, entry: &Entry) -> Summary {
    let pipeline = entry.health.borrow().clone();
    let ended = entry.run.as_ref().and_then(|r| {
        r.ended
            .lock()
            .unwrap_or_else(PoisonError::into_inner)
            .clone()
    });
    let (state, error) = match (&entry.loaded, &entry.run, ended) {
        (Err(e), _, _) => ("failed".to_owned(), Some(e.clone())),
        (_, _, Some(Ended::Halted(e))) => ("halted".to_owned(), Some(e)),
        (_, _, Some(Ended::Failed(e))) => ("failed".to_owned(), Some(e)),
        (_, None, _) | (_, _, Some(Ended::Stopped)) => ("stopped".to_owned(), None),
        (_, Some(_), None) => (
            pipeline
                .as_ref()
                .map_or_else(|| "starting".to_owned(), |h| h.phase.clone()),
            pipeline.as_ref().and_then(|h| h.last_error.clone()),
        ),
    };
    Summary {
        name: name.to_owned(),
        network: entry.network.clone(),
        running: entry.running,
        state,
        error,
        pipeline,
        api: format!("/projects/{name}"),
        created_at: entry.created_at.clone(),
        updated_at: entry.updated_at.clone(),
    }
}

fn detail(name: &str, entry: &Entry) -> Detail {
    Detail {
        summary: summary(name, entry),
        config: entry.config.clone(),
    }
}
