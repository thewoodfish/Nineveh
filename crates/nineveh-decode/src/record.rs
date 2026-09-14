//! From stream transactions to typed records for the sources a project selects.

use std::collections::HashMap;
use std::fmt;

use nineveh_core::{
    Address, Identifier, InvalidAddress, InvalidTypeTag, StructName, StructTag, TypeTag, Value,
    Version,
};
use nineveh_proto::transaction::{
    Event, Transaction, transaction::TxnData, write_set_change::Change,
};

use crate::json::{Decoder, ValueError};
use crate::layout::framework;
use crate::lock::Lockfile;

/// Identifies one of a project's sources; assigned by the caller.
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Hash)]
pub struct SourceId(pub u32);

impl fmt::Display for SourceId {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(f, "source #{}", self.0)
    }
}

/// Which struct types an `event:` or `resource:` source accepts.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum TypeMatcher {
    /// Exactly this type, type arguments included.
    Exact(StructTag),
    /// Any instantiation of a generic struct, like `0x1::coin::CoinStore`.
    AnyInstance(StructName),
}

impl TypeMatcher {
    fn name(&self) -> &StructName {
        match self {
            Self::Exact(tag) => &tag.name,
            Self::AnyInstance(name) => name,
        }
    }

    fn matches(&self, tag: &StructTag) -> bool {
        match self {
            Self::Exact(exact) => exact == tag,
            Self::AnyInstance(name) => *name == tag.name,
        }
    }
}

/// The framework collection a `table:` source's field holds (ADR 0003).
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Container {
    /// `Table<K, V>` or `TableWithLength<K, V>`: one item per entry.
    Table,
    /// `SmartTable<K, V>`: items are buckets of entries, keyed by bucket index.
    SmartTable,
    /// `BigOrderedMap<K, V>`: items are B+ tree nodes, keyed by slot index.
    BigOrderedMap,
}

/// What a `table:` source accepts: the user-level key and value types of its field.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct TableMatcher {
    pub container: Container,
    pub key: TypeTag,
    pub value: TypeTag,
}

impl TableMatcher {
    /// The matcher for field `field` of `parent`, like `Vault.positions`.
    ///
    /// # Errors
    ///
    /// If `parent` has no layout or no such field, the field isn't a supported
    /// collection, or its type depends on type parameters `parent` doesn't fix.
    pub fn for_field(
        lock: &Lockfile,
        parent: &StructTag,
        field: &str,
    ) -> Result<Self, SelectionError> {
        let layout = lock
            .get(&parent.name)
            .ok_or_else(|| SelectionError::NoLayout(parent.name.clone()))?;
        let declared = layout.field(field).ok_or_else(|| SelectionError::NoField {
            parent: Box::new(parent.clone()),
            field: field.to_owned(),
        })?;
        let not_table = || SelectionError::NotATable {
            parent: Box::new(parent.clone()),
            field: field.to_owned(),
            ty: declared.to_string(),
        };
        if usize::from(layout.type_params) != parent.type_args.len() {
            return Err(SelectionError::Generic {
                parent: Box::new(parent.clone()),
                field: field.to_owned(),
            });
        }
        let ty = declared
            .substitute(&parent.type_args)
            .map_err(|_| not_table())?;
        let tag = ty.as_struct().ok_or_else(not_table)?;
        let container = if framework::is_table(&tag.name) {
            Container::Table
        } else if framework::is_smart_table(&tag.name) {
            Container::SmartTable
        } else if framework::is_big_ordered_map(&tag.name) {
            Container::BigOrderedMap
        } else {
            return Err(not_table());
        };
        let [key, value] = tag.type_args.as_slice() else {
            return Err(not_table());
        };
        Ok(Self {
            container,
            key: key.clone(),
            value: value.clone(),
        })
    }

    /// The `(key_type, value_type)` the stream reports for this container's items.
    fn item_types(&self) -> (TypeTag, TypeTag) {
        match self.container {
            Container::Table => (self.key.clone(), self.value.clone()),
            Container::SmartTable => (
                TypeTag::U64,
                TypeTag::Vector(Box::new(framework_struct(
                    "smart_table",
                    "Entry",
                    vec![self.key.clone(), self.value.clone()],
                ))),
            ),
            Container::BigOrderedMap => (
                TypeTag::U64,
                framework_struct(
                    "storage_slots_allocator",
                    "Link",
                    vec![framework_struct(
                        "big_ordered_map",
                        "Node",
                        vec![self.key.clone(), self.value.clone()],
                    )],
                ),
            ),
        }
    }
}

