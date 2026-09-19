//! Decode every record in the real-transaction fixtures, and pin each rendering
//! convention in `docs/research/stream-json-conventions.md` to the fixture it came from.
//!
//! Layouts come from the module ABIs under `fixtures/abi/<network>/`, trimmed to the
//! structs the fixtures reach and built into a lock by [`LockBuilder`], the same way
//! `nineveh init` does. Each fixture is decoded with a selection that matches every
//! event, resource and table item in it, so a convention the decoder mishandles
//! anywhere in a fixture fails here.

#![allow(
    clippy::unwrap_used,
    clippy::panic,
    reason = "test-only crate: helpers panic on a malformed fixture tree"
)]

use std::collections::BTreeSet;
use std::fs;
use std::path::{Path, PathBuf};

use nineveh_core::{Address, Network, StructName, StructTag, TypeTag, Value};
use nineveh_decode::{
    Container, DecodedTransaction, LockBuilder, Lockfile, ModuleAbi, Origin, Record, RecordData,
    Selection, SourceId, StoredRecord, TableMatcher, TransactionDecoder, TypeMatcher,
};
use nineveh_proto::transaction::{Transaction, transaction::TxnData, write_set_change::Change};
use prost::Message;

fn fixtures_dir() -> PathBuf {
    Path::new(env!("CARGO_MANIFEST_DIR")).join("../../fixtures")
}

fn load_tx(name: &str) -> (Network, Transaction) {
    let network: Network = name.split_once('-').unwrap().0.parse().unwrap();
    let path = fixtures_dir()
        .join(network.as_str())
        .join(format!("{name}.pb"));
    let bytes = fs::read(&path).unwrap_or_else(|e| panic!("{}: {e}", path.display()));
    (network, Transaction::decode(bytes.as_slice()).unwrap())
}

fn all_fixture_names() -> Vec<String> {
    let mut names = Vec::new();
    for network in Network::ALL {
        let Ok(entries) = fs::read_dir(fixtures_dir().join(network.as_str())) else {
            continue;
        };
        for entry in entries {
            let path = entry.unwrap().path();
            if path.extension().is_some_and(|e| e == "pb") {
                names.push(path.file_stem().unwrap().to_string_lossy().into_owned());
            }
        }
    }
    names.sort();
    names
}

fn builder(network: Network) -> LockBuilder {
    let mut builder = LockBuilder::new(network);
    let dir = fixtures_dir().join("abi").join(network.as_str());
    for entry in fs::read_dir(&dir).unwrap() {
        let text = fs::read_to_string(entry.unwrap().path()).unwrap();
        builder.add_module(serde_json::from_str::<ModuleAbi>(&text).unwrap());
    }
    // REST ABIs don't carry resource-group membership; `nineveh init` reads it from
    // module metadata. These are the framework's ObjectGroup members the fixtures use.
    for member in [
        "0x1::object::ObjectCore",
        "0x1::fungible_asset::FungibleStore",
        "0x1::fungible_asset::Metadata",
        "0x1::fungible_asset::ConcurrentSupply",
        "0x1::primary_fungible_store::DeriveRefPod",
    ] {
        builder.set_group(name(member), name("0x1::object::ObjectGroup"));
    }
    builder
}

fn name(s: &str) -> StructName {
    s.parse().unwrap()
}

fn events(tx: &Transaction) -> &[nineveh_proto::transaction::Event] {
    match &tx.txn_data {
        Some(TxnData::User(t)) => &t.events,
        Some(TxnData::BlockMetadata(t)) => &t.events,
        Some(TxnData::Genesis(t)) => &t.events,
        Some(TxnData::Validator(t)) => &t.events,
        _ => &[],
    }
}

fn changes(tx: &Transaction) -> impl Iterator<Item = (u32, &Change)> {
    tx.info
        .iter()
        .flat_map(|i| &i.changes)
        .enumerate()
        .filter_map(|(i, c)| Some((u32::try_from(i).unwrap(), c.change.as_ref()?)))
}

