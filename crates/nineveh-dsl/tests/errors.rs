//! What the compiler says when a handler is wrong.
//!
//! These are product copy as much as tests: a developer writing a reducer for the
//! first time meets these messages, not the code. Each case asserts the message and
//! the suggestion, so neither can drift without someone deciding to change it.

#![allow(
    clippy::panic,
    reason = "the helper reports an unexpectedly successful compile"
)]

use nineveh_dsl::{Context, SourceInfo, TableInfo, compile};

const PRELUDE: &str = r"
export const balances = table({
  key: { user: address },
  columns: { balance: u128.default(0) },
})
";

fn ctx() -> Context {
    Context {
        sources: vec![
            SourceInfo {
                name: "deposits".into(),
                has_deletes: false,
            },
            SourceInfo {
                name: "vaults".into(),
                has_deletes: true,
            },
        ],
        tables: vec![
            TableInfo {
                name: "markets".into(),
                key_arity: 1,
                is_log: false,
            },
            TableInfo {
                name: "deposit_log".into(),
                key_arity: 2,
                is_log: true,
            },
        ],
    }
}

/// Compile something expected to fail, and return every message and help line.
fn errors(body: &str) -> String {
    let source = format!("{PRELUDE}\n{body}");
    match compile(&source, &ctx()) {
        Ok(_) => panic!("expected this to fail:\n{source}"),
        Err(d) => d
            .as_slice()
            .iter()
            .map(|e| match &e.help {
                Some(help) => format!("{}\n  help: {help}", e.message),
                None => e.message.clone(),
            })
            .collect::<Vec<_>>()
            .join("\n"),
    }
}

#[test]
fn unknown_source_suggests_the_closest() {
    assert_eq!(
        errors("on(deposit, (d) => { balances.row(d.user).balance += u128(d.amount) })"),
        "nothing in nineveh.yaml is called `deposit`\n  help: did you mean `deposits`?"
    );
}

#[test]
fn unknown_column_suggests_the_closest() {
    assert_eq!(
        errors("on(deposits, (d) => { balances.row(d.user).balence += u128(d.amount) })"),
        "`balances` has no column `balence`\n  help: did you mean `balance`?"
    );
}

#[test]
fn an_event_source_has_no_deletes() {
    assert_eq!(
        errors("on(deposits.deleted, (d) => { balances.row(d.user).delete() })"),
        "`deposits` is an event source, so it has no deletes\n  \
         help: only `resource:` and `table:` sources delete"
    );
}

#[test]
fn a_key_column_is_not_set_by_a_rule() {
    assert_eq!(
        errors("on(deposits, (d) => { balances.row(d.user).user = d.other })"),
        "`user` identifies the row, so a rule can't set it\n  \
         help: pass it to `balances.row(…)` instead"
    );
}

#[test]
fn a_rule_reads_its_own_row_only() {
    assert_eq!(
        errors(
            "on(deposits, (d) => {
               const a = balances.row(d.user)
               const b = balances.row(d.other)
               b.balance = a.balance
             })"
        ),
        "`a` isn't the row this rule writes\n  \
         help: a rule reads its own row; for another, use `balances.get(<key>)?.balance`"
    );
}

#[test]
fn a_log_cannot_be_read() {
    assert_eq!(
        errors(
            "on(deposits, (d) => {
               balances.row(d.user).balance = u128(deposit_log.get(d.a, d.b)?.amount ?? 0)
             })"
        ),
        "`deposit_log` is a log, so a rule can't read it\n  \
         help: logs are append-only history, not state (ADR 0019)"
    );
}

#[test]
fn a_mirror_table_takes_no_writes() {
    assert_eq!(
        errors("on(deposits, (d) => { markets.row(d.market).fee_bps = 1 })"),
        "`markets` isn't written by a rule\n  \
         help: its source writes it; only a `table({ … })` here takes writes"
    );
}

#[test]
fn let_is_not_part_of_the_language() {
    assert_eq!(
        errors("on(deposits, (d) => { let x = 1 })"),
        "`let` isn't part of this language\n  \
         help: use `const`: a rule computes values, it doesn't keep any"
    );
}

#[test]
fn a_row_is_not_a_value() {
    assert_eq!(
        errors(
            "on(deposits, (d) => {
               const b = balances.row(d.user)
               b.balance = markets.row(d.market)
             })"
        ),
        "a row isn't a value\n  \
         help: `.row(…)` names a row to write; to read one, use `.get(…)`"
    );
}

#[test]
fn a_lookup_needs_a_column() {
    assert_eq!(
        errors(
            "on(deposits, (d) => {
               balances.row(d.user).balance = u128(markets.get(d.market) ?? 0)
             })"
        ),
        "`markets.get(…)` is a row, not a value\n  \
         help: read a column of it: `markets.get(…)?.<column>`"
    );
}

#[test]
fn the_record_is_reached_by_field() {
    assert_eq!(
        errors("on(deposits, (d) => { balances.row(d.user).balance = u128(d) })"),
        "`d` is the whole record\n  help: read a field of it: `d.<field>`"
    );
}

#[test]
fn a_column_is_set_once_per_rule() {
    assert_eq!(
        errors(
            "on(deposits, (d) => {
               const b = balances.row(d.user)
               b.balance = u128(1)
               b.balance = u128(2)
             })"
        ),
        "`balance` is set twice for the same row\n  \
         help: one rule sets each column once; combine them into one expression"
    );
}

#[test]
fn unreachable_writes_are_rejected() {
    assert_eq!(
        errors(
            "on(deposits, (d) => {
               return
               balances.row(d.user).balance = u128(1)
             })"
        ),
        "this can never run\n  help: an earlier `return` always stops the handler first"
    );
}

#[test]
fn a_row_is_written_or_deleted_not_both() {
    assert_eq!(
        errors(
            "on(vaults.deleted, (v) => {
               const b = balances.row(v.address)
               b.balance = u128(0)
               b.delete()
             })"
        ),
        "this row is both written and deleted\n  \
         help: a rule either sets columns or deletes the row"
    );
}

#[test]
fn the_wrong_number_of_keys() {
    assert_eq!(
        errors("on(deposits, (d) => { balances.row(d.user, d.other).balance = u128(1) })"),
        "`balances` is keyed by 1 column, but 2 were given"
    );
}

/// Parse errors carry the place too, and say what was expected.
#[test]
fn a_parse_error_says_what_was_expected() {
    let source = format!("{PRELUDE}\non(deposits, (d) => {{ balances.row(d.user).balance }})");
    let Err(d) = compile(&source, &ctx()) else {
        panic!("expected a parse error")
    };
    assert!(
        d.render("vault.nineveh.ts", &source)
            .contains("expected `=`, `+=`, `-=` or `.delete()`, found `}`"),
        "{}",
        d.render("vault.nineveh.ts", &source)
    );
}
