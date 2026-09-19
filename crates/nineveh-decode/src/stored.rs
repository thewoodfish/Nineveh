//! The record log's encoding (ADR 0022).
//!
//! A project's rebuild replays its own decoded records rather than re-reading the
//! chain, so those records have to survive a round trip through Postgres. This is the
//! shape they take: a mirror of [`Record`] carrying every discriminator, with Move
//! values in [`Stored`] rather than the API's shape.
//!
//! It stores what the decoder emitted, not what matched a declared source. A table
//! source only works because parent resource writes reveal its handles (ADR 0012), and
//! those parents are pinned rather than declared — dropping them would lose handle
//! attribution on replay and break table sources without saying so.
//!
//! Records name their source rather than numbering it. [`SourceId`] is an index into
//! `config.sources` (`resolve.rs`), so reordering the sources in a config would
//! renumber every one of them and silently misattribute every record already stored.
//! The name is what the user wrote and what survives an edit, so the log stores that
//! and the id is resolved again on the way out.
//!
//! `success` and `sender` are kept although nothing folds them today. The log is
//! meant to be replayable under rules written later, and a rule that reads
//! `tx.sender` shouldn't force a backfill of history we already had.

use std::str::FromStr as _;

use serde::{Deserialize, Serialize};

use nineveh_core::{Address, InvalidStored, Stored, StructTag, Value};

use crate::record::{Container, Origin, Record, RecordData, SourceId};

/// One record, as it is written to the log.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct StoredRecord {
    /// The source's name in the config, not its id: ids are positional.
    pub source: String,
    pub origin: StoredOrigin,
    pub data: StoredData,
}

/// A record's position in its transaction.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
pub enum StoredOrigin {
    Event(u32),
    Change(u32),
}

/// The record's payload, by kind.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub enum StoredData {
    Event {
        ty: String,
        account: String,
        creation_number: String,
        sequence_number: String,
        value: Stored,
    },
    ResourceWrite {
        address: String,
        ty: String,
        value: Stored,
    },
    ResourceDelete {
        address: String,
        ty: String,
    },
    GroupDelete {
        address: String,
        group: String,
    },
    TableWrite {
        container: Container,
        handle: String,
        key: Stored,
        value: Stored,
    },
    TableValue {
        handle: String,
        value: Stored,
    },
    TableDelete {
        container: Container,
        handle: String,
        key: Stored,
    },
}

/// A logged record that doesn't decode: written by an older encoder, or corrupted.
#[derive(Debug, thiserror::Error)]
pub enum InvalidRecord {
    #[error("stored record has an unreadable value: {0}")]
    Value(#[from] InvalidStored),
    #[error("stored record has an unreadable {what}: {reason}")]
    Field { what: &'static str, reason: String },
}

impl InvalidRecord {
    fn field(what: &'static str, reason: &impl ToString) -> Self {
        Self::Field {
            what,
            reason: reason.to_string(),
        }
    }
}

fn address(s: &str) -> Result<Address, InvalidRecord> {
    Address::from_str(s).map_err(|e| InvalidRecord::field("address", &e))
}

fn struct_tag(s: &str) -> Result<StructTag, InvalidRecord> {
    StructTag::from_str(s).map_err(|e| InvalidRecord::field("type", &e))
}

fn int(what: &'static str, s: &str) -> Result<u64, InvalidRecord> {
    s.parse().map_err(|e| InvalidRecord::field(what, &e))
}

impl StoredRecord {
    /// A record as the log stores it, under the name its source has in the config.
    #[must_use]
    pub fn of(record: &Record, source: &str) -> Self {
        let v = Stored::from;
        Self {
            source: source.to_owned(),
            origin: match record.origin {
                Origin::Event(i) => StoredOrigin::Event(i),
                Origin::Change(i) => StoredOrigin::Change(i),
            },
            data: match &record.data {
                RecordData::Event {
                    ty,
                    account,
                    creation_number,
                    sequence_number,
                    value,
                } => StoredData::Event {
                    ty: ty.to_string(),
                    account: account.to_string(),
                    creation_number: creation_number.to_string(),
                    sequence_number: sequence_number.to_string(),
                    value: v(value),
                },
                RecordData::ResourceWrite { address, ty, value } => StoredData::ResourceWrite {
                    address: address.to_string(),
                    ty: ty.to_string(),
                    value: v(value),
                },
                RecordData::ResourceDelete { address, ty } => StoredData::ResourceDelete {
                    address: address.to_string(),
                    ty: ty.to_string(),
                },
                RecordData::GroupDelete { address, group } => StoredData::GroupDelete {
                    address: address.to_string(),
                    group: group.to_string(),
                },
                RecordData::TableWrite {
                    container,
                    handle,
                    key,
                    value,
                } => StoredData::TableWrite {
                    container: *container,
                    handle: handle.to_string(),
                    key: v(key),
                    value: v(value),
                },
                RecordData::TableValue { handle, value } => StoredData::TableValue {
                    handle: handle.to_string(),
                    value: v(value),
                },
                RecordData::TableDelete {
                    container,
                    handle,
                    key,
                } => StoredData::TableDelete {
                    container: *container,
                    handle: handle.to_string(),
                    key: v(key),
                },
            },
        }
    }
}

impl Record {
    /// A logged record, back in the shape the engine folds, under the id its source
    /// has in the config being replayed.
    ///
    /// # Errors
    ///
    /// [`InvalidRecord`] if the stored record doesn't decode, which means an older
    /// encoder or a corrupted row.
    pub fn from_stored(stored: StoredRecord, source: SourceId) -> Result<Self, InvalidRecord> {
        Ok(Self {
            source,
            origin: match stored.origin {
                StoredOrigin::Event(i) => Origin::Event(i),
                StoredOrigin::Change(i) => Origin::Change(i),
            },
            data: match stored.data {
                StoredData::Event {
                    ty,
                    account,
                    creation_number,
                    sequence_number,
                    value,
                } => RecordData::Event {
                    ty: struct_tag(&ty)?,
                    account: address(&account)?,
                    creation_number: int("creation number", &creation_number)?,
                    sequence_number: int("sequence number", &sequence_number)?,
                    value: Value::try_from(value)?,
                },
                StoredData::ResourceWrite {
                    address: a,
                    ty,
                    value,
                } => RecordData::ResourceWrite {
                    address: address(&a)?,
                    ty: struct_tag(&ty)?,
                    value: Value::try_from(value)?,
                },
                StoredData::ResourceDelete { address: a, ty } => RecordData::ResourceDelete {
                    address: address(&a)?,
                    ty: struct_tag(&ty)?,
                },
                StoredData::GroupDelete { address: a, group } => RecordData::GroupDelete {
                    address: address(&a)?,
                    group: struct_tag(&group)?,
                },
                StoredData::TableWrite {
                    container,
                    handle,
                    key,
                    value,
                } => RecordData::TableWrite {
                    container,
                    handle: address(&handle)?,
                    key: Value::try_from(key)?,
                    value: Value::try_from(value)?,
                },
                StoredData::TableValue { handle, value } => RecordData::TableValue {
                    handle: address(&handle)?,
                    value: Value::try_from(value)?,
                },
                StoredData::TableDelete {
                    container,
                    handle,
                    key,
                } => RecordData::TableDelete {
                    container,
                    handle: address(&handle)?,
                    key: Value::try_from(key)?,
                },
            },
        })
    }
}
