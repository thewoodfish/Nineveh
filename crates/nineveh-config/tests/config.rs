//! `nineveh.yaml` end to end: parsing, every class of diagnostic, and resolution
//! against a lock built from real module ABIs, down to decoding a real transaction
//! with the resolved selection.

#![allow(
    clippy::unwrap_used,
    clippy::panic,
    reason = "test-only crate: helpers panic on unexpected results"
)]

use std::fs;
use std::path::{Path, PathBuf};

use nineveh_config::{
    Action, ColumnType, Config, Diagnostics, Input, Projection, ResolvedAction, ResolvedTable,
    StartVersion, TableKind, parse,
};
use nineveh_core::{Address, Network, Value};
use nineveh_decode::{LockBuilder, Lockfile, ModuleAbi, RecordData, TransactionDecoder};
use nineveh_expr::{Inputs, Tx};
use nineveh_proto::transaction::Transaction;
use prost::Message;

const PALETTE: &str = "0x54854b644a44548c19299ba64dd4ff76915a3a55f09be4a7ac0f3cb32f10df54";
const PERP: &str = "0x50ead22afd6ffd9769e3b3d6e0e64a2a350d68e8b102c4e72e33d0b8cfdfdb06";

fn fixtures() -> PathBuf {
    Path::new(env!("CARGO_MANIFEST_DIR")).join("../../fixtures")
}

/// Build the lock for `config` the way `nineveh init` will: from its roots, over the
/// module ABIs (here, the trimmed fixture ABIs instead of a fullnode).
fn lock_for(config: &Config) -> Lockfile {
    let mut builder = LockBuilder::new(config.network);
    let dir = fixtures().join("abi").join(config.network.as_str());
    for entry in fs::read_dir(dir).unwrap() {
        let text = fs::read_to_string(entry.unwrap().path()).unwrap();
        builder.add_module(serde_json::from_str::<ModuleAbi>(&text).unwrap());
    }
    builder.build(&config.roots()).unwrap()
}

fn messages(yaml: &str) -> Vec<String> {
    match parse(yaml) {
        Ok(_) => panic!("expected errors for:\n{yaml}"),
        Err(d) => d.as_slice().iter().map(|d| d.message.clone()).collect(),
    }
}

fn render(yaml: &str) -> String {
    parse(yaml).unwrap_err().render("nineveh.yaml", yaml)
}

fn resolve_errors(yaml: &str) -> Diagnostics {
    let config = parse(yaml).unwrap_or_else(|d| panic!("{}", d.render("nineveh.yaml", yaml)));
    let lock = lock_for(&config);
    config.resolve(&lock).unwrap_err()
}

fn palette_yaml() -> String {
    format!(
        r#"# An SBT collection: mints, per-holder counts, limits and balances.
name: palette
network: mainnet
start_version: 7205730000

sources:
  mints:    {{ event: {PALETTE}::PaletteSBTModule::TokenMinted }}
  limits:   {{ resource: {PALETTE}::PaletteSBTModule::CollectionLimits }}
  balances: {{ table: {PALETTE}::PaletteSBTModule::CollectionLimits.user_balances }}

state:
  holders:
    key: [soul_bound_to]
    columns:
      soul_bound_to: address
      minted:   {{ type: u64, default: 0 }}
      last_uri: {{ type: string, nullable: true }}
    reduce:
      - on: mints
        set: {{ minted: "minted + 1", last_uri: uri }}
  limits:   {{ mirror: limits }}
  balances: {{ mirror: balances }}
  mint_log: {{ log: mints }}

api: {{ graphql: false }}
realtime:
  - on: holders.changed
    webhook: https://example.com/hooks/holders
"#
    )
}

