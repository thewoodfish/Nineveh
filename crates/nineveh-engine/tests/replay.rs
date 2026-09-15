//! The M1 headline property (ADR 0005): for any workload, any batch boundaries and any
//! crash points, the committed state and change feed equal a single clean pass, and
//! both equal what a model of the contract says.
//!
//! Workloads are random operations on a synthetic vault contract, rendered as real
//! Transaction Stream messages (JSON in the fullnode's conventions) and run through the
//! decoder, so this exercises decode and fold together:
//!
//! - deposit and withdraw events folded into balances;
//! - a vault resource mirrored, created and deleted;
//! - a `Table<address, Position>` whose items are written and deleted;
//! - a `SmartTable<address, u64>` whose buckets are rewritten, including splits that move
//!   entries between buckets;
//! - items written to foreign tables with the same types, which must be ignored.

#![allow(
    clippy::unwrap_used,
    clippy::panic,
    reason = "test-only crate: helpers panic on unexpected results"
)]

use std::collections::{BTreeMap, BTreeSet};

use nineveh_config::{Project, parse};
use nineveh_core::{Address, Value};
use nineveh_decode::{DecodedTransaction, LockBuilder, Lockfile, ModuleAbi, TransactionDecoder};
use nineveh_engine::{
    ChangeSet, Engine, FoldError, Key, Lookup, MemoryState, RowChange, StateView, TableId,
};
use nineveh_proto::transaction::{
    DeleteResource, DeleteTableData, DeleteTableItem, Event, EventKey, Transaction,
    TransactionInfo, UserTransaction, WriteResource, WriteSetChange, WriteTableData,
    WriteTableItem, transaction::TxnData, write_set_change::Change,
};
use proptest::prelude::*;

const MODULE: &str = "0x000000000000000000000000000000000000000000000000000000000000cafe";

const CONFIG: &str = r#"
name: vault
network: testnet
sources:
  deposits:    { event: 0xcafe::vault::DepositEvent }
  withdrawals: { event: 0xcafe::vault::WithdrawEvent }
  vaults:      { resource: 0xcafe::vault::Vault }
  positions:   { table: 0xcafe::vault::Vault.positions }
  shares:      { table: 0xcafe::vault::Vault.shares }
state:
  balances:
    key: [user]
    columns:
      user: address
      balance: { type: u128, default: 0 }
      deposits: { type: u64, default: 0 }
    reduce:
      - { on: deposits, set: { balance: "balance + u128(amount)", deposits: "deposits + 1" } }
      - { on: withdrawals, set: { balance: "balance - u128(amount)" } }
  vaults: { mirror: vaults }
  positions: { mirror: positions }
  shares: { mirror: shares }
  deposit_log: { log: deposits }
  sizes:
    key: [user]
    columns: { user: address, size: u64 }
    reduce:
      - { on: positions, key: { user: "key" }, set: { size: "value.size" } }
      - { on: positions.deleted, key: { user: "key" }, delete: true }
  share_writes:
    key: [user]
    columns: { user: address, n: { type: u64, default: 0 } }
    reduce:
      - { on: shares, key: { user: "key" }, set: { n: "n + 1" } }
"#;

