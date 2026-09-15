//! The server-side filter (ADR 0004): which projects get one, and whether its
//! addresses read the way the stream renders event types.

#![allow(
    clippy::unwrap_used,
    clippy::panic,
    clippy::print_stderr,
    reason = "test-only crate: helpers panic on unexpected results and note skips"
)]

use std::fmt::Write as _;
use std::fs;
use std::path::{Path, PathBuf};

use nineveh_config::{Project, parse};
use nineveh_core::Version;
use nineveh_decode::{LockBuilder, ModuleAbi};
use nineveh_ingest::{StreamConfig, TransactionStream};
use nineveh_pipeline::{MAX_FILTER_BYTES, stream_filter, stream_filter_within};
use nineveh_proto::indexer::{BooleanTransactionFilter, api_filter, boolean_transaction_filter};
use nineveh_proto::transaction::Transaction;
use nineveh_proto::transaction::transaction::TxnData;
use nineveh_testkit::vault;
use prost::Message;

/// A contract whose address starts with a zero nibble: the stream writes it with 63
/// digits.
const MARKET: &str = "0x0e3117b978e079073756f6e1aafff9e4fcb028e51612c3a80c20a095fdfd4a02";
/// A testnet transaction that emits `MARKET::user::CreateContractEvent`.
const FIXTURE: u64 = 6_000_029_471;

fn fixtures() -> PathBuf {
    Path::new(env!("CARGO_MANIFEST_DIR")).join("../../fixtures")
}

fn project(yaml: &str) -> Project {
    let config = parse(yaml).unwrap_or_else(|d| panic!("{}", d.render("nineveh.yaml", yaml)));
    let mut builder = LockBuilder::new(config.network);
    for entry in fs::read_dir(fixtures().join("abi").join(config.network.as_str())).unwrap() {
        let text = fs::read_to_string(entry.unwrap().path()).unwrap();
        builder.add_module(serde_json::from_str::<ModuleAbi>(&text).unwrap());
    }
    let lock = builder.build(&config.roots()).unwrap();
    config
        .resolve(&lock)
        .unwrap_or_else(|d| panic!("{}", d.render("nineveh.yaml", yaml)))
}

fn market_events() -> Project {
    project(&format!(
        "\
name: market
network: testnet
sources:
  created: {{ event: {MARKET}::user::CreateContractEvent }}
  closed:  {{ event: {MARKET}::user::CloseContractEvent }}
  taker:   {{ event: {MARKET}::market::TakerEvent }}
  again:   {{ event: {MARKET}::user::CreateContractEvent }}
state:
  created_log: {{ log: created }}
  closed_log:  {{ log: closed }}
  taker_log:   {{ log: taker }}
  again_log:   {{ log: again }}
"
    ))
}

/// The `(address, module, name)` of each event type the filter ORs together.
fn leaves(filter: &BooleanTransactionFilter) -> Vec<(String, String, String)> {
    coarse_leaves(filter)
        .into_iter()
        .map(|(address, module, name)| (address, module.unwrap(), name.unwrap()))
        .collect()
}

/// Each leaf's address, and its module and struct if it names them.
fn coarse_leaves(
    filter: &BooleanTransactionFilter,
) -> Vec<(String, Option<String>, Option<String>)> {
    let Some(boolean_transaction_filter::Filter::LogicalOr(or)) = &filter.filter else {
        panic!("expected an OR of event filters: {filter:?}");
    };
    or.filters
        .iter()
        .map(|leaf| {
            let Some(boolean_transaction_filter::Filter::ApiFilter(api)) = &leaf.filter else {
                panic!("{leaf:?}");
            };
            let Some(api_filter::Filter::EventFilter(event)) = &api.filter else {
                panic!("{api:?}");
            };
            assert_eq!(event.data_substring_filter, None);
            let tag = event.struct_type.clone().unwrap();
            (tag.address.unwrap(), tag.module, tag.name)
        })
        .collect()
}

