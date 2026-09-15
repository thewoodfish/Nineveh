//! Type-directed decoding of the Aptos Transaction Stream.
//!
//! The stream carries Move values as JSON strings rendered by the fullnode, not BCS,
//! and the JSON carries no types (ADR 0002). This crate decodes it against struct
//! layouts pinned in `nineveh.lock`:
//!
//! - [`Lockfile`] is the pinned layouts; [`LockBuilder`] builds one from module ABIs.
//! - [`Decoder`] turns one JSON value plus its Move type into a typed [`Value`].
//! - [`Selection`] compiles a project's sources, and [`TransactionDecoder`] turns each
//!   stream [`Transaction`](nineveh_proto::transaction::Transaction) into the
//!   [`Record`]s those sources select.
//!
//! Decoding never touches the network and never guesses. A record that doesn't match
//! its layout is a [`DecodeError`] naming the version, the record and the JSON path.
//!
//! [`Value`]: nineveh_core::Value

mod abi;
mod json;
mod layout;
mod lock;
mod metadata;
mod record;

pub use abi::{BuildError, FieldAbi, LockBuilder, ModuleAbi, ModuleId, StructAbi, VariantAbi};
pub use json::{Decoder, Reason, ValueError};
pub use layout::{Body, Field, StructLayout, Variant};
pub use lock::{FORMAT as LOCK_FORMAT, LockError, Lockfile};
pub use metadata::{MetadataError, resource_group_members};
pub use record::{
    Container, DecodeError, DecodeErrorKind, DecodedTransaction, Origin, Record, RecordData,
    Selection, SelectionError, SourceId, TableMatcher, TransactionDecoder, TypeMatcher,
};
