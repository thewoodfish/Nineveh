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
//!   API and change feed. [`router`] puts it on HTTP for Studio.
//!
//! Everything read from Aptos goes through a [`Chain`]: [`Hosted`] in production.
//! `nineveh init` and `nineveh run` are thin wrappers over [`pin`] and [`Runner`], so
//! the CLI and the control plane set up and run projects the same way.

pub mod catalog;
mod chain;
mod http;
mod pin;
mod plane;
mod runner;
pub mod scaffold;

pub use catalog::{Catalog, Item, ItemField, ItemKind, catalog, snake_case};
pub use chain::{Chain, ChainError, Hosted, ModuleInfo};
pub use http::router;
pub use pin::{PinError, pin};
pub use plane::{ControlError, ControlPlane, Detail, ScaffoldRequest, StartRequest, Summary};
pub use runner::{RunError, RunOptions, Runner};
pub use scaffold::{Draft, ScaffoldError, Start, scaffold};
