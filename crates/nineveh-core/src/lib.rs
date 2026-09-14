//! Core domain types shared by every Nineveh crate.
//!
//! This crate has no I/O and no async runtime. Everything else depends on it, so it
//! stays small and dependency-light. CI checks that it never picks up tokio, sqlx or
//! tonic.

mod chain;

pub use chain::{ChainId, InvalidChainId, Network, UnknownNetwork, Version};