#[test]
fn a_real_project_resolves_and_decodes_its_transaction() {
    let yaml = palette_yaml();
    let config = parse(&yaml).unwrap_or_else(|d| panic!("{}", d.render("nineveh.yaml", &yaml)));
    assert_eq!(config.network, Network::Mainnet);
    assert_eq!(config.start_version, StartVersion::Version(7_205_730_000));
    assert!(config.api.rest && !config.api.graphql);
    assert_eq!(config.realtime.len(), 1);

    let TableKind::Reduce { columns, rules, .. } = &config.table("holders").unwrap().kind else {
        panic!("holders is a reduce table");
    };
    assert_eq!(columns[1].ty, ColumnType::U64);
    assert_eq!(columns[1].default, Some(Value::U64(0)));
    // An unquoted YAML scalar is still an expression.
    let Action::Set(set) = &rules[0].action else {
        panic!("expected a set rule")
    };
    assert_eq!(set[1].1.text, "uri");

    let lock = lock_for(&config);
    let project = config.resolve(&lock).unwrap();

    // The implicit key comes from the event field of the same name.
    let ResolvedTable::Reduce { rules } = &project.tables()[0] else {
        panic!("holders resolves as a reduce table");
    };
    let rule = &rules[0];
    assert_eq!(rule.key[0].0, "soul_bound_to");
    assert_eq!(rule.key[0].1.source.text, "mints.soul_bound_to");
    assert!(rule.scope.get("uri").is_some());

    let balances = project.source_id("balances").unwrap();
    assert!(matches!(project.input(balances), Some(Input::Table(_))));

    // The resolved selection decodes the real transaction's records for each source.
    let bytes = fs::read(fixtures().join("mainnet/mainnet-7205730568.pb")).unwrap();
    let tx = Transaction::decode(bytes.as_slice()).unwrap();
    let decoded = TransactionDecoder::new(&lock, project.selection())
        .decode(&tx)
        .unwrap();
    let from = |name: &str| {
        let id = project.source_id(name).unwrap();
        decoded.records.iter().filter(move |r| r.source == id)
    };
    assert!(from("limits").any(|r| matches!(r.data, RecordData::ResourceWrite { .. })));
    assert!(from("balances").any(|r| matches!(r.data, RecordData::TableWrite { .. })));

    // Run the rule's compiled expressions on the real event, as the engine will: the
    // record's fields in scope order, and a new row holding its defaults.
    let event = from("mints")
        .find_map(|r| match &r.data {
            RecordData::Event { value, .. } => Some(value.clone()),
            _ => None,
        })
        .unwrap();
    let record: Vec<Value> = rule
        .scope
        .fields
        .iter()
        .map(|(name, _)| event.field(name.as_str()).unwrap().clone())
        .collect();
    let new_row = [
        Value::Address(Address::ZERO),
        Value::U64(0),
        Value::Option(None),
    ];
    let inputs = Inputs {
        row: &new_row,
        record: &record,
        tx: Tx {
            version: decoded.version.get(),
            timestamp_micros: decoded.timestamp_micros,
        },
    };
    let key = rule.key[0].1.compiled.eval(&inputs).unwrap();
    assert_eq!(&key, event.field("soul_bound_to").unwrap());
    let ResolvedAction::Set(set) = &rule.action else {
        panic!("expected a set rule")
    };
    assert_eq!(set[0].1.compiled.eval(&inputs).unwrap(), Value::U64(1));
    assert!(matches!(
        set[1].1.compiled.eval(&inputs).unwrap(),
        Value::Option(Some(_))
    ));
}

#[test]
fn expression_errors_point_into_the_yaml() {
    let yaml = palette_yaml().replace(
        "set: { minted: \"minted + 1\", last_uri: uri }",
        "set: { minted: \"minted + uri\", last_uri: urii }",
    );
    let rendered = resolve_errors(&yaml).render("nineveh.yaml", &yaml);
    assert!(
        rendered.contains(
            "error: expected u64, found string\n  --> nineveh.yaml:20:34\n   |\n\
             20 |         set: { minted: \"minted + uri\", last_uri: urii }\n   |                                  ^^^"
        ),
        "{rendered}"
    );
    assert!(
        rendered.contains("error: unknown name `urii`"),
        "{rendered}"
    );
    assert!(rendered.contains("help: did you mean `uri`?"), "{rendered}");
}

#[test]
fn reading_a_column_without_a_default_is_caught() {
    let yaml = palette_yaml().replace("minted:   { type: u64, default: 0 }", "minted:   u64");
    let errors = resolve_errors(&yaml);
    assert_eq!(
        errors.as_slice()[0].message,
        "`minted` has no default, so a new row has no value to read"
    );
}

#[test]
fn roots_are_what_init_must_pin() {
    let config = parse(&palette_yaml()).unwrap();
    let roots: Vec<String> = config.roots().iter().map(ToString::to_string).collect();
    assert_eq!(
        roots,
        [
            format!("{PALETTE}::PaletteSBTModule::CollectionLimits"),
            format!("{PALETTE}::PaletteSBTModule::TokenMinted"),
        ]
    );
}

