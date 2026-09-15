//! The control plane: many projects in one process, each with its pipeline supervised
//! and its state API and change feed served (ADR 0017).

use std::collections::{BTreeMap, BTreeSet};
use std::sync::{Arc, Mutex, PoisonError};
use std::time::Duration;

use axum::Router;
use nineveh_api::{Api, Health};
use nineveh_config::{Config, Diagnostics, Project, StartVersion, parse};
use nineveh_core::{Address, Network, Version};
use nineveh_decode::Lockfile;
use nineveh_realtime::Hub;
use nineveh_store::{StoreError, registry, shadow_name};
use serde::Serialize;
use sqlx::PgPool;
use tokio::sync::{RwLock, watch};
use tokio::task::JoinHandle;
use tracing::{error, info, warn};

use crate::catalog::{Catalog, catalog};
use crate::chain::{Chain, ChainError};
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
}

impl<C> std::fmt::Debug for ControlPlane<C> {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("ControlPlane")
            .field("options", &self.options)
            .finish_non_exhaustive()
    }
}

impl<C: Chain> ControlPlane<C> {
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
        });
        for record in registry::list(&plane.pool).await? {
            let loaded = load(&record.config, &record.lock).map(Arc::new);
            if let Err(e) = &loaded {
                warn!(project = %record.name, error = %e, "can't load its stored config");
            }
            let mut entry = plane.entry(
                &record.name,
                loaded,
                record.network,
                record.config,
                record.running,
                record.created_at,
                record.updated_at,
            );
            if entry.running {
                plane.spawn(&record.name, &mut entry);
            }
            info!(project = %record.name, running = entry.running, "loaded");
            plane.projects.write().await.insert(record.name, entry);
        }
        Ok(plane)
    }

    /// Stop every pipeline after its current commit.
    pub async fn shutdown(&self) {
        let runs: Vec<Run> = self
            .projects
            .write()
            .await
            .values_mut()
            .filter_map(|entry| entry.run.take())
            .collect();
        for run in runs {
            stop(run).await;
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
    pub async fn create(&self, text: &str) -> Result<Detail, ControlError> {
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
        registry::insert(&self.pool, &name, config.network.as_str(), text, &lock_text).await?;
        let record = self.record(&name).await?;
        let mut entry = self.entry(
            &name,
            Ok(Arc::new(loaded)),
            record.network,
            record.config,
            true,
            record.created_at,
            record.updated_at,
        );
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
    pub async fn update(&self, name: &str, text: &str) -> Result<Detail, ControlError> {
        let _changing = self.changes.lock().await;
        let config = parse(text).map_err(|d| ControlError::invalid(&d, text))?;
        if config.name.name != name {
            return Err(ControlError::BadRequest(format!(
                "the config names the project `{}`; renaming isn't supported, so keep `name: {name}`",
                config.name.name
            )));
        }
        let running = self.get_entry(name, |e| e.running).await?;
        let (loaded, lock_text) = self.prepare(&config, text).await?;
        let old = self.take_run(name).await;
        if let Some(run) = old {
            stop(run).await;
        }
        registry::update(&self.pool, name, text, &lock_text).await?;
        let record = self.record(name).await?;
        let mut entry = self.entry(
            name,
            Ok(Arc::new(loaded)),
            record.network,
            record.config,
            running,
            record.created_at,
            record.updated_at,
        );
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
    pub async fn set_running(&self, name: &str, running: bool) -> Result<Summary, ControlError> {
        let _changing = self.changes.lock().await;
        self.get_entry(name, |_| ()).await?;
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
        self.get_entry(name, |e| summary(name, e)).await
    }

    /// Stop a project, drop its state, and forget it.
    ///
    /// # Errors
    ///
    /// If there's no such project, or the database fails.
    pub async fn delete(&self, name: &str) -> Result<(), ControlError> {
        let _changing = self.changes.lock().await;
        self.get_entry(name, |_| ()).await?;
        if let Some(run) = self.take_run(name).await {
            stop(run).await;
        }
        registry::delete(&self.pool, name).await?;
        self.projects.write().await.remove(name);
        info!(project = %name, "deleted");
        Ok(())
    }

    /// Every project, by name.
    pub async fn list(&self) -> Vec<Summary> {
        self.projects
            .read()
            .await
            .iter()
            .map(|(name, entry)| summary(name, entry))
            .collect()
    }

    /// One project, with its config.
    ///
    /// # Errors
    ///
    /// If there's no such project.
    pub async fn get(&self, name: &str) -> Result<Detail, ControlError> {
        self.get_entry(name, |e| detail(name, e)).await
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

    async fn get_entry<T>(
        &self,
        name: &str,
        f: impl FnOnce(&Entry) -> T,
    ) -> Result<T, ControlError> {
        self.projects
            .read()
            .await
            .get(name)
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

    /// A served project: its API and change feed over the schema of its name.
    #[allow(clippy::too_many_arguments, reason = "the registry row's fields")]
    fn entry(
        &self,
        name: &str,
        loaded: Result<Arc<Loaded>, String>,
        network: String,
        config: String,
        running: bool,
        created_at: String,
        updated_at: String,
    ) -> Entry {
        let (health, _) = watch::channel(None);
        let router = match &loaded {
            Ok(loaded) => {
                let api = Arc::new(Api::new(
                    self.pool.clone(),
                    name,
                    &loaded.project,
                    health.subscribe(),
                ));
                nineveh_api::router(api).merge(nineveh_realtime::router(self.hub.feed(name)))
            }
            Err(_) => Router::new(),
        };
        Entry {
            network,
            config,
            running,
            created_at,
            updated_at,
            loaded,
            router,
            health,
            run: None,
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
