//! A synthetic vault contract, rendered as real stream transactions:
//!
//! - deposit and withdraw events folded into balances;
//! - a vault resource mirrored, created and deleted;
//! - a `Table<address, Position>` whose items are written and deleted;
//! - a `SmartTable<address, u64>` whose buckets are rewritten, including splits that move
//!   entries between buckets;
//! - items written to foreign tables with the same types, which must be ignored.

use std::collections::{BTreeMap, BTreeSet};

use nineveh_config::{Project, parse};
use nineveh_core::{Address, Value};
use nineveh_decode::{DecodedTransaction, LockBuilder, Lockfile, ModuleAbi, TransactionDecoder};
use nineveh_proto::transaction::{
    DeleteResource, DeleteTableData, DeleteTableItem, Event, EventKey, Transaction,
    TransactionInfo, UserTransaction, WriteResource, WriteSetChange, WriteTableData,
    WriteTableItem, transaction::TxnData, write_set_change::Change,
};
use proptest::prelude::*;

pub const MODULE: &str = "0x000000000000000000000000000000000000000000000000000000000000cafe";

pub const CONFIG: &str = r#"
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
pub fn modules() -> Vec<ModuleAbi> {
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

pub fn project() -> (Lockfile, Project) {
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

#[derive(Debug, Clone)]
pub enum Op {
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

pub fn op() -> impl Strategy<Value = Op> {
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

pub fn user(u: u8) -> Address {
    Address::special(0x10 + u)
}

pub fn vault_address(v: u8) -> Address {
    addr(0x3000 + u64::from(v))
}

pub fn positions_handle(v: u8) -> Address {
    addr(0x1000 + u64::from(v))
}

pub fn shares_handle(v: u8) -> Address {
    addr(0x2000 + u64::from(v))
}

pub fn addr(n: u64) -> Address {
    format!("{n:#x}").parse().unwrap()
}

/// The contract's state, tracked alongside the transactions that change it. Its
/// projection is what the fold must produce.
#[derive(Debug, Default)]
pub struct Model {
    pub balances: BTreeMap<Address, (u128, u64)>,
    pub vaults: BTreeSet<u8>,
    pub positions: BTreeMap<(Address, Address), u32>,
    /// Per vault: user to (amount, bucket).
    pub shares: BTreeMap<u8, BTreeMap<u8, (u8, u64)>>,
    pub share_writes: BTreeMap<Address, u64>,
    pub sizes: BTreeMap<Address, u32>,
    pub deposits: usize,
}

/// Render the ops as stream transactions, skipping ops the contract would reject, and
/// build the model as we go.
#[allow(clippy::too_many_lines, reason = "one arm per workload operation")]
pub fn transactions(ops: &[Op]) -> (Vec<Transaction>, Model) {
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

pub fn event(name: &str, u: u8, amount: u32) -> Event {
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

pub fn vault_write(vault: u8, model: &Model) -> Change {
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

pub fn position_write(handle: Address, u: u8, size: u32) -> Change {
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
pub fn bucket_write(vault: u8, bucket: u64, model: &Model) -> Change {
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

pub fn transaction(version: u64, events: Vec<Event>, changes: Vec<Change>) -> Transaction {
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

/// Decode `txs` with the project's selection.
pub fn decode(lock: &Lockfile, project: &Project, txs: &[Transaction]) -> Vec<DecodedTransaction> {
    let decoder = TransactionDecoder::new(lock, project.selection());
    txs.iter().map(|tx| decoder.decode(tx).unwrap()).collect()
}

/// A state table's rows, as (key, row) pairs in any order.
pub type Rows = Vec<(Vec<Value>, Vec<Value>)>;

impl Model {
    /// Check a projection's tables against the model. `rows(i)` returns the rows of
    /// the `i`th state table in [`CONFIG`].
    #[allow(clippy::too_many_lines, reason = "one block per state table")]
    pub fn check(&self, rows: impl Fn(u32) -> Rows) {
        let model = self;

        let balances: BTreeMap<Address, (u128, u64)> = rows(0)
            .into_iter()
            .map(|(_, r)| match r.as_slice() {
                [Value::Address(a), Value::U128(b), Value::U64(n)] => (*a, (*b, *n)),
                other => panic!("unexpected balances row {other:?}"),
            })
            .collect();
        assert_eq!(balances, model.balances, "balances");

        let vaults: BTreeSet<Address> = rows(1)
            .into_iter()
            .map(|(k, _)| match k[0] {
                Value::Address(a) => a,
                _ => panic!(),
            })
            .collect();
        let expected_vaults: BTreeSet<Address> =
            model.vaults.iter().map(|v| vault_address(*v)).collect();
        assert_eq!(vaults, expected_vaults, "vaults");

        let positions: BTreeMap<(Address, Address), u32> = rows(2)
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

        let shares: BTreeMap<(Address, Address), u64> = rows(3)
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
                s.iter().map(move |(u, (amount, _))| {
                    ((shares_handle(*v), user(*u)), u64::from(*amount))
                })
            })
            .collect();
        assert_eq!(shares, expected_shares, "shares");

        assert_eq!(rows(4).len(), model.deposits, "deposit log");

        let sizes: BTreeMap<Address, u32> = rows(5)
            .into_iter()
            .map(|(_, r)| match r.as_slice() {
                [Value::Address(u), Value::U64(s)] => (*u, u32::try_from(*s).unwrap()),
                other => panic!("unexpected sizes row {other:?}"),
            })
            .collect();
        assert_eq!(sizes, model.sizes, "sizes");

        let share_writes: BTreeMap<Address, u64> = rows(6)
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
}
