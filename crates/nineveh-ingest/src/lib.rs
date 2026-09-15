//! Client for the Aptos Transaction Stream: Aptos' gRPC feed of transactions,
//! ordered by version. Also a small fullnode REST client ([`RestClient`]) for the
//! current ledger version and module ABIs.
//!
//! [`TransactionStream`] yields batches of transactions and checks that versions arrive
//! in order with nothing skipped or repeated. It also checks that the stream is for the
//! chain the caller expected. Everything Aptos-specific about the wire format stays
//! behind this crate and `nineveh-decode`.
//!
//! This client is intentionally thin: it makes one connection and surfaces every
//! failure. Reconnect-and-resume from the committed cursor belongs to the pipeline
//! supervisor. That component uses [`IngestError::is_retryable`] to tell transient
//! failures from fatal ones.

pub mod proto;

mod config;
mod contiguity;
mod error;
mod rest;
mod stream;

pub use config::{Compression, DEFAULT_MAX_MESSAGE_SIZE, StreamConfig};
pub use error::IngestError;
pub use rest::{LedgerInfo, Module, RestClient, RestError};
pub use stream::{Batch, TransactionStream};
