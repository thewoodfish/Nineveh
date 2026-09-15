//! The catalog and scaffolding over real testnet ABIs: the lending market at
//! `0x0e3117…` (trimmed fixture ABIs, as `nineveh init` would fetch them).

#![allow(
    clippy::unwrap_used,
    clippy::panic,
    reason = "test-only crate: helpers panic on unexpected results"
)]

use std::fs;
use std::path::Path;

use nineveh_config::parse;
use nineveh_control::{Catalog, Draft, ItemKind, ScaffoldError, Start, catalog, scaffold};
use nineveh_core::{Address, Network};
use nineveh_decode::{LockBuilder, ModuleAbi};

const MARKET: &str = "0x0e3117b978e079073756f6e1aafff9e4fcb028e51612c3a80c20a095fdfd4a02";

fn testnet_abis() -> Vec<ModuleAbi> {
    let dir = Path::new(env!("CARGO_MANIFEST_DIR")).join("../../fixtures/abi/testnet");
    fs::read_dir(dir)
        .unwrap()
        .map(|entry| {
            let text = fs::read_to_string(entry.unwrap().path()).unwrap();
            serde_json::from_str(&text).unwrap()
        })
        .collect()
}

fn market() -> Catalog {
    catalog(MARKET.parse::<Address>().unwrap(), &testnet_abis())
}

fn draft(picks: &[&str]) -> Draft {
    Draft {
        name: "market".into(),
        network: Network::Testnet,
        start: Start::Auto,
        picks: picks.iter().map(|p| format!("{MARKET}::{p}")).collect(),
    }
}

#[test]
fn lists_events_resources_and_tables() {
    let catalog = market();
    assert_eq!(catalog.address, MARKET);
    assert!(catalog.modules.contains(&"user".to_owned()));
    let find = |id: &str| {
        catalog
            .items
            .iter()
            .find(|i| i.id == format!("{MARKET}::{id}"))
            .unwrap_or_else(|| panic!("no {id} in {:#?}", catalog.items))
    };

    let created = find("user::CreateContractEvent");
    assert_eq!(created.kind, ItemKind::Event);
    assert_eq!(created.suggested_name, "create_contract_event");
    assert!(
        created
            .fields
            .iter()
            .any(|f| f.name == "user_address" && f.ty == "address")
    );
    assert_eq!(created.unsupported, None);

    let states = find("user::MarketStates");
    assert_eq!(states.kind, ItemKind::Resource);
    assert_eq!(states.suggested_name, "market_states");

    let table = find("user::MarketStates.states");
    assert_eq!(table.kind, ItemKind::Table);
    assert_eq!(table.suggested_name, "market_states_states");
    assert_eq!(table.fields[0].ty, "u64");
    assert!(
        table.fields[1].ty.ends_with("::user::MarketState"),
        "{:?}",
        table.fields
    );

    // Two modules define `LinkedList`: the module goes in front.
    let names: Vec<&str> = catalog
        .items
        .iter()
        .map(|i| i.suggested_name.as_str())
        .collect();
    assert!(names.contains(&"linked_list_linked_list"), "{names:?}");
    assert!(names.contains(&"ref_linked_list_linked_list"), "{names:?}");
    let generic = find("linked_list::LinkedList");
    assert!(generic.generic);

    // Events first, then resources, then tables; names unique.
    let kinds: Vec<ItemKind> = catalog.items.iter().map(|i| i.kind).collect();
    assert!(kinds.is_sorted(), "{kinds:?}");
    let mut unique = names.clone();
    unique.sort_unstable();
    unique.dedup();
    assert_eq!(unique.len(), names.len());

    // Only this address's modules.
    assert!(catalog.items.iter().all(|i| i.id.starts_with(MARKET)));
}

