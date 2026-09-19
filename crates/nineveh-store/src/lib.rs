//! Nineveh's Postgres store: where a project's state tables live, and the one place
//! they're written.
//!
//! Each project's state is a Postgres schema generated from its config (see
//! [`layout`](crate) in the source): a table per state table with the typed columns
//! the API serves (ADR 0008), plus the exact encoding of the engine's keys and rows
//! that the fold reads back. Nineveh's own tables live in the `nineveh` schema: a row
//! per build, holding its cursor, and the change outbox (ADR 0006).
//!
//! The pipeline drives a [`Store`] batch by batch:
//!
//! 1. [`Store::load`] the rows a batch reads into a [`Loaded`] view, and fold over it.
//!    The fold names any key it wasn't given; load those and fold again (ADR 0013).
//! 2. [`Store::commit`] the resulting [`ChangeSet`](nineveh_engine::ChangeSet): row
//!    writes, outbox rows and the cursor in one transaction (ADR 0005). A crash
//!    before the commit loses nothing; the next [`Store::open`] resumes from the
//!    cursor and the fold recomputes the same batch.
//!
//! The control plane's [`registry`] of projects lives beside the builds, in the
//! `nineveh` schema (ADR 0017), with its [`accounts`], sessions and API keys (ADR 0018).
//!
//! Every build records a fingerprint of its config, lock and Nineveh's semantics, and
//! [`Store::open`] refuses to extend a schema built from anything else.

pub mod accounts;
mod cells;
mod codec;
mod error;
mod layout;
pub mod records;
pub mod registry;
mod rows;
mod store;
pub mod webhooks;

pub use error::StoreError;
pub use rows::row_json;
pub use store::{Loaded, NOTIFY_CHANNEL, Store, lock_hash, migrate, shadow_name};