#[test]
fn event_only_projects_filter_on_their_event_types() {
    let filter = stream_filter(&market_events()).unwrap();
    let short = "0xe3117b978e079073756f6e1aafff9e4fcb028e51612c3a80c20a095fdfd4a02";
    let leaf = |module: &str, name: &str| (short.to_owned(), module.to_owned(), name.to_owned());
    assert_eq!(
        leaves(&filter),
        [
            leaf("market", "TakerEvent"),
            leaf("user", "CloseContractEvent"),
            leaf("user", "CreateContractEvent"),
        ],
        "one leaf per event type, sources sharing a type included once"
    );
}

/// The budgets that make `stream_filter_within` fall back to module leaves, and to
/// address leaves.
fn budgets(project: &Project) -> (usize, usize) {
    let exact = stream_filter(project).unwrap().encoded_len();
    let module = stream_filter_within(project, exact - 1)
        .unwrap()
        .encoded_len();
    (exact - 1, module - 1)
}

#[test]
fn a_filter_too_big_to_send_matches_by_module_then_address() {
    let market = market_events();
    let short = "0xe3117b978e079073756f6e1aafff9e4fcb028e51612c3a80c20a095fdfd4a02";
    let (module_budget, address_budget) = budgets(&market);

    let by_module = stream_filter_within(&market, module_budget).unwrap();
    assert!(by_module.encoded_len() <= module_budget);
    let module = |m: &str| (short.to_owned(), Some(m.to_owned()), None);
    assert_eq!(
        coarse_leaves(&by_module),
        [module("market"), module("user")]
    );

    let by_address = stream_filter_within(&market, address_budget).unwrap();
    assert_eq!(coarse_leaves(&by_address), [(short.to_owned(), None, None)]);

    assert_eq!(
        stream_filter_within(&market, 10),
        None,
        "too big even so: no filter"
    );
}

/// A contract with more event types than fit the server's limit, as a real one did
/// (98 events, 11,781 bytes).
#[test]
fn many_event_types_fit_the_server_limit() {
    let events = 150;
    // Full width, as real contract addresses are.
    let address = format!("0x{}", "5a".repeat(32));
    let structs: Vec<serde_json::Value> = (0..events)
        .map(|i| {
            serde_json::json!({
                "name": format!("SomethingHappenedEvent{i}"), "is_event": true,
                "abilities": ["drop", "store"], "generic_type_params": [],
                "fields": [{ "name": "amount", "type": "u64" }],
            })
        })
        .collect();
    let modules: Vec<ModuleAbi> = ["orders", "positions", "vaults"]
        .iter()
        .enumerate()
        .map(|(m, name)| {
            let part: Vec<_> = structs.iter().skip(m).step_by(3).cloned().collect();
            serde_json::from_value(serde_json::json!({
                "address": address, "name": name, "structs": part,
            }))
            .unwrap()
        })
        .collect();
    let mut yaml = String::from("name: busy\nnetwork: testnet\nsources:\n");
    for i in 0..events {
        let module = ["orders", "positions", "vaults"][i % 3];
        writeln!(
            yaml,
            "  e{i}: {{ event: {address}::{module}::SomethingHappenedEvent{i} }}"
        )
        .unwrap();
    }
    yaml.push_str("state:\n");
    for i in 0..events {
        writeln!(yaml, "  e{i}_log: {{ log: e{i} }}").unwrap();
    }
    let config = parse(&yaml).unwrap_or_else(|d| panic!("{}", d.render("nineveh.yaml", &yaml)));
    let mut builder = LockBuilder::new(config.network);
    for module in modules {
        builder.add_module(module);
    }
    let lock = builder.build(&config.roots()).unwrap();
    let project = config.resolve(&lock).unwrap();

    let filter = stream_filter(&project).unwrap();
    assert!(filter.encoded_len() <= MAX_FILTER_BYTES);
    assert_eq!(coarse_leaves(&filter).len(), 3, "one leaf per module");
}