/// ABIs in the fullnode REST shape: the vault module plus the framework structs its
/// tables use (copied from mainnet's).
fn modules() -> Vec<ModuleAbi> {
    let abi = |json: &str| serde_json::from_str::<ModuleAbi>(json).unwrap();
    vec![
        abi(r#"{"address": "0xcafe", "name": "vault", "structs": [
          {"name": "DepositEvent", "is_event": true, "abilities": ["drop", "store"], "generic_type_params": [],
           "fields": [{"name": "user", "type": "address"}, {"name": "amount", "type": "u64"}]},
          {"name": "WithdrawEvent", "is_event": true, "abilities": ["drop", "store"], "generic_type_params": [],
           "fields": [{"name": "user", "type": "address"}, {"name": "amount", "type": "u64"}]},
          {"name": "Position", "abilities": ["copy", "drop", "store"], "generic_type_params": [],
           "fields": [{"name": "size", "type": "u64"}, {"name": "entry", "type": "u64"}]},
          {"name": "Vault", "abilities": ["key"], "generic_type_params": [],
           "fields": [{"name": "owner", "type": "address"},
                      {"name": "positions", "type": "0x1::table::Table<address, 0xcafe::vault::Position>"},
                      {"name": "shares", "type": "0x1::smart_table::SmartTable<address, u64>"}]}
        ]}"#),
        abi(r#"{"address": "0x1", "name": "table", "structs": [
          {"name": "Table", "abilities": ["store"], "generic_type_params": [{"constraints": []}, {"constraints": []}],
           "fields": [{"name": "handle", "type": "address"}]}]}"#),
        abi(
            r#"{"address": "0x1", "name": "table_with_length", "structs": [
          {"name": "TableWithLength", "abilities": ["store"], "generic_type_params": [{"constraints": []}, {"constraints": []}],
           "fields": [{"name": "inner", "type": "0x1::table::Table<T0, T1>"}, {"name": "length", "type": "u64"}]}]}"#,
        ),
        abi(r#"{"address": "0x1", "name": "smart_table", "structs": [
          {"name": "Entry", "abilities": ["copy", "drop", "store"], "generic_type_params": [{"constraints": []}, {"constraints": []}],
           "fields": [{"name": "hash", "type": "u64"}, {"name": "key", "type": "T0"}, {"name": "value", "type": "T1"}]},
          {"name": "SmartTable", "abilities": ["store"], "generic_type_params": [{"constraints": []}, {"constraints": []}],
           "fields": [{"name": "buckets", "type": "0x1::table_with_length::TableWithLength<u64, vector<0x1::smart_table::Entry<T0, T1>>>"},
                      {"name": "num_buckets", "type": "u64"}, {"name": "level", "type": "u8"}, {"name": "size", "type": "u64"},
                      {"name": "split_load_threshold", "type": "u8"}, {"name": "target_bucket_size", "type": "u64"}]}]}"#),
    ]
}

fn project() -> (Lockfile, Project) {
    let config = parse(CONFIG).unwrap_or_else(|d| panic!("{}", d.render("nineveh.yaml", CONFIG)));
    let mut builder = LockBuilder::new(config.network);
    for module in modules() {
        builder.add_module(module);
    }
    let lock = builder.build(&config.roots()).unwrap();
    let project = config
        .resolve(&lock)
        .unwrap_or_else(|d| panic!("{}", d.render("nineveh.yaml", CONFIG)));
    (lock, project)
}

// --- the workload ------------------------------------------------------------------

#[derive(Debug, Clone)]
enum Op {
    Deposit {
        user: u8,
        amount: u32,
    },
    Withdraw {
        user: u8,
        amount: u32,
    },
    CreateVault {
        vault: u8,
    },
    DeleteVault {
        vault: u8,
    },
    SetPosition {
        vault: u8,
        user: u8,
        size: u32,
    },
    DeletePosition {
        vault: u8,
        user: u8,
    },
    SetShare {
        vault: u8,
        user: u8,
        amount: u8,
    },
    RemoveShare {
        vault: u8,
        user: u8,
    },
    /// Move every share entry to the other bucket in one transaction, as a split does.
    Rebucket {
        vault: u8,
    },
    /// A `Table<address, Position>` item in a table no vault owns.
    Foreign {
        user: u8,
        size: u32,
    },
}

fn op() -> impl Strategy<Value = Op> {
    let (v, u) = (0u8..3, 0u8..4);
    prop_oneof![
        (u.clone(), 0u32..1000).prop_map(|(user, amount)| Op::Deposit { user, amount }),
        (u.clone(), 0u32..1000).prop_map(|(user, amount)| Op::Withdraw { user, amount }),
        v.clone().prop_map(|vault| Op::CreateVault { vault }),
        v.clone().prop_map(|vault| Op::DeleteVault { vault }),
        (v.clone(), u.clone(), 1u32..100).prop_map(|(vault, user, size)| Op::SetPosition {
            vault,
            user,
            size
        }),
        (v.clone(), u.clone()).prop_map(|(vault, user)| Op::DeletePosition { vault, user }),
        (v.clone(), u.clone(), 0u8..3).prop_map(|(vault, user, amount)| Op::SetShare {
            vault,
            user,
            amount
        }),
        (v.clone(), u.clone()).prop_map(|(vault, user)| Op::RemoveShare { vault, user }),
        v.prop_map(|vault| Op::Rebucket { vault }),
        (u, 1u32..100).prop_map(|(user, size)| Op::Foreign { user, size }),
    ]
}

fn user(u: u8) -> Address {
    Address::special(0x10 + u)
}

fn vault_address(v: u8) -> Address {
    addr(0x3000 + u64::from(v))
}

fn positions_handle(v: u8) -> Address {
    addr(0x1000 + u64::from(v))
}