fn framework_struct(module: &'static str, name: &'static str, type_args: Vec<TypeTag>) -> TypeTag {
    TypeTag::Struct(Box::new(StructTag {
        name: StructName::new(
            Address::ONE,
            Identifier::from_static(module),
            Identifier::from_static(name),
        ),
        type_args,
    }))
}

/// A project's sources, compiled for fast matching against the stream.
#[derive(Debug, Clone, Default)]
pub struct Selection {
    events: HashMap<StructName, Vec<(TypeMatcher, SourceId)>>,
    resources: HashMap<StructName, Vec<(TypeMatcher, SourceId)>>,
    /// Resource sources whose struct is a member of the keyed resource group.
    group_members: HashMap<StructName, Vec<SourceId>>,
    table_items: HashMap<(TypeTag, TypeTag), Vec<(SourceId, Container)>>,
    table_keys: HashMap<TypeTag, Vec<(SourceId, Container)>>,
}

impl Selection {
    #[must_use]
    pub fn new() -> Self {
        Self::default()
    }

    /// Select events of the matched type.
    ///
    /// # Errors
    ///
    /// If the type has no layout in `lock`.
    pub fn add_event(
        &mut self,
        lock: &Lockfile,
        id: SourceId,
        matcher: TypeMatcher,
    ) -> Result<(), SelectionError> {
        check_layout(lock, &matcher)?;
        self.events
            .entry(matcher.name().clone())
            .or_default()
            .push((matcher, id));
        Ok(())
    }

    /// Select writes and deletes of resources of the matched type.
    ///
    /// # Errors
    ///
    /// If the type has no layout in `lock`.
    pub fn add_resource(
        &mut self,
        lock: &Lockfile,
        id: SourceId,
        matcher: TypeMatcher,
    ) -> Result<(), SelectionError> {
        let layout = check_layout(lock, &matcher)?;
        if let Some(group) = &layout.group {
            self.group_members
                .entry(group.clone())
                .or_default()
                .push(id);
        }
        self.resources
            .entry(matcher.name().clone())
            .or_default()
            .push((matcher, id));
        Ok(())
    }

    /// Select items of tables with the matcher's key and value types.
    pub fn add_table(&mut self, id: SourceId, matcher: &TableMatcher) {
        let (key, value) = matcher.item_types();
        self.table_keys
            .entry(key.clone())
            .or_default()
            .push((id, matcher.container));
        self.table_items
            .entry((key, value))
            .or_default()
            .push((id, matcher.container));
    }

    /// Whether this selection can match anything.
    #[must_use]
    pub fn is_empty(&self) -> bool {
        self.events.is_empty() && self.resources.is_empty() && self.table_items.is_empty()
    }
}

fn check_layout<'l>(
    lock: &'l Lockfile,
    matcher: &TypeMatcher,
) -> Result<&'l crate::layout::StructLayout, SelectionError> {
    let name = matcher.name();
    let layout = lock
        .get(name)
        .ok_or_else(|| SelectionError::NoLayout(name.clone()))?;
    if let TypeMatcher::Exact(tag) = matcher
        && usize::from(layout.type_params) != tag.type_args.len()
    {
        return Err(SelectionError::Arity {
            ty: Box::new(tag.clone()),
            declared: layout.type_params,
        });
    }
    Ok(layout)
}

/// A source that can't be selected against the lock.
#[derive(Debug, Clone, PartialEq, Eq, thiserror::Error)]
#[non_exhaustive]
pub enum SelectionError {
    #[error("`{0}` has no layout in nineveh.lock; run `nineveh init` to refresh it")]
    NoLayout(StructName),

    #[error("`{ty}` has the wrong number of type arguments: the struct declares {declared}")]
    Arity { ty: Box<StructTag>, declared: u16 },

    #[error("`{parent}` has no field `{field}`")]
    NoField {
        parent: Box<StructTag>,
        field: String,
    },

