//! Rules come out numbered in the order the handler's statements run.
//!
//! This is the half of ADR 0025's ordering rule that lives in the frontend; the engine
//! half — that it applies them in that order — is `nineveh-engine/tests/ordering.rs`.
//! Together they make "rules apply in the order you wrote them" true, which is the
//! only ordering rule worth asking a developer to hold in their head.

#![allow(clippy::panic, reason = "the helper reports a failed compile")]

use nineveh_config::{StateTable, TableKind};
use nineveh_dsl::{Context, SourceInfo, compile};

/// Every rule as `(table, seq)`, so a test can read the order at a glance.
fn order(dsl: &str) -> Vec<(String, u32)> {
    let ctx = Context {
        sources: vec![
            SourceInfo {
                name: "deposits".into(),
                has_deletes: false,
            },
            SourceInfo {
                name: "withdrawals".into(),
                has_deletes: false,
            },
        ],
        tables: Vec::new(),
    };
    let tables: Vec<StateTable> = match compile(dsl, &ctx) {
        Ok(tables) => tables,
        Err(d) => panic!("{}", d.render("vault.nineveh.ts", dsl)),
    };
    let mut out: Vec<(String, u32)> = tables
        .iter()
        .flat_map(|t| match &t.kind {
            TableKind::Reduce { rules, .. } => rules
                .iter()
                .map(|r| (t.name.as_str().to_owned(), r.seq))
                .collect(),
            TableKind::Mirror { .. } | TableKind::Log { .. } => Vec::new(),
        })
        .collect();
    out.sort_by_key(|(_, seq)| *seq);
    out
}

const TABLES: &str = r"
export const first  = table({ key: { k: address }, columns: { v: u64.default(0) } })
export const second = table({ key: { k: address }, columns: { v: u64.default(0) } })
";

/// Writing `second` before `first` numbers them that way — which is the case a
/// table-major config cannot express, because it orders by declaration.
#[test]
fn statement_order_beats_declaration_order() {
    let order = order(&format!(
        "{TABLES}
on(deposits, (d) => {{
  second.row(d.user).v += 1
  first.row(d.user).v += 1
}})"
    ));
    assert_eq!(
        order,
        vec![("second".to_owned(), 0), ("first".to_owned(), 1)]
    );
}

/// A later handler keeps counting, so numbers are unique across the whole file.
#[test]
fn handlers_keep_counting() {
    let order = order(&format!(
        "{TABLES}
on(deposits, (d) => {{
  first.row(d.user).v += 1
}})

on(withdrawals, (w) => {{
  second.row(w.user).v += 1
  first.row(w.user).v += 1
}})"
    ));
    assert_eq!(
        order,
        vec![
            ("first".to_owned(), 0),
            ("second".to_owned(), 1),
            ("first".to_owned(), 2),
        ]
    );
}

/// Several writes to one row are one rule, so they take one number between them.
#[test]
fn one_row_takes_one_number() {
    let order = order(
        r"
export const t = table({ key: { k: address }, columns: { a: u64.default(0), b: u64.default(0) } })

on(deposits, (d) => {
  const r = t.row(d.user)
  r.a += 1
  r.b += 1
})",
    );
    assert_eq!(order, vec![("t".to_owned(), 0)]);
}

/// Both arms of an `if` write the same row, so they are two rules — and they keep the
/// order they were written in.
#[test]
fn branches_take_a_number_each() {
    let order = order(
        r"
export const t = table({ key: { k: address }, columns: { a: u64.default(0), b: u64.default(0) } })

on(deposits, (d) => {
  const r = t.row(d.user)
  if (d.amount > 0) { r.a += 1 } else { r.b += 1 }
})",
    );
    assert_eq!(order, vec![("t".to_owned(), 0), ("t".to_owned(), 1)]);
}