fn shares_handle(v: u8) -> Address {
    addr(0x2000 + u64::from(v))
}

fn addr(n: u64) -> Address {
    format!("{n:#x}").parse().unwrap()
}

/// The contract's state, tracked alongside the transactions that change it. Its
/// projection is what the fold must produce.
#[derive(Debug, Default)]
struct Model {
    balances: BTreeMap<Address, (u128, u64)>,
    vaults: BTreeSet<u8>,
    positions: BTreeMap<(Address, Address), u32>,
    /// Per vault: user to (amount, bucket).
    shares: BTreeMap<u8, BTreeMap<u8, (u8, u64)>>,
    share_writes: BTreeMap<Address, u64>,
    sizes: BTreeMap<Address, u32>,
    deposits: usize,
}

/// Render the ops as stream transactions, skipping ops the contract would reject, and
/// build the model as we go.
#[allow(clippy::too_many_lines, reason = "one arm per workload operation")]
fn transactions(ops: &[Op]) -> (Vec<Transaction>, Model) {
    let mut model = Model::default();
    let mut txs = Vec::new();
    let mut version = 1_000u64;
    for op in ops {
        let mut events = Vec::new();
        let mut changes = Vec::new();
        match *op {
            Op::Deposit { user: u, amount } => {
                let entry = model.balances.entry(user(u)).or_default();
                entry.0 += u128::from(amount);
                entry.1 += 1;
                model.deposits += 1;
                events.push(event("DepositEvent", u, amount));
            }
            Op::Withdraw { user: u, amount } => {
                let balance = model.balances.get(&user(u)).map_or(0, |b| b.0);
                let amount = u32::try_from(balance.min(u128::from(amount))).unwrap();
                model.balances.entry(user(u)).or_default().0 -= u128::from(amount);
                events.push(event("WithdrawEvent", u, amount));
            }
            Op::CreateVault { vault } => {
                if !model.vaults.insert(vault) {
                    continue;
                }
                // An opening position in the same transaction, ahead of the vault that
                // reveals its table's handle.
                model
                    .positions
                    .insert((positions_handle(vault), user(0)), 1);
                model.sizes.insert(user(0), 1);
                changes.push(position_write(positions_handle(vault), 0, 1));
                changes.push(vault_write(vault, &model));
            }
            Op::DeleteVault { vault } => {
                if !model.vaults.remove(&vault) {
                    continue;
                }
                changes.push(Change::DeleteResource(DeleteResource {
                    address: vault_address(vault).to_string(),
                    type_str: format!("{MODULE}::vault::Vault"),
                    ..Default::default()
                }));
            }
            Op::SetPosition {
                vault,
                user: u,
                size,
            } => {
                if !model.vaults.contains(&vault) {
                    continue;
                }
                model
                    .positions
                    .insert((positions_handle(vault), user(u)), size);
                model.sizes.insert(user(u), size);
                changes.push(position_write(positions_handle(vault), u, size));
            }
            Op::DeletePosition { vault, user: u } => {
                if !model.vaults.contains(&vault)
                    || model
                        .positions
                        .remove(&(positions_handle(vault), user(u)))
                        .is_none()
                {
                    continue;
                }
                model.sizes.remove(&user(u));
                changes.push(Change::DeleteTableItem(DeleteTableItem {
                    handle: positions_handle(vault).to_string(),
                    data: Some(DeleteTableData {
                        key: format!("\"{}\"", user(u).to_standard_string()),
                        key_type: "address".into(),
                    }),
                    ..Default::default()
                }));
            }
            Op::SetShare {
                vault,
                user: u,
                amount,
            } => {
                if !model.vaults.contains(&vault) {
                    continue;
                }
                let shares = model.shares.entry(vault).or_default();
                let bucket = shares.get(&u).map_or(u64::from(u % 2), |s| s.1);
                if shares.get(&u).map(|s| s.0) != Some(amount) {
                    *model.share_writes.entry(user(u)).or_default() += 1;
                }
                shares.insert(u, (amount, bucket));
                // Write sets are ordered by state key, not meaning: the item can come
                // before the parent that reveals its handle.
                changes.push(bucket_write(vault, bucket, &model));
                changes.push(vault_write(vault, &model));
            }
            Op::RemoveShare { vault, user: u } => {
                if !model.vaults.contains(&vault) {
                    continue;
                }
                let Some(bucket) = model
                    .shares
                    .get_mut(&vault)
                    .and_then(|s| s.remove(&u))
                    .map(|s| s.1)
                else {
                    continue;
                };
                changes.push(vault_write(vault, &model));
                changes.push(bucket_write(vault, bucket, &model));
            }
            Op::Rebucket { vault } => {
                if !model.vaults.contains(&vault) {
                    continue;
                }
                let Some(shares) = model.shares.get_mut(&vault).filter(|s| !s.is_empty()) else {
                    continue;
                };
                for share in shares.values_mut() {
                    share.1 = 1 - share.1;
                }
                changes.push(bucket_write(vault, 0, &model));
                changes.push(bucket_write(vault, 1, &model));
            }
            Op::Foreign { user: u, size } => {
                changes.push(position_write(addr(0xdead), u, size));
            }
        }
        version += 1;
        txs.push(transaction(version, events, changes));
    }
    (txs, model)
}