    #[error("`{parent}.{field}` is a `{ty}`, not a Table, SmartTable or BigOrderedMap")]
    NotATable {
        parent: Box<StructTag>,
        field: String,
        ty: String,
    },

    #[error("`{parent}.{field}`: give `{parent}` its type arguments to fix the table's types")]
    Generic {
        parent: Box<StructTag>,
        field: String,
    },
}

// --- records ---------------------------------------------------------------------

/// The selected records from one transaction, in the order they happened.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct DecodedTransaction {
    pub version: Version,
    /// Block timestamp, in microseconds since the Unix epoch.
    pub timestamp_micros: u64,
    /// Whether the transaction succeeded. Failed transactions still commit a write set
    /// (gas, sequence number), so their records are real.
    pub success: bool,
    /// The sender, for user transactions.
    pub sender: Option<Address>,
    pub records: Vec<Record>,
}

/// One input to a source.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Record {
    pub source: SourceId,
    /// Where in the transaction the record came from.
    pub origin: Origin,
    pub data: RecordData,
}

/// A record's position in its transaction. Events come before write-set changes.
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Hash)]
pub enum Origin {
    Event(u32),
    Change(u32),
}

impl fmt::Display for Origin {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::Event(i) => write!(f, "event {i}"),
            Self::Change(i) => write!(f, "write-set change {i}"),
        }
    }
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum RecordData {
    Event {
        ty: StructTag,
        /// The event handle's account for legacy handle events; `0x0` for module
        /// events.
        account: Address,
        creation_number: u64,
        sequence_number: u64,
        value: Value,
    },
    ResourceWrite {
        address: Address,
        ty: StructTag,
        value: Value,
    },
    ResourceDelete {
        address: Address,
        ty: StructTag,
    },
    /// The object at `address` was deleted, taking every member of `group` with it.
    /// The group delete is the only record for its members (ADR 0002).
    GroupDelete {
        address: Address,
        group: StructTag,
    },
    /// An item write. For `SmartTable` and `BigOrderedMap` the item is a whole bucket
    /// or node; per-entry changes come from diffing it against its previous contents.
    TableWrite {
        container: Container,
        handle: Address,
        key: Value,
        value: Value,
    },
    /// An item delete. The stream gives no value type for deletes, so this is
    /// matched on the key type alone: it's a *candidate*, and applies only if the
    /// handle is one the source has written to (ADR 0003).
    TableDelete {
        container: Container,
        handle: Address,
        key: Value,
    },
}

/// Decodes transactions into records for a [`Selection`].
#[derive(Debug, Clone, Copy)]
pub struct TransactionDecoder<'a> {
    decoder: Decoder<'a>,
    selection: &'a Selection,
}

impl<'a> TransactionDecoder<'a> {
    #[must_use]
    pub fn new(lock: &'a Lockfile, selection: &'a Selection) -> Self {
        Self {
            decoder: Decoder::new(lock),
            selection,
        }
    }

    /// Decode every record the selection matches in `tx`.
    ///
    /// # Errors
    ///
    /// If a matched record doesn't decode. The error is deterministic: the project
    /// halts at this version (ADR 0005).
    pub fn decode(&self, tx: &Transaction) -> Result<DecodedTransaction, DecodeError> {
        let version = Version::new(tx.version);
        let info = tx.info.as_ref();
        let mut out = DecodedTransaction {
            version,
            timestamp_micros: timestamp_micros(tx),
            success: info.is_some_and(|i| i.success),
            sender: None,
            records: Vec::new(),
        };

        let events: &[Event] = match &tx.txn_data {
            Some(TxnData::User(user)) => {
                if let Some(request) = &user.request {
                    out.sender = Some(
                        parse_address(&request.sender)
                            .map_err(|e| DecodeError::new(version, None, e))?,
                    );
                }
                &user.events
            }
            Some(TxnData::BlockMetadata(block)) => &block.events,
            Some(TxnData::Genesis(genesis)) => &genesis.events,
            Some(TxnData::Validator(validator)) => &validator.events,
            Some(TxnData::StateCheckpoint(_) | TxnData::BlockEpilogue(_)) | None => &[],
        };

        for (i, event) in events.iter().enumerate() {
            let origin = Origin::Event(index(i));
            self.event(event, origin, &mut out.records)
                .map_err(|kind| DecodeError::new(version, Some(origin), kind))?;
        }
        for (i, change) in info.map_or(&[][..], |i| &i.changes).iter().enumerate() {
            let origin = Origin::Change(index(i));
            let Some(change) = &change.change else {
                continue;
            };
            self.change(change, origin, &mut out.records)
                .map_err(|kind| DecodeError::new(version, Some(origin), kind))?;
        }
        Ok(out)
    }