#[test]
fn scaffolds_a_config_that_resolves() {
    let yaml = scaffold(
        &draft(&[
            "user::CreateContractEvent",
            "user::CloseContractEvent",
            "user::MarketStates",
            "user::MarketStates.states",
        ]),
        &[market()],
    )
    .unwrap();
    assert!(yaml.contains("start_version: auto\n"), "{yaml}");
    assert!(
        yaml.contains("  create_contract_event:\n    log: create_contract_event\n"),
        "{yaml}"
    );
    assert!(
        yaml.contains("  market_states_states:\n    mirror: market_states_states\n"),
        "{yaml}"
    );

    let config = parse(&yaml).unwrap_or_else(|d| panic!("{}", d.render("nineveh.yaml", &yaml)));
    let mut builder = LockBuilder::new(Network::Testnet);
    for abi in testnet_abis() {
        builder.add_module(abi);
    }
    let lock = builder.build(&config.roots()).unwrap();
    let project = config
        .resolve(&lock)
        .unwrap_or_else(|d| panic!("{}", d.render("nineveh.yaml", &yaml)));
    let tables: Vec<&str> = project
        .config()
        .state
        .iter()
        .map(|t| t.name.as_str())
        .collect();
    assert_eq!(
        tables,
        [
            "create_contract_event",
            "close_contract_event",
            "market_states",
            "market_states_states"
        ]
    );
}

#[test]
fn starts_at_a_version_when_asked() {
    let mut draft = draft(&["user::CreateContractEvent"]);
    draft.start = Start::Version(11_215_853_021);
    let yaml = scaffold(&draft, &[market()]).unwrap();
    assert!(yaml.contains("start_version: 11215853021\n"), "{yaml}");
    parse(&yaml).unwrap();
}

#[test]
fn refuses_what_it_cant_scaffold() {
    let catalog = [market()];
    let mut bad_name = draft(&["user::CreateContractEvent"]);
    bad_name.name = "My Market".into();
    assert!(matches!(
        scaffold(&bad_name, &catalog),
        Err(ScaffoldError::Name(_))
    ));
    assert_eq!(scaffold(&draft(&[]), &catalog), Err(ScaffoldError::NoPicks));
    assert!(matches!(
        scaffold(&draft(&["user::Nope"]), &catalog),
        Err(ScaffoldError::Unknown(_))
    ));
}

#[test]
fn explains_what_it_cant_follow() {
    let abi: ModuleAbi = serde_json::from_value(serde_json::json!({
        "address": "0xcafe",
        "name": "pool",
        "structs": [
            {
                "name": "Registry", "abilities": ["key"], "generic_type_params": [],
                "fields": [
                    { "name": "address", "type": "address" },
                    { "name": "orders", "type": "0x1::big_ordered_map::BigOrderedMap<u64, u64>" },
                    { "name": "legacy", "type": "0x1::table_with_length::TableWithLength<u64, u64>" },
                    { "name": "swaps", "type": "0x1::event::EventHandle<0xcafe::pool::Swapped>" }
                ]
            },
            {
                "name": "Pool", "abilities": ["key"],
                "generic_type_params": [{ "constraints": [] }],
                "fields": [ { "name": "items", "type": "0x1::smart_table::SmartTable<u64, T0>" } ]
            },
            {
                "name": "Swapped", "abilities": ["drop", "store"], "generic_type_params": [],
                "fields": [ { "name": "amountIn", "type": "u64" } ]
            }
        ]
    }))
    .unwrap();
    let catalog = catalog("0xcafe".parse().unwrap(), &[abi]);
    let reason = |id: &str| {
        catalog
            .items
            .iter()
            .find(|i| {
                i.id == format!(
                    "0x000000000000000000000000000000000000000000000000000000000000cafe::{id}"
                )
            })
            .unwrap_or_else(|| panic!("no {id} in {:#?}", catalog.items))
            .unsupported
            .clone()
    };
    assert!(reason("pool::Registry").unwrap().contains("`address`"));
    assert!(
        reason("pool::Registry.orders")
            .unwrap()
            .contains("BigOrderedMap")
    );
    assert!(
        reason("pool::Registry.legacy")
            .unwrap()
            .contains("TableWithLength")
    );
    assert!(reason("pool::Pool.items").unwrap().contains("generic"));
    // Held in an `EventHandle`: an old-style event, whose camelCase field can't be a
    // column.
    let swapped = catalog.items.iter().find(|i| i.name == "Swapped").unwrap();
    assert_eq!(swapped.kind, ItemKind::Event);
    assert!(swapped.unsupported.as_ref().unwrap().contains("amountIn"));
    assert!(matches!(
        scaffold(
            &Draft {
                name: "pool".into(),
                network: Network::Testnet,
                start: Start::Auto,
                picks: vec![swapped.id.clone()],
            },
            std::slice::from_ref(&catalog)
        ),
        Err(ScaffoldError::Unsupported { .. })
    ));
}