/// The `table:` matcher that selects items with these stream item types.
fn table_matcher(key_type: &str, value_type: &str) -> TableMatcher {
    let key: TypeTag = key_type.parse().unwrap();
    let value: TypeTag = value_type.parse().unwrap();
    let inner_args = |ty: &TypeTag, module: &str, name: &str| -> Option<(TypeTag, TypeTag)> {
        let tag = ty.as_struct()?;
        if !tag.name.is(Address::ONE, module, name) {
            return None;
        }
        match tag.type_args.as_slice() {
            [k, v] => Some((k.clone(), v.clone())),
            [inner] => {
                let node = inner.as_struct()?;
                match node.type_args.as_slice() {
                    [k, v] if node.name.is(Address::ONE, "big_ordered_map", "Node") => {
                        Some((k.clone(), v.clone()))
                    }
                    _ => None,
                }
            }
            _ => None,
        }
    };
    if key == TypeTag::U64 {
        if let TypeTag::Vector(entry) = &value
            && let Some((k, v)) = inner_args(entry, "smart_table", "Entry")
        {
            return TableMatcher {
                container: Container::SmartTable,
                key: k,
                value: v,
            };
        }
        if let Some((k, v)) = inner_args(&value, "storage_slots_allocator", "Link") {
            return TableMatcher {
                container: Container::BigOrderedMap,
                key: k,
                value: v,
            };
        }
    }
    TableMatcher {
        container: Container::Table,
        key,
        value,
    }
}

/// Decode `name` with a selection matching everything in it.
fn decode_everything(name: &str) -> (Lockfile, DecodedTransaction) {
    let (network, tx) = load_tx(name);
    let mut roots = BTreeSet::new();
    let mut event_types = BTreeSet::new();
    let mut resource_types = BTreeSet::new();
    let mut tables = Vec::new();

    let add_roots = |ty: &TypeTag, roots: &mut BTreeSet<StructName>| {
        fn visit(ty: &TypeTag, roots: &mut BTreeSet<StructName>) {
            match ty {
                TypeTag::Vector(inner) => visit(inner, roots),
                TypeTag::Struct(tag) => {
                    roots.insert(tag.name.clone());
                    tag.type_args.iter().for_each(|a| visit(a, roots));
                }
                _ => {}
            }
        }
        visit(ty, roots);
    };

    for event in events(&tx) {
        let tag: StructTag = event.type_str.parse().unwrap();
        add_roots(&TypeTag::Struct(Box::new(tag.clone())), &mut roots);
        event_types.insert(tag);
    }
    for (_, change) in changes(&tx) {
        match change {
            Change::WriteResource(w) => {
                let tag: StructTag = w.type_str.parse().unwrap();
                add_roots(&TypeTag::Struct(Box::new(tag.clone())), &mut roots);
                resource_types.insert(tag);
            }
            Change::DeleteResource(d) => {
                let tag: StructTag = d.type_str.parse().unwrap();
                add_roots(&TypeTag::Struct(Box::new(tag.clone())), &mut roots);
                resource_types.insert(tag);
            }
            Change::WriteTableItem(w) => {
                let data = w.data.as_ref().unwrap();
                add_roots(&data.key_type.parse().unwrap(), &mut roots);
                add_roots(&data.value_type.parse().unwrap(), &mut roots);
                tables.push(table_matcher(&data.key_type, &data.value_type));
            }
            Change::DeleteTableItem(d) => {
                add_roots(
                    &d.data.as_ref().unwrap().key_type.parse().unwrap(),
                    &mut roots,
                );
            }
            Change::WriteModule(_) | Change::DeleteModule(_) => {}
        }
    }
    // Always select ObjectCore, so object deletes have a group member to route to.
    let object_core: StructTag = "0x1::object::ObjectCore".parse().unwrap();
    roots.insert(object_core.name.clone());
    resource_types.insert(object_core);

    let lock = builder(network)
        .build(&roots)
        .unwrap_or_else(|e| panic!("{name}: {e}"));
    // The lock must survive a round trip through its file format unchanged.
    assert_eq!(Lockfile::from_json(&lock.to_json().unwrap()).unwrap(), lock);

    let mut selection = Selection::new();
    let mut next_id = 0;
    let mut id = || {
        next_id += 1;
        SourceId(next_id)
    };
    for tag in event_types {
        selection
            .add_event(&lock, id(), TypeMatcher::Exact(tag))
            .unwrap();
    }
    for tag in resource_types {
        selection
            .add_resource(&lock, id(), TypeMatcher::Exact(tag))
            .unwrap();
    }
    tables.dedup();
    for matcher in &tables {
        selection.add_table(id(), matcher);
    }

    let decoded = TransactionDecoder::new(&lock, &selection)
        .decode(&tx)
        .unwrap_or_else(|e| panic!("{name}: {e}"));
    (lock, decoded)
}