    fn event(
        &self,
        event: &Event,
        origin: Origin,
        out: &mut Vec<Record>,
    ) -> Result<(), DecodeErrorKind> {
        let tag = parse_struct(&event.type_str)?;
        let Some(sources) = matching(&self.selection.events, &tag) else {
            return Ok(());
        };
        let value = self.value(&tag, &event.data)?;
        let key = event.key.as_ref();
        let account = key
            .map(|k| parse_address(&k.account_address))
            .transpose()?
            .unwrap_or(Address::ZERO);
        push_all(out, &sources, origin, |_| RecordData::Event {
            ty: tag.clone(),
            account,
            creation_number: key.map_or(0, |k| k.creation_number),
            sequence_number: event.sequence_number,
            value: value.clone(),
        });
        Ok(())
    }

    fn change(
        &self,
        change: &Change,
        origin: Origin,
        out: &mut Vec<Record>,
    ) -> Result<(), DecodeErrorKind> {
        match change {
            Change::WriteResource(write) => {
                let tag = parse_struct(&write.type_str)?;
                let Some(sources) = matching(&self.selection.resources, &tag) else {
                    return Ok(());
                };
                let address = parse_address(&write.address)?;
                let value = self.value(&tag, &write.data)?;
                push_all(out, &sources, origin, |_| RecordData::ResourceWrite {
                    address,
                    ty: tag.clone(),
                    value: value.clone(),
                });
            }
            Change::DeleteResource(delete) => {
                let tag = parse_struct(&delete.type_str)?;
                let address = parse_address(&delete.address)?;
                if let Some(sources) = matching(&self.selection.resources, &tag) {
                    push_all(out, &sources, origin, |_| RecordData::ResourceDelete {
                        address,
                        ty: tag.clone(),
                    });
                }
                if let Some(members) = self.selection.group_members.get(&tag.name) {
                    push_all(out, members, origin, |_| RecordData::GroupDelete {
                        address,
                        group: tag.clone(),
                    });
                }
            }
            Change::WriteTableItem(write) => {
                let data = write
                    .data
                    .as_ref()
                    .ok_or(DecodeErrorKind::MissingTableData)?;
                let types = (parse_type(&data.key_type)?, parse_type(&data.value_type)?);
                let Some(sources) = self.selection.table_items.get(&types) else {
                    return Ok(());
                };
                let handle = parse_address(&write.handle)?;
                let key = self.decode_str(&types.0, &data.key)?;
                let value = self.decode_str(&types.1, &data.value)?;
                for &(source, container) in sources {
                    out.push(Record {
                        source,
                        origin,
                        data: RecordData::TableWrite {
                            container,
                            handle,
                            key: key.clone(),
                            value: value.clone(),
                        },
                    });
                }
            }
            Change::DeleteTableItem(delete) => {
                let data = delete
                    .data
                    .as_ref()
                    .ok_or(DecodeErrorKind::MissingTableData)?;
                let key_type = parse_type(&data.key_type)?;
                let Some(sources) = self.selection.table_keys.get(&key_type) else {
                    return Ok(());
                };
                let handle = parse_address(&delete.handle)?;
                let key = self.decode_str(&key_type, &data.key)?;
                for &(source, container) in sources {
                    out.push(Record {
                        source,
                        origin,
                        data: RecordData::TableDelete {
                            container,
                            handle,
                            key: key.clone(),
                        },
                    });
                }
            }
            Change::WriteModule(_) | Change::DeleteModule(_) => {}
        }
        Ok(())
    }

    fn value(&self, tag: &StructTag, data: &str) -> Result<Value, DecodeErrorKind> {
        self.decode_str(&TypeTag::Struct(Box::new(tag.clone())), data)
    }