// --- YAML shape -------------------------------------------------------------------

const MINIMAL: &str = "\
name: vault
network: testnet
sources:
  deposits: { event: 0xabc::vault::DepositEvent }
state:
  log: { log: deposits }
";

#[test]
fn the_minimal_config_parses_with_defaults() {
    let config = parse(MINIMAL).unwrap();
    assert_eq!(config.start_version, StartVersion::Auto);
    assert!(config.api.rest && config.api.graphql);
    assert!(config.realtime.is_empty());
}

#[test]
fn typos_in_keys_are_located() {
    let yaml = MINIMAL.replace("state:", "stat:");
    assert_eq!(
        render(&yaml),
        "error: unknown field `stat`, expected one of name, network, start_version, sources, \
         state, api, realtime\n \
         --> nineveh.yaml:5:1\n  \
         |\n\
         5 | stat:\n  \
         | ^^^^\n"
    );
}

#[test]
fn duplicate_keys_are_errors() {
    let yaml = MINIMAL.replace(
        "  deposits: { event: 0xabc::vault::DepositEvent }\n",
        "  deposits: { event: 0xabc::vault::DepositEvent }\n  deposits: { event: 0xabc::vault::X }\n",
    );
    assert_eq!(messages(&yaml), ["`deposits` is defined twice"]);
}

#[test]
fn start_version_is_auto_or_a_version() {
    let negative = MINIMAL.replace("network: testnet", "network: testnet\nstart_version: -1");
    assert_eq!(messages(&negative), ["start_version can't be negative"]);
    let word = MINIMAL.replace(
        "network: testnet",
        "network: testnet\nstart_version: latest",
    );
    assert!(messages(&word)[0].contains("expected `auto` or a transaction version"));
}

#[test]
fn yes_and_no_are_not_booleans() {
    let yaml = format!("{MINIMAL}api: {{ rest: yes }}\n");
    assert_eq!(
        messages(&yaml),
        ["invalid boolean (strict mode expects true/false)"]
    );
}

// --- meaning ---------------------------------------------------------------------

#[test]
fn names_and_networks_get_suggestions() {
    let yaml = MINIMAL
        .replace("name: vault", "name: My-Vault")
        .replace("network: testnet", "network: mainet");
    let rendered = render(&yaml);
    assert!(
        rendered.contains("error: invalid project name `My-Vault`"),
        "{rendered}"
    );
    assert!(
        rendered.contains("help: did you mean `mainnet`?"),
        "{rendered}"
    );
}

