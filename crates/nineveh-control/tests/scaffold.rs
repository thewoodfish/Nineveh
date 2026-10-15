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
use nineveh_control::{
    Catalog, Draft, ItemKind, ScaffoldError, Start, add_sources, catalog, scaffold,
};
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
    assert!(created.variants.is_empty());

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
            },
            {
                "name": "Traded", "is_event": true, "is_enum": true, "abilities": ["drop", "store"],
                "generic_type_params": [], "fields": [],
                "variants": [
                    { "name": "V1", "fields": [ { "name": "size", "type": "u64" } ] },
                    { "name": "V2", "fields": [ { "name": "size", "type": "u64" }, { "name": "fee", "type": "u64" } ] }
                ]
            },
            {
                "name": "Clashed", "is_event": true, "is_enum": true, "abilities": ["drop", "store"],
                "generic_type_params": [], "fields": [],
                "variants": [ { "name": "V1", "fields": [ { "name": "version", "type": "u64" } ] } ]
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
    // `TableWithLength` is followable: same handle and same items as a plain `Table`,
    // with a count beside them, which is why the decoder maps both to one container.
    // It is listed as a table source, not listed with a reason it cannot be one.
    assert_eq!(reason("pool::Registry.legacy"), None);
    assert!(reason("pool::Pool.items").unwrap().contains("generic"));
    // Held in an `EventHandle`: an old-style event, whose camelCase field can't be a
    // column.
    // A versioned event: its variants, and the latest one's fields.
    let traded = catalog.items.iter().find(|i| i.name == "Traded").unwrap();
    assert_eq!(traded.kind, ItemKind::Event);
    assert_eq!(traded.variants, ["V1", "V2"]);
    let fields: Vec<&str> = traded.fields.iter().map(|f| f.name.as_str()).collect();
    assert_eq!(fields, ["size", "fee"]);
    assert_eq!(traded.unsupported, None);
    // An enum's fields are columns too, so they can clash with the log's key.
    let clashed = catalog.items.iter().find(|i| i.name == "Clashed").unwrap();
    assert!(clashed.unsupported.as_ref().unwrap().contains("`version`"));
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

// --- adding sources to a project that already exists ---------------------------------

/// A project that already follows the market's create event and its states resource,
/// as Studio would have scaffolded it.
fn existing() -> String {
    scaffold(
        &draft(&["user::CreateContractEvent", "user::MarketStates"]),
        &[market()],
    )
    .unwrap()
}

fn add(text: &str, picks: &[&str]) -> Result<String, ScaffoldError> {
    let config = parse(text).unwrap_or_else(|d| panic!("{}", d.render("nineveh.yaml", text)));
    let picks: Vec<String> = picks.iter().map(|p| format!("{MARKET}::{p}")).collect();
    add_sources(text, &config, &picks, &[market()])
}

/// The state tables a config declares, once it has been resolved against the fixture
/// ABIs: proof that what was added is followable and not only well-formed.
fn resolved_tables(yaml: &str) -> Vec<String> {
    let config = parse(yaml).unwrap_or_else(|d| panic!("{}", d.render("nineveh.yaml", yaml)));
    let mut builder = LockBuilder::new(Network::Testnet);
    for abi in testnet_abis() {
        builder.add_module(abi);
    }
    let lock = builder.build(&config.roots()).unwrap();
    let project = config
        .resolve(&lock)
        .unwrap_or_else(|d| panic!("{}", d.render("nineveh.yaml", yaml)));
    project
        .config()
        .state
        .iter()
        .map(|t| t.name.as_str().to_owned())
        .collect()
}

#[test]
fn adds_a_source_and_the_table_it_brings() {
    let before = existing();
    let after = add(&before, &["user::CloseContractEvent"]).unwrap();

    assert!(
        after.contains("  close_contract_event:\n    event: \""),
        "the source isn't there:\n{after}"
    );
    assert!(
        after.contains("  close_contract_event:\n    log: close_contract_event\n"),
        "the table isn't there:\n{after}"
    );
    // Added to the end of each block, so the project reads in the order it was built.
    assert_eq!(
        resolved_tables(&after),
        [
            "create_contract_event",
            "market_states",
            "close_contract_event"
        ]
    );
    // And nothing that was already in the file moved.
    for line in before.lines().filter(|l| !l.trim().is_empty()) {
        assert!(after.contains(line), "lost `{line}`:\n{after}");
    }
}

#[test]
fn adds_two_at_once_and_keeps_their_order() {
    let after = add(
        &existing(),
        &["user::CloseContractEvent", "user::MarketStates.states"],
    )
    .unwrap();
    assert_eq!(
        resolved_tables(&after),
        [
            "create_contract_event",
            "market_states",
            "close_contract_event",
            "market_states_states"
        ]
    );
}

/// The config is the author's file. Their comments, their own reduce table and their
/// indentation all survive — the entries are spliced in, not written over.
#[test]
fn leaves_the_rest_of_the_file_alone() {
    let before = format!(
        "\
# The market backend. Keep this in version control.
name: market
network: testnet
start_version: auto

sources:
    created:
        event: \"{MARKET}::user::CreateContractEvent\"

state:
    # How many contracts each address has opened.
    contracts:
        key: [user_address]
        columns:
            user_address: address
            opened: {{ type: u64, default: 0 }}
        reduce:
            - on: created
              set: {{ opened: \"opened + 1\" }}
"
    );
    let after = add(&before, &["user::CloseContractEvent"]).unwrap();

    assert!(after.starts_with("# The market backend."), "{after}");
    assert!(after.contains("    # How many contracts each address has opened.\n"));
    assert!(after.contains("              set: { opened: \"opened + 1\" }\n"));
    // Four spaces in, because that is what its neighbours use: an entry indented two
    // would not be in the same mapping at all.
    assert!(
        after.contains("    close_contract_event:\n        event: \""),
        "{after}"
    );
    assert!(
        after.contains("    close_contract_event:\n        log: close_contract_event\n"),
        "{after}"
    );
    assert_eq!(
        resolved_tables(&after),
        ["contracts", "close_contract_event"]
    );
}

/// `examples/03-market` writes an entry per line. One added underneath should not be
/// the only one that doesn't.
#[test]
fn writes_an_entry_the_way_its_neighbours_are_written() {
    let before = format!(
        "\
name: market
network: testnet
start_version: auto

sources:
  created: {{ event: \"{MARKET}::user::CreateContractEvent\" }}

state:
  created: {{ log: created }}
"
    );
    let after = add(&before, &["user::MarketStates"]).unwrap();
    assert!(
        after.contains(&format!(
            "  market_states: {{ resource: \"{MARKET}::user::MarketStates\" }}\n"
        )),
        "{after}"
    );
    assert!(
        after.contains("  market_states: { mirror: market_states }\n"),
        "{after}"
    );
    assert_eq!(resolved_tables(&after), ["created", "market_states"]);
}

/// Every table comes from a `reducers:` file, so there is no `state:` block to add the
/// raw table to: it gets one.
#[test]
fn opens_a_state_block_when_there_is_none() {
    let before = format!(
        "\
name: market
network: testnet
start_version: auto
reducers: ./market.nineveh.ts

sources:
  created:
    event: \"{MARKET}::user::CreateContractEvent\"
"
    );
    let after = add(&before, &["user::CloseContractEvent"]).unwrap();
    assert!(
        after.contains("\nstate:\n  close_contract_event:\n    log: close_contract_event\n"),
        "{after}"
    );
    // It still parses, and the reducers key it had is still the one it has.
    let config = parse(&after).unwrap_or_else(|d| panic!("{}", d.render("nineveh.yaml", &after)));
    assert_eq!(config.reducers.as_deref(), Some("./market.nineveh.ts"));
    assert_eq!(config.sources.len(), 2);
}

/// A file whose last line has no newline is still a file: the entry goes under it,
/// not on the end of it.
#[test]
fn adds_under_a_last_line_with_no_newline() {
    let before = format!(
        "name: market\nnetwork: testnet\nstart_version: auto\n\nsources:\n  created:\n             event: \"{MARKET}::user::CreateContractEvent\"\n\nstate:\n  created:\n    log: created"
    );
    let after = add(&before, &["user::CloseContractEvent"]).unwrap();
    assert!(
        after.contains("CreateContractEvent\"\n  close_contract_event:"),
        "{after}"
    );
    assert!(
        after.ends_with(
            "    log: created\n  close_contract_event:\n    log: close_contract_event\n"
        ),
        "{after}"
    );
    assert_eq!(resolved_tables(&after), ["created", "close_contract_event"]);
}

#[test]
fn refuses_a_source_the_project_already_follows() {
    let before = existing();
    assert_eq!(
        add(&before, &["user::MarketStates"]),
        Err(ScaffoldError::AlreadyFollowed {
            id: format!("{MARKET}::user::MarketStates"),
            existing: "market_states".to_owned(),
        })
    );
    // The resource and the table held in it are two different things to follow.
    assert!(add(&before, &["user::MarketStates.states"]).is_ok());
}

/// Suggested names are unique within a catalog, not against a project that has been
/// edited since. A clash is numbered rather than written over.
#[test]
fn numbers_a_name_the_project_already_uses() {
    let before = format!(
        "\
name: market
network: testnet
start_version: auto

sources:
  close_contract_event:
    event: \"{MARKET}::user::CreateContractEvent\"

state:
  close_contract_event:
    log: close_contract_event
"
    );
    let after = add(&before, &["user::CloseContractEvent"]).unwrap();
    assert_eq!(
        resolved_tables(&after),
        ["close_contract_event", "close_contract_event_2"]
    );
}

#[test]
fn refuses_what_it_cant_add() {
    let before = existing();
    assert_eq!(add(&before, &[]), Err(ScaffoldError::NoPicks));
    assert!(matches!(
        add(&before, &["user::Nope"]),
        Err(ScaffoldError::Unknown(_))
    ));

    // A whole block written as one flow mapping has no line to put a line after. The
    // refusal says so rather than mangling the file.
    let flow = format!(
        "name: market\nnetwork: testnet\nstart_version: auto\nsources: {{ created: {{ event: \
         \"{MARKET}::user::CreateContractEvent\" }} }}\n\nstate:\n  created:\n    log: created\n"
    );
    assert_eq!(
        add(&flow, &["user::CloseContractEvent"]),
        Err(ScaffoldError::Unextendable("sources".to_owned()))
    );
}

/// `state:` is not always the last block. An entry added to it goes inside it, not into
/// whatever follows — and the blocks after it keep their own contents.
#[test]
fn adds_into_a_state_block_that_is_not_the_last() {
    let before = format!(
        "\
name: market
network: testnet
start_version: auto

sources:
  created:
    event: \"{MARKET}::user::CreateContractEvent\"

state:
  created:
    log: created

api:
  graphql: false

webhooks:
  my_backend:
    url: https://example.invalid/hook
    on: [created.inserted]
"
    );
    let after = add(&before, &["user::CloseContractEvent"]).unwrap();
    let config = parse(&after).unwrap_or_else(|d| panic!("{}", d.render("nineveh.yaml", &after)));

    // The new table is in `state:`, above `api:` — not swept into it.
    let state_at = after.find("\nstate:\n").unwrap();
    let api_at = after.find("\napi:\n").unwrap();
    let table_at = after.find("  close_contract_event:\n    log:").unwrap();
    assert!(state_at < table_at && table_at < api_at, "{after}");

    // And the blocks after it are untouched.
    assert!(after.contains("api:\n  graphql: false\n"), "{after}");
    assert!(after.contains("    on: [created.inserted]\n"), "{after}");
    assert_eq!(config.webhooks.len(), 1);
    assert!(!config.api.graphql);
    assert_eq!(resolved_tables(&after), ["created", "close_contract_event"]);
}

/// A config whose `state:` comes before its `sources:` is unusual and perfectly good
/// YAML. Each entry still has to land in its own block.
#[test]
fn adds_to_blocks_in_whatever_order_they_are_written() {
    let before = format!(
        "\
name: market
network: testnet
start_version: auto

state:
  created:
    log: created

sources:
  created:
    event: \"{MARKET}::user::CreateContractEvent\"
"
    );
    let after = add(&before, &["user::CloseContractEvent"]).unwrap();
    let added_table = after.find("  close_contract_event:\n    log:").unwrap();
    let sources_key = after.find("\nsources:\n").unwrap();
    let added_source = after.find("  close_contract_event:\n    event:").unwrap();
    assert!(
        added_table < sources_key,
        "the table stayed in `state:`: {after}"
    );
    assert!(
        sources_key < added_source,
        "and the source in `sources:`: {after}"
    );
    assert_eq!(resolved_tables(&after), ["created", "close_contract_event"]);
}