fn event(name: &str, u: u8, amount: u32) -> Event {
    Event {
        key: Some(EventKey {
            creation_number: 0,
            account_address: "0x0".into(),
        }),
        type_str: format!("{MODULE}::vault::{name}"),
        data: format!(
            r#"{{"user":"{}","amount":"{amount}"}}"#,
            user(u).to_standard_string()
        ),
        ..Default::default()
    }
}

fn vault_write(vault: u8, model: &Model) -> Change {
    let size = model.shares.get(&vault).map_or(0, BTreeMap::len);
    Change::WriteResource(WriteResource {
        address: vault_address(vault).to_string(),
        type_str: format!("{MODULE}::vault::Vault"),
        data: format!(
            r#"{{"owner":"0x1","positions":{{"handle":"{}"}},"shares":{{"buckets":{{"inner":{{"handle":"{}"}},"length":"2"}},"level":1,"num_buckets":"2","size":"{size}","split_load_threshold":75,"target_bucket_size":"4"}}}}"#,
            positions_handle(vault),
            shares_handle(vault),
        ),
        ..Default::default()
    })
}

fn position_write(handle: Address, u: u8, size: u32) -> Change {
    Change::WriteTableItem(WriteTableItem {
        handle: handle.to_string(),
        data: Some(WriteTableData {
            key: format!("\"{}\"", user(u).to_standard_string()),
            key_type: "address".into(),
            value: format!(r#"{{"size":"{size}","entry":"7"}}"#),
            value_type: format!("{MODULE}::vault::Position"),
        }),
        ..Default::default()
    })
}

/// A bucket's full contents, or a delete when it's empty.
fn bucket_write(vault: u8, bucket: u64, model: &Model) -> Change {
    let entries: Vec<String> = model
        .shares
        .get(&vault)
        .into_iter()
        .flatten()
        .filter(|(_, s)| s.1 == bucket)
        .map(|(u, s)| {
            format!(
                r#"{{"hash":"{}","key":"{}","value":"{}"}}"#,
                u64::from(*u) * 7919,
                user(*u).to_standard_string(),
                s.0
            )
        })
        .collect();
    if entries.is_empty() {
        return Change::DeleteTableItem(DeleteTableItem {
            handle: shares_handle(vault).to_string(),
            data: Some(DeleteTableData {
                key: format!("\"{bucket}\""),
                key_type: "u64".into(),
            }),
            ..Default::default()
        });
    }
    Change::WriteTableItem(WriteTableItem {
        handle: shares_handle(vault).to_string(),
        data: Some(WriteTableData {
            key: format!("\"{bucket}\""),
            key_type: "u64".into(),
            value: format!("[{}]", entries.join(",")),
            value_type: "vector<0x1::smart_table::Entry<address, u64>>".into(),
        }),
        ..Default::default()
    })
}

fn transaction(version: u64, events: Vec<Event>, changes: Vec<Change>) -> Transaction {
    Transaction {
        version,
        info: Some(TransactionInfo {
            success: true,
            changes: changes
                .into_iter()
                .map(|change| WriteSetChange {
                    change: Some(change),
                    ..Default::default()
                })
                .collect(),
            ..Default::default()
        }),
        txn_data: Some(TxnData::User(UserTransaction {
            request: None,
            events,
        })),
        ..Default::default()
    }
}

// --- folding strategies -----------------------------------------------------------

fn decode(lock: &Lockfile, project: &Project, txs: &[Transaction]) -> Vec<DecodedTransaction> {
    let decoder = TransactionDecoder::new(lock, project.selection());
    txs.iter().map(|tx| decoder.decode(tx).unwrap()).collect()
}

/// The committed state and the committed change feed.
type Outcome = (MemoryState, Vec<RowChange>);

fn single_pass(engine: &Engine<'_>, txs: &[DecodedTransaction]) -> Result<Outcome, FoldError> {
    let mut state = MemoryState::new();
    let changes = engine.fold(&state, txs)?;
    state.apply(&changes);
    Ok((state, changes.changes))
}

/// Fold in batches of the given sizes (cycled). A `crash` discards a folded batch
/// before commit; the loop resumes from the committed cursor with the next size.
fn chunked(
    engine: &Engine<'_>,
    txs: &[DecodedTransaction],
    sizes: &[usize],
    crashes: &[bool],
) -> Result<Outcome, FoldError> {
    let mut state = MemoryState::new();
    let mut feed = Vec::new();
    let mut committed = 0;
    let mut step = 0;
    while committed < txs.len() {
        let size = sizes[step % sizes.len()].max(1);
        let crash = crashes.get(step).copied().unwrap_or(false);
        step += 1;
        let end = (committed + size).min(txs.len());
        let changes: ChangeSet = engine.fold(&state, &txs[committed..end])?;
        if crash {
            continue;
        }
        state.apply(&changes);
        feed.extend(changes.changes);
        committed = end;
        assert_eq!(state.cursor(), Some(txs[end - 1].version));
    }
    Ok((state, feed))
}

/// A view that starts empty and serves only the keys it's been given, as a
/// database-backed view does after preloading.
struct Preloaded<'a> {
    state: &'a MemoryState,
    loaded: BTreeSet<(TableId, Key)>,
}

