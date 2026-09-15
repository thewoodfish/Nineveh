//! Nineveh's control plane: what Studio drives (ADR 0017).
//!
//! - [`catalog`] reads a contract's modules and lists what a project can follow:
//!   events, resources, and tables held in resources.
//! - [`scaffold`] turns picks from a catalog into a `nineveh.yaml`: a `log` table per
//!   event and a `mirror` table per resource or table, so a developer has live tables
//!   before writing any rule.

pub mod catalog;
pub mod scaffold;

pub use catalog::{Catalog, Item, ItemField, ItemKind, catalog, snake_case};
pub use scaffold::{Draft, ScaffoldError, Start, scaffold};
