//! Test workloads shared by Nineveh's crates.
//!
//! [`vault`] is a synthetic vault contract: random operations rendered as real
//! Transaction Stream messages (JSON in the fullnode's conventions), with a model of
//! the contract's state to check a projection against. The engine's replay property
//! runs it in memory; the store's runs it through Postgres.

#![allow(
    clippy::unwrap_used,
    clippy::expect_used,
    clippy::panic,
    clippy::missing_panics_doc,
    clippy::must_use_candidate,
    reason = "test support: helpers panic on unexpected results"
)]

pub mod vault;