impl StateView for Preloaded<'_> {
    fn get(&self, table: TableId, key: &[Value]) -> Lookup {
        if self.loaded.contains(&(table, key.to_vec())) {
            self.state.get(table, key)
        } else {
            Lookup::NotLoaded
        }
    }
}

/// Fold each transaction through a view that must be filled from `NotLoaded` retries.
fn with_retries(engine: &Engine<'_>, txs: &[DecodedTransaction]) -> Result<Outcome, FoldError> {
    let mut state = MemoryState::new();
    let mut feed = Vec::new();
    for tx in txs {
        let mut loaded = BTreeSet::new();
        let changes = loop {
            let view = Preloaded {
                state: &state,
                loaded: loaded.clone(),
            };
            match engine.fold(&view, std::slice::from_ref(tx)) {
                Err(FoldError::NotLoaded(keys)) => {
                    assert!(
                        keys.iter().any(|k| !loaded.contains(k)),
                        "a retry must make progress"
                    );
                    loaded.extend(keys);
                }
                other => break other?,
            }
        };
        state.apply(&changes);
        feed.extend(changes.changes);
    }
    Ok((state, feed))
}

// --- expectations -----------------------------------------------------------------

fn rows(state: &MemoryState, table: u32) -> Vec<(Key, Vec<Value>)> {
    state
        .rows(TableId::State(table))
        .map(|(k, r)| (k.clone(), r.clone()))
        .collect()
}

