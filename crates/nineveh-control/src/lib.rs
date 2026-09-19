//! Nineveh's control plane: what Studio drives (ADR 0017).
//!
//! - [`catalog`] reads a contract's modules and lists what a project can follow:
//!   events, resources, and tables held in resources.
//! - [`scaffold`] turns picks from a catalog into a `nineveh.yaml`: a `log` table per
//!   event and a `mirror` table per resource or table, so a developer has live tables
//!   before writing any rule.
//! - [`pin`] pins a config's layouts in a lock and resolves `start_version: auto`.
//! - [`Runner`] builds a project's state and keeps it current, rebuilding into a
//!   shadow schema when the config changes (ADR 0016).
//!
//! - [`ControlPlane`] manages many projects in one process: it saves each one's
//!   config and lock in the registry, supervises its pipeline, and serves its state
//!   API and change feed. [`router`] puts it on HTTP for Studio, signed in with
//!   GitHub when hosted and reached with project API keys (ADR 0018; see [`Access`]).
//!
//! Everything read from Aptos goes through a [`Chain`]: [`Hosted`] in production.
//! `nineveh init` and `nineveh run` are thin wrappers over [`pin`] and [`Runner`], so
//! the CLI and the control plane set up and run projects the same way.

mod auth;
mod catalog;
mod chain;
mod deliver;
mod http;
pub mod idle;
mod pin;
mod plane;
mod runner;
mod scaffold;
pub mod tier;

pub use auth::{Access, AuthError, ExternalUser, GitHub, IdentityProvider};
pub use catalog::{Catalog, Item, ItemField, ItemKind, catalog, snake_case};
pub use chain::{Chain, ChainError, Hosted, ModuleInfo};
pub use deliver::Deliveries;
pub use http::router;
pub use pin::{PinError, pin};
pub use plane::{
    Caller, ControlError, ControlPlane, Detail, ReaderInfo, ScaffoldRequest, StartRequest, Summary,
};
pub use runner::{RunError, RunOptions, Runner};
pub use scaffold::{Draft, ScaffoldError, Start, scaffold};
pub use tier::{FREE, Limit, Limits};