/// Every value reachable through a field called `field`, across all records.
fn fields<'a>(tx: &'a DecodedTransaction, field: &str) -> Vec<&'a Value> {
    fn walk<'a>(value: &'a Value, field: &str, out: &mut Vec<&'a Value>) {
        match value {
            Value::Struct(fields) | Value::Variant { fields, .. } => {
                for (name, v) in fields {
                    if name.as_str() == field {
                        out.push(v);
                    }
                    walk(v, field, out);
                }
            }
            Value::Vector(items) => items.iter().for_each(|v| walk(v, field, out)),
            Value::Option(Some(v)) => walk(v, field, out),
            _ => {}
        }
    }
    let mut out = Vec::new();
    for record in &tx.records {
        match &record.data {
            RecordData::Event { value, .. } | RecordData::ResourceWrite { value, .. } => {
                walk(value, field, &mut out);
            }
            RecordData::TableWrite { key, value, .. } => {
                walk(key, field, &mut out);
                walk(value, field, &mut out);
            }
            _ => {}
        }
    }
    out
}

#[test]
fn every_record_in_every_fixture_decodes() {
    let names = all_fixture_names();
    assert!(
        names.len() >= 13,
        "expected the M0 fixtures, found {names:?}"
    );
    for name in names {
        let (_, tx) = load_tx(&name);
        let (_, decoded) = decode_everything(&name);

        // Every event and every resource or table change yields at least one record.
        let mut expected = BTreeSet::new();
        for i in 0..events(&tx).len() {
            expected.insert(Origin::Event(u32::try_from(i).unwrap()));
        }
        for (i, change) in changes(&tx) {
            if !matches!(change, Change::WriteModule(_) | Change::DeleteModule(_)) {
                expected.insert(Origin::Change(i));
            }
        }
        let produced: BTreeSet<Origin> = decoded.records.iter().map(|r| r.origin).collect();
        eprintln!(
            "{name}: {} records from {} inputs",
            decoded.records.len(),
            expected.len()
        );
        assert_eq!(
            produced, expected,
            "{name}: records don't cover the transaction"
        );
        assert_eq!(decoded.version.get(), tx.version);
    }
}

#[test]
fn integers_follow_the_width_convention() {
    // u16 as a JSON number.
    let (_, tx) = decode_everything("testnet-11196227787");
    assert!(fields(&tx, "inner_max_degree").contains(&&Value::U16(102)));

    // u64 above 2^63, which would overflow BIGINT.
    let (_, tx) = decode_everything("testnet-11196227786");
    assert!(fields(&tx, "balance").contains(&&Value::U64(18_441_553_330_519_219_599)));
    assert!(fields(&tx, "frozen").contains(&&Value::Bool(false)));

    // i64 and i128 as decimal strings, negatives with a sign.
    let (_, tx) = decode_everything("mainnet-7205731421");
    assert!(fields(&tx, "index").contains(&&Value::I128(-6_294_071_852_848)));
    assert!(
        fields(&tx, "unrealized_funding_amount_before_last_update")
            .iter()
            .all(|v| matches!(v, Value::I64(_)))
    );
}

