//! Rules apply in `seq` order, not in the order their tables happen to be declared.
//!
//! It only shows when one rule reads a table another rule writes for the same record
//! (ADR 0019): whichever runs first decides what the other sees. Collecting rules
//! table by table says nothing about that, so the engine sorts them back into the
//! order their frontend gave them — which for a DSL handler is the order its
//! statements run (ADR 0025).

#![allow(
    clippy::unwrap_used,
    clippy::expect_used,
    clippy::panic,
    reason = "test-only crate: helpers panic on unexpected results"
)]

use nineveh_config::{Project, TableKind, parse};
use nineveh_core::Value;
use nineveh_decode::{LockBuilder, Lockfile};
use nineveh_engine::{Engine, MemoryState, TableId};
use nineveh_testkit::vault::{MODULE, decode, event, modules, transaction};

/// `mirror` copies each deposit's amount; `echo` reads it back through a lookup. Which
/// value `echo` sees is decided entirely by which rule runs first.
fn config() -> String {
    format!(
        "\
name: order
network: testnet
sources:
  deposits: {{ event: {MODULE}::vault::DepositEvent }}
state:
  echo:
    key: [user]
    columns:
      user: address
      seen: {{ type: u64, default: 0 }}
    reduce:
      - on: deposits
        key: {{ user: \"deposits.user\" }}
        set: {{ seen: \"unwrap_or(mirror[deposits.user].amount, 0)\" }}
  mirror:
    key: [user]
    columns:
      user: address
      amount: {{ type: u64, default: 0 }}
    reduce:
      - on: deposits
        key: {{ user: \"deposits.user\" }}
        set: {{ amount: \"deposits.amount\" }}
"
    )
}

/// Build the project, optionally swapping the two rules' sequence numbers — which is
/// all a frontend does to say "this one runs first".
fn project(mirror_first: bool) -> (Lockfile, Project) {
    let text = config();
    let mut config = parse(&text).unwrap_or_else(|d| panic!("{}", d.render("nineveh.yaml", &text)));
    if mirror_first {
        for table in &mut config.state {
            if let TableKind::Reduce { rules, .. } = &mut table.kind {
                for rule in rules {
                    rule.seq = u32::from(table.name.as_str() != "mirror");
                }
            }
        }
    }
    let mut builder = LockBuilder::new(config.network);
    for module in modules() {
        builder.add_module(module);
    }
    let lock = builder.build(&config.roots()).unwrap();
    let project = config
        .resolve(&lock)
        .unwrap_or_else(|d| panic!("{}", d.render("nineveh.yaml", &text)));
    (lock, project)
}

/// What `echo.seen` holds after one deposit of 7.
fn seen(mirror_first: bool) -> u64 {
    let (lock, project) = project(mirror_first);
    let txs = decode(
        &lock,
        &project,
        &[transaction(1, vec![event("DepositEvent", 1, 7)], vec![])],
    );
    let engine = Engine::new(&project);
    let mut state = MemoryState::new();
    let changes = engine.fold(&state, &txs).unwrap();
    state.apply(&changes);

    let echo = project
        .config()
        .state
        .iter()
        .position(|t| t.name.as_str() == "echo")
        .and_then(|i| u32::try_from(i).ok())
        .unwrap();
    let row = state
        .rows(TableId::State(echo))
        .next()
        .expect("echo has a row")
        .1
        .clone();
    match row.last() {
        Some(Value::U64(v)) => *v,
        other => panic!("unexpected `seen`: {other:?}"),
    }
}

/// Declared order: `echo` is written first, so its lookup finds nothing yet.
#[test]
fn a_lookup_sees_nothing_when_it_runs_first() {
    assert_eq!(seen(false), 0);
}

/// Renumbered so `mirror` runs first, the same lookup finds the value — without the
/// config, the tables or the expressions changing at all.
#[test]
fn a_lookup_sees_the_write_when_it_runs_after() {
    assert_eq!(seen(true), 7);
}