#[test]
fn projects_with_resource_or_table_sources_stream_unfiltered() {
    let (_, vault) = vault::project();
    assert_eq!(stream_filter(&vault), None);

    let resource_only = project(&format!(
        "\
name: market
network: testnet
sources:
  created: {{ event: {MARKET}::user::CreateContractEvent }}
  books:   {{ resource: {MARKET}::market::OrderBook }}
state:
  created_log: {{ log: created }}
  books:       {{ mirror: books }}
"
    ));
    assert_eq!(stream_filter(&resource_only), None);
}

#[test]
fn filter_addresses_are_written_as_the_stream_writes_event_types() {
    let bytes = fs::read(fixtures().join(format!("testnet/testnet-{FIXTURE}.pb"))).unwrap();
    let tx = Transaction::decode(bytes.as_slice()).unwrap();
    let Some(TxnData::User(user)) = &tx.txn_data else {
        panic!("expected a user transaction");
    };
    let event = user
        .events
        .iter()
        .find(|e| e.type_str.ends_with("::user::CreateContractEvent"))
        .unwrap();
    let rendered = event.type_str.split("::").next().unwrap();

    let filter = stream_filter(&market_events()).unwrap();
    let (address, _, _) = leaves(&filter).into_iter().next().unwrap();
    assert_eq!(
        address, rendered,
        "the filter matches the stream's own rendering"
    );
    assert_eq!(
        rendered.len(),
        2 + 63,
        "and this address has a stripped leading zero"
    );
}

/// Streams the fixture's version from testnet with `filter`, returning the versions
/// delivered.
async fn stream_one(key: &str, filter: Option<BooleanTransactionFilter>) -> Vec<u64> {
    let mut config = StreamConfig::hosted(nineveh_core::Network::Testnet, Version::new(FIXTURE));
    config.api_key = Some(key.to_owned().into());
    config.transactions_count = Some(1);
    assert!(filter.is_some());
    config.filter = filter;
    let mut stream = TransactionStream::connect(config).await.unwrap();
    let mut delivered = Vec::new();
    while let Some(batch) = stream.next_batch().await.unwrap() {
        delivered.extend(batch.transactions.iter().map(|tx| tx.version));
    }
    delivered
}

/// Checks the server delivers the fixture's transaction for the generated filter, and
/// nothing for a filter it doesn't match. Run with `APTOS_API_KEY` set:
/// `cargo test -p nineveh-pipeline --test filter -- --ignored`.
#[tokio::test]
#[ignore = "needs APTOS_API_KEY and the network"]
async fn the_server_matches_the_generated_filter() {
    let Ok(key) = std::env::var("APTOS_API_KEY") else {
        panic!("set APTOS_API_KEY (a Geomi testnet key) to run this test");
    };
    let market = market_events();
    assert_eq!(
        stream_one(&key, stream_filter(&market)).await,
        [FIXTURE],
        "the filter must match the fixture's event"
    );
    // The coarser filters for a filter too big to send match it too.
    let (module, address) = budgets(&market);
    for budget in [module, address] {
        assert_eq!(
            stream_one(&key, stream_filter_within(&market, budget)).await,
            [FIXTURE],
            "a coarser filter must match the fixture's event"
        );
    }

    // A control: the same version, filtered on an event it doesn't emit.
    let config = parse(
        "\
name: other
network: testnet
sources:
  deposits: { event: 0xcafe::vault::DepositEvent }
state:
  deposit_log: { log: deposits }
",
    )
    .unwrap();
    let mut builder = LockBuilder::new(config.network);
    for module in vault::modules() {
        builder.add_module(module);
    }
    let lock = builder.build(&config.roots()).unwrap();
    let other = config.resolve(&lock).unwrap();
    assert_eq!(
        stream_one(&key, stream_filter(&other)).await,
        Vec::<u64>::new(),
        "a filter the transaction doesn't match delivers nothing"
    );
}