#[test]
fn addresses_and_bytes_are_told_apart_by_layout() {
    let (_, tx) = decode_everything("testnet-11196227807");
    // A 63-digit proposer normalizes to 64 digits.
    let proposers = fields(&tx, "proposer");
    assert!(
        proposers
            .iter()
            .any(|v| matches!(v, Value::Address(a) if a.to_string().starts_with("0x0")))
    );
    // "0xf600" is a vector<u8> here, not a short address.
    assert!(fields(&tx, "previous_block_votes_bitvec").contains(&&Value::Bytes(vec![0xf6, 0x00])));
}

#[test]
fn framework_wrappers_and_enums() {
    let (_, tx) = decode_everything("testnet-11196227786");
    // Option<T> as {"vec": [x]}, with an Object<T> {"inner": "0xa"} inside.
    let burn_ref = fields(&tx, "burn_ref_opt");
    assert!(burn_ref.iter().any(|v| matches!(v, Value::Option(Some(_)))));
    assert!(fields(&tx, "inner").contains(&&Value::Address(Address::special(0xa))));

    // Move 2 enums, including nested ones inside BigOrderedMap nodes.
    let (_, tx) = decode_everything("testnet-11196227787");
    let v1 = tx.records.iter().any(|r| match &r.data {
        RecordData::ResourceWrite { value, .. } | RecordData::TableWrite { value, .. } => {
            contains_variant(value, "V1")
        }
        _ => false,
    });
    assert!(v1, "expected a V1 enum value");
    assert!(fields(&tx, "ask_prices").iter().any(
        |v| matches!(v, Value::Vector(items) if items.iter().all(|i| matches!(i, Value::U64(_))))
    ));
    assert!(
        fields(&tx, "handle")
            .iter()
            .any(|v| matches!(v, Value::Address(_)))
    );
}

fn contains_variant(value: &Value, variant: &str) -> bool {
    match value {
        Value::Variant { name, fields } => {
            name.as_str() == variant || fields.iter().any(|(_, v)| contains_variant(v, variant))
        }
        Value::Struct(fields) => fields.iter().any(|(_, v)| contains_variant(v, variant)),
        Value::Vector(items) => items.iter().any(|v| contains_variant(v, variant)),
        Value::Option(Some(v)) => contains_variant(v, variant),
        _ => false,
    }
}

#[test]
fn module_events_and_legacy_handle_events() {
    let (_, tx) = decode_everything("testnet-11196227786");
    let withdraw = tx
        .records
        .iter()
        .find_map(|r| match &r.data {
            RecordData::Event { ty, account, .. }
                if ty.to_string() == "0x1::fungible_asset::Withdraw" =>
            {
                Some(*account)
            }
            _ => None,
        })
        .unwrap();
    assert_eq!(
        withdraw,
        Address::ZERO,
        "module events have no handle account"
    );

    let (_, tx) = decode_everything("testnet-11196227807");
    let new_block = tx
        .records
        .iter()
        .find_map(|r| match &r.data {
            RecordData::Event {
                ty,
                account,
                sequence_number,
                ..
            } if ty.name.name.as_str() == "NewBlockEvent" => Some((*account, *sequence_number)),
            _ => None,
        })
        .unwrap();
    assert_ne!(new_block.0, Address::ZERO);
    assert!(
        new_block.1 > 0,
        "legacy handle events carry a real sequence number"
    );
}

