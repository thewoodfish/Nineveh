//! The fold on real mainnet transactions from `fixtures/`.

#![allow(
    clippy::unwrap_used,
    clippy::panic,
    reason = "test-only crate: helpers panic on unexpected results"
)]

use std::fs;
use std::path::{Path, PathBuf};

use nineveh_config::{Project, parse};
use nineveh_core::{Address, Value};
use nineveh_decode::{LockBuilder, Lockfile, ModuleAbi, TransactionDecoder};
use nineveh_engine::{Engine, MemoryState, TableId};
use nineveh_proto::transaction::Transaction;
use prost::Message;

const PERP: &str = "0x50ead22afd6ffd9769e3b3d6e0e64a2a350d68e8b102c4e72e33d0b8cfdfdb06";

fn fixtures() -> PathBuf {
    Path::new(env!("CARGO_MANIFEST_DIR")).join("../../fixtures")
}

fn project(yaml: &str) -> (Lockfile, Project) {
    let config = parse(yaml).unwrap_or_else(|d| panic!("{}", d.render("nineveh.yaml", yaml)));
    let mut builder = LockBuilder::new(config.network);
    for entry in fs::read_dir(fixtures().join("abi").join(config.network.as_str())).unwrap() {
        let text = fs::read_to_string(entry.unwrap().path()).unwrap();
        builder.add_module(serde_json::from_str::<ModuleAbi>(&text).unwrap());
    }
    let lock = builder.build(&config.roots()).unwrap();
    let project = config
        .resolve(&lock)
        .unwrap_or_else(|d| panic!("{}", d.render("nineveh.yaml", yaml)));
    (lock, project)
}

fn transaction(name: &str) -> Transaction {
    let bytes = fs::read(fixtures().join(format!("mainnet/{name}.pb"))).unwrap();
    Transaction::decode(bytes.as_slice()).unwrap()
}

#[test]
fn identical_tables_are_told_apart_by_handle() {
    // TradingVolumeBucket holds two Table<address, VolumeHistory>. In this
    // transaction the bucket is written (as a table value) with both handles, and one
    // VolumeHistory item lands in each table.
    let yaml = format!(
        "\
name: perp
network: mainnet
sources:
  taker: {{ table: {PERP}::trading_volume_tracker::TradingVolumeBucket.user_taker_volume_history }}
  maker: {{ table: {PERP}::trading_volume_tracker::TradingVolumeBucket.user_maker_volume_history }}
state:
  taker_volume: {{ mirror: taker }}
  maker_volume: {{ mirror: maker }}
"
    );
    let (lock, project) = project(&yaml);
    let decoded = TransactionDecoder::new(&lock, project.selection())
        .decode(&transaction("mainnet-7205731421"))
        .unwrap();

    let mut state = MemoryState::new();
    let changes = Engine::new(&project).fold(&state, &[decoded]).unwrap();
    state.apply(&changes);

    let handles = |table: u32| -> Vec<Address> {
        state
            .rows(TableId::State(table))
            .map(|(key, _)| match &key[0] {
                Value::Address(h) => *h,
                other => panic!("mirror keys start with the handle, got {other:?}"),
            })
            .collect()
    };
    let taker: Address = "0xb4d37e87a037a4fa2bb2909ed53df23101b28bf9de3ab0ed847ee671ac501b90"
        .parse()
        .unwrap();
    let maker: Address = "0x696d36c111efb55823344fe77b0a5e192c48d9ead92a4446d938bc6052053495"
        .parse()
        .unwrap();
    assert_eq!(
        handles(0),
        [taker],
        "taker volume holds only the taker table's item"
    );
    assert_eq!(
        handles(1),
        [maker],
        "maker volume holds only the maker table's item"
    );
    // Both attributions are internal state, committed with the rows.
    assert_eq!(state.rows(TableId::Handles).count(), 2);
    assert_eq!(changes.last_version.unwrap().get(), 7_205_731_421);
}