fn check_against_model(state: &MemoryState, model: &Model) {
    let balances: BTreeMap<Address, (u128, u64)> = rows(state, 0)
        .into_iter()
        .map(|(_, r)| match r.as_slice() {
            [Value::Address(a), Value::U128(b), Value::U64(n)] => (*a, (*b, *n)),
            other => panic!("unexpected balances row {other:?}"),
        })
        .collect();
    assert_eq!(balances, model.balances, "balances");

    let vaults: BTreeSet<Address> = rows(state, 1)
        .into_iter()
        .map(|(k, _)| match k[0] {
            Value::Address(a) => a,
            _ => panic!(),
        })
        .collect();
    let expected_vaults: BTreeSet<Address> =
        model.vaults.iter().map(|v| vault_address(*v)).collect();
    assert_eq!(vaults, expected_vaults, "vaults");

    let positions: BTreeMap<(Address, Address), u32> = rows(state, 2)
        .into_iter()
        .map(|(k, r)| match (k.as_slice(), r.last()) {
            ([Value::Address(h), Value::Address(u)], Some(value)) => {
                let Some(Value::U64(size)) = value.field("size") else {
                    panic!("position value {value:?}")
                };
                ((*h, *u), u32::try_from(*size).unwrap())
            }
            other => panic!("unexpected positions row {other:?}"),
        })
        .collect();
    assert_eq!(
        positions, model.positions,
        "positions (and no foreign items)"
    );

    let shares: BTreeMap<(Address, Address), u64> = rows(state, 3)
        .into_iter()
        .map(|(k, r)| match (k.as_slice(), r.last()) {
            ([Value::Address(h), Value::Address(u)], Some(Value::U64(amount))) => {
                ((*h, *u), *amount)
            }
            other => panic!("unexpected shares row {other:?}"),
        })
        .collect();
    let expected_shares: BTreeMap<(Address, Address), u64> = model
        .shares
        .iter()
        .flat_map(|(v, s)| {
            s.iter()
                .map(move |(u, (amount, _))| ((shares_handle(*v), user(*u)), u64::from(*amount)))
        })
        .collect();
    assert_eq!(shares, expected_shares, "shares");

    assert_eq!(rows(state, 4).len(), model.deposits, "deposit log");

    let sizes: BTreeMap<Address, u32> = rows(state, 5)
        .into_iter()
        .map(|(_, r)| match r.as_slice() {
            [Value::Address(u), Value::U64(s)] => (*u, u32::try_from(*s).unwrap()),
            other => panic!("unexpected sizes row {other:?}"),
        })
        .collect();
    assert_eq!(sizes, model.sizes, "sizes");

    let share_writes: BTreeMap<Address, u64> = rows(state, 6)
        .into_iter()
        .map(|(_, r)| match r.as_slice() {
            [Value::Address(u), Value::U64(n)] => (*u, *n),
            other => panic!("unexpected share_writes row {other:?}"),
        })
        .collect();
    assert_eq!(
        share_writes, model.share_writes,
        "share writes: bucket moves must not count as writes"
    );
}

proptest! {
    #![proptest_config(ProptestConfig {
        cases: 64,
        failure_persistence: None,
        ..ProptestConfig::default()
    })]

    #[test]
    fn any_batching_and_any_crashes_give_the_same_state(
        ops in proptest::collection::vec(op(), 1..80),
        sizes in proptest::collection::vec(1usize..12, 1..8),
        crashes in proptest::collection::vec(any::<bool>(), 0..16),
    ) {
        let (lock, project) = project();
        let engine = Engine::new(&project);
        let (txs, model) = transactions(&ops);
        let decoded = decode(&lock, &project, &txs);

        let clean = single_pass(&engine, &decoded).unwrap();
        check_against_model(&clean.0, &model);

        let replayed = chunked(&engine, &decoded, &sizes, &crashes).unwrap();
        prop_assert_eq!(&replayed.0, &clean.0, "state differs after batching/crashes");
        prop_assert_eq!(&replayed.1, &clean.1, "change feed differs after batching/crashes");

        let retried = with_retries(&engine, &decoded).unwrap();
        prop_assert_eq!(&retried.0, &clean.0, "state differs when keys load lazily");
        prop_assert_eq!(&retried.1, &clean.1, "change feed differs when keys load lazily");
    }
}

#[test]
fn an_underflow_halts_at_the_same_version_however_it_is_batched() {
    let (lock, project) = project();
    let engine = Engine::new(&project);
    let (mut txs, _) = transactions(&[
        Op::Deposit {
            user: 1,
            amount: 10,
        },
        Op::Deposit { user: 2, amount: 5 },
    ]);
    // Withdraw more than was deposited: the contract would abort, but a reducer that
    // disagrees with the chain must halt rather than invent a balance.
    txs.push(transaction(
        1_003,
        vec![event("WithdrawEvent", 1, 11)],
        vec![],
    ));
    txs.push(transaction(
        1_004,
        vec![event("DepositEvent", 1, 1)],
        vec![],
    ));
    let decoded = decode(&lock, &project, &txs);

    let halt = |result: Result<Outcome, FoldError>| match result {
        Err(FoldError::Halt(halt)) => *halt,
        other => panic!("expected a halt, got {other:?}"),
    };
    let once = halt(single_pass(&engine, &decoded));
    let batched = halt(chunked(&engine, &decoded, &[1], &[]));
    assert_eq!(once, batched);
    assert_eq!(once.version.get(), 1_003);
    assert_eq!(once.table.as_deref(), Some("balances"));
    assert_eq!(
        once.message,
        "`balance - u128(amount)`: `-` overflowed: the result doesn't fit in u128"
    );
    // The span points into nineveh.yaml at the failing subtraction.
    let span = once.span.unwrap();
    assert_eq!(
        &CONFIG[span.offset..span.offset + span.len],
        "balance - u128(amount)"
    );
    assert!(!FoldError::Halt(Box::new(once)).is_retryable());
}