#[test]
fn generic_resources_and_events() {
    let (_, tx) = decode_everything("testnet-6000000000");
    assert!(tx.records.iter().any(|r| matches!(&r.data,
        RecordData::ResourceWrite { ty, .. }
            if ty.to_string() == "0x1::coin::CoinStore<0x1::aptos_coin::AptosCoin>")));

    let (_, tx) = decode_everything("mainnet-7205748025");
    assert!(tx.records.iter().any(|r| matches!(&r.data,
        RecordData::Event { ty, .. } if ty.name.name.as_str() == "WitnessDropEvent" && !ty.type_args.is_empty())));
}

#[test]
fn table_items_of_every_container() {
    let containers = |name: &str| -> BTreeSet<String> {
        decode_everything(name)
            .1
            .records
            .iter()
            .filter_map(|r| match &r.data {
                RecordData::TableWrite { container, .. } => Some(format!("write {container:?}")),
                RecordData::TableDelete { container, .. } => Some(format!("delete {container:?}")),
                _ => None,
            })
            .collect()
    };
    assert!(containers("testnet-11196231182").contains("write SmartTable"));
    assert!(containers("mainnet-7205730568").contains("write SmartTable"));
    assert!(containers("testnet-11196227787").contains("write BigOrderedMap"));
    assert!(containers("mainnet-7205730378").contains("write BigOrderedMap"));
    assert!(containers("mainnet-7205731421").contains("write Table"));
    assert!(containers("testnet-6000029471").contains("delete Table"));

    // A delete's key is decoded from its JSON, even when the key is a struct.
    let (_, tx) = decode_everything("testnet-6000029471");
    assert!(tx.records.iter().any(|r| matches!(
        &r.data,
        RecordData::TableDelete {
            key: Value::Struct(_),
            ..
        }
    )));
}

#[test]
fn materialized_aggregators() {
    let (_, tx) = decode_everything("mainnet-7205730378");
    assert!(!fields(&tx, "max_value").is_empty());
}

#[test]
fn object_deletes_route_to_group_members() {
    let (_, tx) = decode_everything("mainnet-7205805457");
    let group_deletes: Vec<_> = tx
        .records
        .iter()
        .filter_map(|r| match &r.data {
            RecordData::GroupDelete { address, group } => Some((*address, group.to_string())),
            _ => None,
        })
        .collect();
    assert!(
        !group_deletes.is_empty(),
        "expected the ObjectGroup delete to reach ObjectCore"
    );
    assert!(
        group_deletes
            .iter()
            .all(|(_, g)| g == "0x1::object::ObjectGroup")
    );
}

#[test]
fn failed_transactions_still_have_records() {
    let (_, tx) = decode_everything("testnet-11196281560");
    assert!(!tx.success);
    assert!(tx.records.iter().any(|r| matches!(&r.data,
        RecordData::Event { ty, .. } if ty.to_string() == "0x1::transaction_fee::FeeStatement")));
    assert!(
        tx.records
            .iter()
            .any(|r| matches!(r.data, RecordData::ResourceWrite { .. }))
    );
}

/// The record log replays a project's own records instead of re-reading the chain
/// (ADR 0022), so every record has to survive a round trip through its stored shape.
/// Running it over the whole fixture corpus means every rendering convention the
/// decoder handles is also a convention the log can store.
#[test]
fn every_record_survives_the_record_log() {
    let mut checked = 0_usize;
    for name in all_fixture_names() {
        let (_, tx) = decode_everything(&name);
        for record in &tx.records {
            let stored = StoredRecord::from(record);
            let json = serde_json::to_string(&stored)
                .unwrap_or_else(|e| panic!("{name}: serializing {record:?}: {e}"));
            let back: StoredRecord = serde_json::from_str(&json)
                .unwrap_or_else(|e| panic!("{name}: reading back {json}: {e}"));
            let record_back = Record::try_from(back)
                .unwrap_or_else(|e| panic!("{name}: converting back {json}: {e}"));
            assert_eq!(
                &record_back, record,
                "{name}: a record changed on its way through the log"
            );
            checked += 1;
        }
    }
    assert!(
        checked >= 100,
        "only {checked} records round-tripped; the fixture corpus should be richer than that"
    );
}
