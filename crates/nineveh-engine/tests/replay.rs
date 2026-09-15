//! The M1 headline property (ADR 0005): for any workload, any batch boundaries and any
//! crash points, the committed state and change feed equal a single clean pass, and
//! both equal what a model of the contract says.
//!
//! The workload is the testkit's synthetic vault contract, rendered as real Transaction
//! Stream messages and run through the decoder, so this exercises decode and fold
//! together (see `nineveh_testkit::vault`).

#![allow(
    clippy::unwrap_used,
    clippy::panic,
    reason = "test-only crate: helpers panic on unexpected results"
)]

use std::collections::BTreeSet;

use nineveh_core::Value;
use nineveh_decode::DecodedTransaction;
use nineveh_engine::{
    ChangeSet, Engine, FoldError, Key, Lookup, MemoryState, RowChange, StateView, TableId,
};
use nineveh_testkit::vault::{
    CONFIG, Op, Rows, decode, event, op, project, transaction, transactions,
};
use proptest::prelude::*;

// --- folding strategies -----------------------------------------------------------

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

fn rows(state: &MemoryState) -> impl Fn(u32) -> Rows + '_ {
    |table| {
        state
            .rows(TableId::State(table))
            .map(|(k, r)| (k.clone(), r.clone()))
            .collect()
    }
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
        model.check(rows(&clean.0));

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