    fn decode_str(&self, ty: &TypeTag, data: &str) -> Result<Value, DecodeErrorKind> {
        self.decoder
            .decode_str(ty, data)
            .map_err(|e| DecodeErrorKind::Value {
                ty: ty.to_string(),
                error: Box::new(e),
            })
    }
}

/// The sources that accept `tag`, if any.
fn matching(
    index: &HashMap<StructName, Vec<(TypeMatcher, SourceId)>>,
    tag: &StructTag,
) -> Option<Vec<SourceId>> {
    let sources: Vec<SourceId> = index
        .get(&tag.name)?
        .iter()
        .filter(|(m, _)| m.matches(tag))
        .map(|&(_, id)| id)
        .collect();
    (!sources.is_empty()).then_some(sources)
}

fn push_all(
    out: &mut Vec<Record>,
    sources: &[SourceId],
    origin: Origin,
    data: impl Fn(SourceId) -> RecordData,
) {
    for &source in sources {
        out.push(Record {
            source,
            origin,
            data: data(source),
        });
    }
}

fn timestamp_micros(tx: &Transaction) -> u64 {
    tx.timestamp.as_ref().map_or(0, |t| {
        let seconds = u64::try_from(t.seconds).unwrap_or(0);
        let micros = u64::try_from(t.nanos / 1000).unwrap_or(0);
        seconds.saturating_mul(1_000_000).saturating_add(micros)
    })
}

/// Transactions hold far fewer than 2^32 events or changes; saturate rather than
/// fail if that ever changes, since the index is only used to locate records.
fn index(i: usize) -> u32 {
    u32::try_from(i).unwrap_or(u32::MAX)
}

fn parse_struct(s: &str) -> Result<StructTag, DecodeErrorKind> {
    match parse_type(s)? {
        TypeTag::Struct(tag) => Ok(*tag),
        _ => Err(DecodeErrorKind::NotAStruct(s.to_owned())),
    }
}

fn parse_type(s: &str) -> Result<TypeTag, DecodeErrorKind> {
    s.parse().map_err(DecodeErrorKind::Type)
}

fn parse_address(s: &str) -> Result<Address, DecodeErrorKind> {
    s.parse().map_err(DecodeErrorKind::Address)
}

// --- errors ----------------------------------------------------------------------

/// A record that doesn't decode, located by version and position.
///
/// Decoding is deterministic, so this is never retryable: the same input fails the
/// same way. The project halts at `version` until the cause is fixed.
#[derive(Debug, Clone, PartialEq, Eq, thiserror::Error)]
#[error("version {version}{}: {kind}", origin.map(|o| format!(", {o}")).unwrap_or_default())]
pub struct DecodeError {
    pub version: Version,
    pub origin: Option<Origin>,
    pub kind: DecodeErrorKind,
}

impl DecodeError {
    fn new(version: Version, origin: Option<Origin>, kind: DecodeErrorKind) -> Self {
        Self {
            version,
            origin,
            kind,
        }
    }

    /// Always `false`: decoding the same transaction again fails the same way.
    #[must_use]
    pub fn is_retryable(&self) -> bool {
        false
    }
}

#[derive(Debug, Clone, PartialEq, Eq, thiserror::Error)]
#[non_exhaustive]
pub enum DecodeErrorKind {
    #[error("invalid type string: {0}")]
    Type(InvalidTypeTag),

    #[error("expected a struct type, got `{0}`")]
    NotAStruct(String),

    #[error(transparent)]
    Address(InvalidAddress),

    #[error("table item has no decoded key/value data")]
    MissingTableData,

    #[error("`{ty}` {error}")]
    Value { ty: String, error: Box<ValueError> },
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn framework_names_are_valid() {
        let smart = TableMatcher {
            container: Container::SmartTable,
            key: TypeTag::Address,
            value: TypeTag::U64,
        };
        assert_eq!(
            smart.item_types().1.to_string(),
            "vector<0x1::smart_table::Entry<address, u64>>"
        );
        let map = TableMatcher {
            container: Container::BigOrderedMap,
            key: TypeTag::U64,
            value: TypeTag::Bool,
        };
        assert_eq!(
            map.item_types().1.to_string(),
            "0x1::storage_slots_allocator::Link<0x1::big_ordered_map::Node<u64, bool>>"
        );
    }
}