#[test]
fn every_problem_is_reported_at_once() {
    let yaml = "\
name: vault
network: testnet
sources:
  deposits: { event: 0xabc::vault::DepositEvent }
  vaults:   { resource: 0xabc::vault::Vault }
state:
  balances:
    key: [user, user]
    columns:
      user: address
      balance: { type: u8, default: 300 }
      owner: { type: address, default: 0x1 }
      meta: vector<u8>
    reduce:
      - { on: deposit, set: { balance: \"balance + amount\" } }
      - { on: deposits.deleted, delete: true }
      - { on: vaults }
      - { on: vaults, set: { user: \"x\", balanse: \"1\" } }
  mirrored: { mirror: deposits }
  both: { mirror: vaults, log: deposits }
realtime:
  - { on: balances.changed, webhook: http://example.com/hook }
  - { on: balanses.changed, webhook: https://example.com/hook }
";
    let got = messages(yaml);
    let expected = [
        "invalid default for this u8 column: out of range for u8",
        "invalid default for this address column: write addresses as quoted strings, like \"0x1\"",
        "unknown column type `vector<u8>`",
        "`user` is listed in the key twice",
        "unknown source `deposit`",
        "`deposits` is an event source, and events are never deleted",
        "this rule doesn't do anything",
        "`user` is part of the key",
        "unknown column `balanse`",
        "`mirror` needs a resource or table source, but `deposits` is an event source",
        "state table `both` has both `mirror` and `log`",
        "invalid webhook URL `http://example.com/hook`: use https (plain http is only allowed \
         for localhost)",
        "unknown state table `balanses`",
    ];
    for message in expected {
        assert!(
            got.iter().any(|g| g == message),
            "missing {message:?} in {got:#?}"
        );
    }
    let rendered = render(yaml);
    assert!(
        rendered.contains("help: did you mean `deposits`?"),
        "{rendered}"
    );
    assert!(
        rendered.contains("help: byte vectors are `bytes`"),
        "{rendered}"
    );
    assert!(
        rendered.contains("help: did you mean `balance`?"),
        "{rendered}"
    );
}

#[test]
fn every_set_rule_must_complete_a_new_row() {
    let yaml = "\
name: vault
network: testnet
sources:
  deposits: { event: 0xabc::vault::DepositEvent }
state:
  balances:
    key: [user]
    columns:
      user: address
      balance: u128
      note: { type: string, nullable: true }
      count: { type: u64, default: 0 }
    reduce:
      - on: deposits
        set: { count: \"count + 1\" }
";
    assert_eq!(
        messages(yaml),
        ["this rule can create a row but doesn't set `balance`, which has no default"]
    );
}

#[test]
fn wide_defaults_are_exact() {
    let yaml = "\
name: vault
network: testnet
sources:
  deposits: { event: 0xabc::vault::DepositEvent }
state:
  totals:
    key: [id]
    columns:
      id: u64
      big: { type: u256, default: \"115792089237316195423570985008687907853269984665640564039457584007913129639935\" }
      neg: { type: i128, default: -5 }
      raw: { type: bytes, default: \"0xf600\" }
    reduce:
      - { on: deposits, set: { neg: \"neg\" } }
";
    let config = parse(yaml).unwrap();
    let TableKind::Reduce { columns, .. } = &config.state[0].kind else {
        panic!()
    };
    assert_eq!(
        columns[1].default,
        Some(Value::U256(nineveh_core::U256::MAX))
    );
    assert_eq!(columns[2].default, Some(Value::I128(-5)));
    assert_eq!(columns[3].default, Some(Value::Bytes(vec![0xf6, 0])));
}

#[test]
fn floats_are_rejected_everywhere() {
    let yaml = MINIMAL.replace(
        "  log: { log: deposits }\n",
        "  t:\n    key: [id]\n    columns: { id: u64, x: u64 }\n    reduce:\n      - { on: deposits, set: { x: 1.5 } }\n",
    );
    assert!(messages(&yaml)[0].contains("floating-point numbers aren't supported"));
}

// --- resolution ------------------------------------------------------------------

#[test]
fn enum_events_expose_fields_common_to_every_variant() {
    // TradeEvent is an enum (V1, V2); both variants have `account`.
    let yaml = format!(
        "\
name: perp
network: mainnet
sources:
  trades: {{ event: {PERP}::perp_positions::TradeEvent }}
state:
  traders:
    key: [account]
    columns: {{ account: address, trades: {{ type: u64, default: 0 }} }}
    reduce:
      - {{ on: trades, set: {{ trades: \"trades + 1\" }} }}
"
    );
    let config = parse(&yaml).unwrap();
    let lock = lock_for(&config);
    let project = config.resolve(&lock).unwrap();
    let ResolvedTable::Reduce { rules } = &project.tables()[0] else {
        panic!()
    };
    let scope = &rules[0].scope;
    assert!(scope.is_enum);
    assert!(scope.get("account").is_some() && scope.get("size").is_some());
    // Only V2 has `counter_party_account`, so a rule can't rely on it.
    assert!(scope.get("counter_party_account").is_none());
}

#[test]
fn implicit_keys_must_exist_and_match_their_column() {
    let yaml = format!(
        "\
name: palette
network: mainnet
sources:
  mints: {{ event: {PALETTE}::PaletteSBTModule::TokenMinted }}
state:
  a:
    key: [owner]
    columns: {{ owner: address, n: {{ type: u64, default: 0 }} }}
    reduce: [{{ on: mints, set: {{ n: \"n + 1\" }} }}]
  b:
    key: [name]
    columns: {{ name: u64, n: {{ type: u64, default: 0 }} }}
    reduce: [{{ on: mints, set: {{ n: \"n + 1\" }} }}]
"
    );
    let errors = resolve_errors(&yaml);
    let messages: Vec<&str> = errors
        .as_slice()
        .iter()
        .map(|d| d.message.as_str())
        .collect();
    assert_eq!(
        messages,
        [
            "this rule doesn't say where key column `owner` comes from, and `TokenMinted` has \
             no field `owner`",
            "`TokenMinted.name` is a `0x1::string::String`, but key column `name` is `u64`",
        ]
    );
    assert!(
        errors
            .render("nineveh.yaml", &yaml)
            .contains("help: map it under the rule's `key`")
    );
}

#[test]
fn unknown_types_point_at_init() {
    let yaml = format!(
        "\
name: palette
network: mainnet
sources:
  mints: {{ event: {PALETTE}::PaletteSBTModule::TokenMinted }}
  other: {{ event: {PALETTE}::PaletteSBTModule::Nope }}
state:
  log: {{ log: mints }}
"
    );
    let config = parse(&yaml).unwrap();
    // Build the lock without the missing struct, as a stale lock would be.
    let mut stale = config.clone();
    stale.sources.truncate(1);
    let lock = lock_for(&stale);
    let rendered = config
        .resolve(&lock)
        .unwrap_err()
        .render("nineveh.yaml", &yaml);
    assert!(
        rendered.contains("has no layout in nineveh.lock"),
        "{rendered}"
    );
    assert!(rendered.contains("nineveh.yaml:5:"), "{rendered}");
}

#[test]
fn the_lock_must_be_for_the_same_network() {
    let yaml = palette_yaml();
    let config = parse(&yaml).unwrap();
    let lock = lock_for(&config);
    let mut testnet = config.clone();
    testnet.network = Network::Testnet;
    let errors = testnet.resolve(&lock).unwrap_err();
    assert_eq!(
        errors.as_slice()[0].message,
        "this project is for testnet, but nineveh.lock was built for mainnet"
    );
}

#[test]
fn tables_with_identical_item_types_resolve_with_parent_watchers() {
    // TradingVolumeBucket holds two Table<address, VolumeHistory>: taker and maker.
    // Their items are told apart by handle, learned from the bucket (ADR 0012).
    let yaml = format!(
        "\
name: perp
network: mainnet
sources:
  taker: {{ table: {PERP}::trading_volume_tracker::TradingVolumeBucket.user_taker_volume_history }}
  maker: {{ table: {PERP}::trading_volume_tracker::TradingVolumeBucket.user_maker_volume_history }}
state:
  taker: {{ mirror: taker }}
  maker: {{ mirror: maker }}
"
    );
    let config = parse(&yaml).unwrap();
    let lock = lock_for(&config);
    let project = config.resolve(&lock).unwrap();
    let watchers = project.watchers();
    assert_eq!(watchers.len(), 2);
    assert_eq!(
        watchers[0].table_source,
        project.source_id("taker").unwrap()
    );
    assert_eq!(watchers[0].field.as_str(), "user_taker_volume_history");
    assert!(project.watcher(watchers[1].id).is_some());
}

#[test]
fn key_expressions_read_only_the_record() {
    let yaml = palette_yaml().replace(
        "      - on: mints\n        set:",
        "      - on: mints\n        key: { soul_bound_to: \"creator\" }\n        when: \"minted < 100\"\n        set:",
    );
    let config = parse(&yaml).unwrap();
    let lock = lock_for(&config);
    assert!(
        config.clone().resolve(&lock).is_ok(),
        "`when` may read the row"
    );
    let bad = yaml.replace("soul_bound_to: \"creator\"", "soul_bound_to: \"last_uri\"");
    let errors = resolve_errors(&bad);
    assert_eq!(errors.as_slice()[0].message, "unknown name `last_uri`");
}

#[test]
fn every_table_gets_typed_columns() {
    let yaml = palette_yaml();
    let config = parse(&yaml).unwrap();
    let lock = lock_for(&config);
    let project = config.resolve(&lock).unwrap();
    let columns = |table: usize| -> Vec<(String, ColumnType, bool, Projection)> {
        project.schemas()[table]
            .columns
            .iter()
            .map(|c| (c.name.clone(), c.ty, c.nullable, c.from.clone()))
            .collect()
    };
    let field = |i, name: &str| Projection::Field(i, name.parse().unwrap());

    // A reduce table has the columns it declares, keyed as declared.
    assert_eq!(
        columns(0),
        [
            (
                "soul_bound_to".into(),
                ColumnType::Address,
                false,
                Projection::Row(0)
            ),
            ("minted".into(), ColumnType::U64, false, Projection::Row(1)),
            (
                "last_uri".into(),
                ColumnType::String,
                true,
                Projection::Row(2)
            ),
        ]
    );
    assert_eq!(project.schemas()[0].key, [0]);

    // A resource mirror: its address, then the resource's fields.
    assert_eq!(
        columns(1),
        [
            (
                "address".into(),
                ColumnType::Address,
                false,
                Projection::Row(0)
            ),
            (
                "max_balance_per_user".into(),
                ColumnType::U64,
                false,
                field(1, "max_balance_per_user")
            ),
            (
                "user_balances".into(),
                ColumnType::Json,
                false,
                field(1, "user_balances")
            ),
            (
                "collection_max_supply".into(),
                ColumnType::U64,
                false,
                field(1, "collection_max_supply")
            ),
        ]
    );

    // A table mirror: handle and key, then a scalar value in one column.
    assert_eq!(
        columns(2),
        [
            (
                "handle".into(),
                ColumnType::Address,
                false,
                Projection::Row(0)
            ),
            ("key".into(), ColumnType::Address, false, Projection::Row(1)),
            ("value".into(), ColumnType::U64, false, Projection::Row(2)),
        ]
    );
    assert_eq!(project.schemas()[2].key, [0, 1]);

    // A log: version and event index, then the event's fields.
    let log = columns(3);
    assert_eq!(
        log[..3],
        [
            ("version".into(), ColumnType::U64, false, Projection::Row(0)),
            (
                "event_index".into(),
                ColumnType::U32,
                false,
                Projection::Row(1)
            ),
            (
                "creator".into(),
                ColumnType::Address,
                false,
                field(2, "creator")
            ),
        ]
    );
    assert_eq!(log.len(), 2 + 6);
}

#[test]
fn fields_that_cant_be_columns_are_reported() {
    let yaml = "\
name: clash
network: testnet
sources:
  things: { resource: 0xbeef::m::Thing }
  pings:  { event: 0xbeef::m::Ping }
state:
  things: { mirror: things }
  pings:  { log: pings }
";
    let abi: ModuleAbi = serde_json::from_str(
        r#"{"address": "0xbeef", "name": "m", "structs": [
          {"name": "Thing", "abilities": ["key"], "generic_type_params": [],
           "fields": [{"name": "address", "type": "address"}, {"name": "Weird", "type": "u8"}]},
          {"name": "Ping", "is_event": true, "abilities": ["drop", "store"], "generic_type_params": [],
           "fields": [{"name": "version", "type": "u64"}, {"name": "ok", "type": "bool"}]}
        ]}"#,
    )
    .unwrap();
    let config = parse(yaml).unwrap();
    let mut builder = LockBuilder::new(config.network);
    builder.add_module(abi);
    let lock = builder.build(&config.roots()).unwrap();
    let errors = config.resolve(&lock).unwrap_err();
    let messages: Vec<&str> = errors
        .as_slice()
        .iter()
        .map(|d| d.message.as_str())
        .collect();
    // Struct names print their address at full width (AIP-40).
    let module = format!("{}::m", "0xbeef".parse::<Address>().unwrap());
    assert_eq!(
        messages,
        [
            format!(
                "table `things` can't store field `address` of `{module}::Thing`: the name \
                 clashes with a key column"
            ),
            format!(
                "table `things` can't store field `Weird` of `{module}::Thing`: the name isn't \
                 a valid column name (lower snake case, at most 63 characters, not starting \
                 with `_`)"
            ),
            format!(
                "table `pings` can't store field `version` of `{module}::Ping`: the name \
                 clashes with a key column"
            ),
        ]
    );
    // Each points at the source the table names, and suggests a reduce table.
    let rendered = errors.render("nineveh.yaml", yaml);
    assert!(
        rendered.contains("things: { mirror: things }"),
        "{rendered}"
    );
    assert!(
        rendered.contains("build this table with `reduce`"),
        "{rendered}"
    );
}

#[test]
fn the_canonical_form_ignores_formatting_but_not_meaning() {
    let yaml = palette_yaml();
    let canonical = parse(&yaml).unwrap().canonical();
    let reformatted = format!("# a comment\n{}", yaml.replace(":    {", ": {"));
    assert_eq!(parse(&reformatted).unwrap().canonical(), canonical);
    // Serving and webhooks don't change what's built.
    let served = yaml.replace("api: { graphql: false }", "api: { graphql: true }");
    assert_eq!(parse(&served).unwrap().canonical(), canonical);
    // Rules do.
    let changed = yaml.replace("minted + 1", "minted + 2");
    assert_ne!(parse(&changed).unwrap().canonical(), canonical);
}
