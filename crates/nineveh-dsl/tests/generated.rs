//! What Studio's state-table editor writes.
//!
//! The editor builds reducers in the DSL so a table made there and one written by hand
//! are the same artifact (ADR 0025). That only holds if what it emits actually
//! compiles, and its generator lives in TypeScript where these tests can't reach it —
//! so the exact text of each template's output is pinned here. If Studio's output
//! changes shape, update these and check they still hold.
//!
//! Source: `studio/lib/state-table.ts`, `toDsl` applied to each template.

#![allow(clippy::panic, reason = "the helper reports a failed compile")]

use nineveh_config::{Action, TableKind};
use nineveh_dsl::{Context, SourceInfo, TableInfo, compile};

fn ctx() -> Context {
    Context {
        sources: vec![
            SourceInfo {
                name: "deposits".into(),
                has_deletes: false,
            },
            SourceInfo {
                name: "closed".into(),
                has_deletes: false,
            },
        ],
        tables: vec![TableInfo {
            name: "markets".into(),
            key_arity: None,
            is_log: false,
        }],
    }
}

fn built(dsl: &str) -> Vec<nineveh_config::StateTable> {
    match compile(dsl, &ctx()) {
        Ok(tables) => tables,
        Err(d) => panic!("{}", d.render("project.nineveh.ts", dsl)),
    }
}

/// "How many, per field" — the commonest table anyone builds.
#[test]
fn count_per() {
    let tables = built(
        r"export const deposits_per_user = table({
  key:     { user: address },
  columns: {
    count:     u64.default(0),
    last_seen: u64.default(0),
  },
})

on(deposits, (r) => {
  const b = deposits_per_user.row(r.user)
  b.count += 1
  b.last_seen = tx.timestamp
})
",
    );
    assert_eq!(tables.len(), 1);
    let TableKind::Reduce {
        key,
        columns,
        rules,
    } = &tables[0].kind
    else {
        panic!("not a reduce table")
    };
    assert_eq!(key.len(), 1);
    assert_eq!(columns.len(), 3, "the key column plus two");
    assert_eq!(rules.len(), 1, "one row under one condition is one rule");
    let Action::Set(set) = &rules[0].action else {
        panic!("expected a set")
    };
    // `+=` reads the row; a plain `=` doesn't.
    assert_eq!(set[0].1.text, "row.count + 1");
    assert_eq!(set[1].1.text, "tx.timestamp");
    assert_eq!(rules[0].key[0].1.text, "deposits.user");
}

/// Summing a field widens the column, and the cast comes through.
#[test]
fn sum_per() {
    let tables = built(
        r"export const amount_per_user = table({
  key:     { user: address },
  columns: {
    total_amount: u128.default(0),
    count:        u64.default(0),
  },
})

on(deposits, (r) => {
  const b = amount_per_user.row(r.user)
  b.total_amount += u128(r.amount)
  b.count += 1
})
",
    );
    let TableKind::Reduce { rules, .. } = &tables[0].kind else {
        panic!("not a reduce table")
    };
    let Action::Set(set) = &rules[0].action else {
        panic!("expected a set")
    };
    assert_eq!(set[0].1.text, "row.total_amount + u128(deposits.amount)");
}

/// Rows that appear on one event and go on another: two handlers, one table.
#[test]
fn live_set() {
    let tables = built(
        r"export const open_deposits = table({
  key:     { user: address },
  columns: {
    amount: u64.default(0),
    since:  u64.default(0),
  },
})

on(deposits, (r) => {
  const b = open_deposits.row(r.user)
  b.amount = r.amount
  b.since = tx.timestamp
})

on(closed, (r) => {
  open_deposits.row(r.user).delete()
})
",
    );
    let TableKind::Reduce { rules, .. } = &tables[0].kind else {
        panic!("not a reduce table")
    };
    assert_eq!(rules.len(), 2);
    assert!(matches!(rules[0].action, Action::Set(_)));
    assert!(matches!(rules[1].action, Action::Delete));
    assert_eq!(rules[1].on.source.as_str(), "closed");
    // The delete is written second, so it applies second (ADR 0025).
    assert!(rules[0].seq < rules[1].seq);
}

/// Reading a column of another table, which is always optional (ADR 0019).
#[test]
fn with_lookup() {
    let tables = built(
        r"export const deposits_with_fee_bps = table({
  key:     { user: address },
  columns: {
    count:   u64.default(0),
    fee_bps: u64.nullable(),
  },
})

on(deposits, (r) => {
  const b = deposits_with_fee_bps.row(r.user)
  b.count += 1
  b.fee_bps = markets.get(r.user)?.fee_bps
})
",
    );
    let TableKind::Reduce { columns, rules, .. } = &tables[0].kind else {
        panic!("not a reduce table")
    };
    assert!(
        columns
            .iter()
            .any(|c| c.name.as_str() == "fee_bps" && c.nullable)
    );
    let Action::Set(set) = &rules[0].action else {
        panic!("expected a set")
    };
    assert_eq!(set[1].1.text, "markets[deposits.user].fee_bps");
}
