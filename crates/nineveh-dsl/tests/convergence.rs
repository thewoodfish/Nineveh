//! The DSL and `nineveh.yaml` describe the same project.
//!
//! ADR 0025 says both frontends converge on the same tables and rules. That claim is
//! only worth making if it's checked, so each case here writes a project both ways and
//! compares what they build — through [`Config::canonical`], which is the exact text a
//! build fingerprint is taken over (ADR 0005) and which ignores spans and layout.

#![allow(
    clippy::panic,
    clippy::expect_used,
    reason = "the helpers report a failed compile with its rendered diagnostics"
)]

use nineveh_config::{Config, StateTable};
use nineveh_dsl::{Context, SourceInfo, TableInfo, compile};

/// The sources every case here shares, as `nineveh.yaml` declares them.
const SOURCES: &str = r"
name: vault
network: testnet
start_version: auto

sources:
  deposits:    { event: 0xabc::vault::DepositEvent }
  withdrawals: { event: 0xabc::vault::WithdrawEvent }
  vaults:      { resource: 0xabc::vault::Vault }
";

fn context() -> Context {
    Context {
        sources: vec![
            SourceInfo {
                name: "deposits".into(),
                has_deletes: false,
            },
            SourceInfo {
                name: "withdrawals".into(),
                has_deletes: false,
            },
            SourceInfo {
                name: "vaults".into(),
                has_deletes: true,
            },
        ],
        tables: Vec::new(),
    }
}

/// Parse `state:` YAML into a config, and compile the DSL into one with the same
/// sources. Returns the two canonical forms, which must match.
fn both(state: &str, dsl: &str) -> (String, String) {
    let yaml = format!("{SOURCES}\nstate:\n{state}");
    let from_yaml = nineveh_config::parse(&yaml).expect("the YAML is a valid config");
    let tables = match compile(dsl, &context()) {
        Ok(tables) => tables,
        Err(d) => panic!(
            "the DSL should compile:\n{}",
            d.render("vault.nineveh.ts", dsl)
        ),
    };
    let from_dsl = Config {
        state: tables,
        ..from_yaml.clone()
    };
    (from_yaml.canonical(), from_dsl.canonical())
}

fn assert_same(state: &str, dsl: &str) {
    let (yaml, dsl_built) = both(state, dsl);
    assert_eq!(
        yaml, dsl_built,
        "\nYAML built:\n{yaml}\nDSL built:\n{dsl_built}"
    );
}

/// Several writes to one named row are one rule with several columns.
#[test]
fn one_row_many_columns_is_one_rule() {
    assert_same(
        r#"
  balances:
    key: [user]
    columns:
      user:     address
      balance:  { type: u128, default: 0 }
      deposits: { type: u64, default: 0 }
    reduce:
      - on: deposits
        key: { user: "deposits.user" }
        set:
          balance:  "row.balance + u128(deposits.amount)"
          deposits: "row.deposits + 1"
"#,
        r"
export const balances = table({
  key: { user: address },
  columns: { balance: u128.default(0), deposits: u64.default(0) },
})

on(deposits, (d) => {
  const b = balances.row(d.user)
  b.balance += u128(d.amount)
  b.deposits += 1
})
",
    );
}

/// One handler writing two tables scatters into one rule on each — the case YAML can
/// only express by naming `deposits` twice, in two places.
#[test]
fn one_handler_two_tables() {
    assert_same(
        r#"
  balances:
    key: [user]
    columns:
      user:    address
      balance: { type: u128, default: 0 }
    reduce:
      - on: deposits
        key: { user: "deposits.user" }
        set: { balance: "row.balance + u128(deposits.amount)" }
  market_stats:
    key: [market]
    columns:
      market: address
      volume: { type: u128, default: 0 }
    reduce:
      - on: deposits
        key: { market: "deposits.market" }
        set: { volume: "row.volume + u128(deposits.amount)" }
"#,
        r"
export const balances = table({
  key: { user: address },
  columns: { balance: u128.default(0) },
})

export const market_stats = table({
  key: { market: address },
  columns: { volume: u128.default(0) },
})

on(deposits, (d) => {
  balances.row(d.user).balance += u128(d.amount)
  market_stats.row(d.market).volume += u128(d.amount)
})
",
    );
}

/// `if`/`else` over one row becomes two rules with opposite conditions.
#[test]
fn branches_become_opposite_whens() {
    assert_same(
        r#"
  flows:
    key: [user]
    columns:
      user: address
      in:   { type: u128, default: 0 }
      out:  { type: u128, default: 0 }
    reduce:
      - on: deposits
        when: "deposits.incoming"
        key: { user: "deposits.user" }
        set: { in: "row.in + u128(deposits.amount)" }
      - on: deposits
        when: "!deposits.incoming"
        key: { user: "deposits.user" }
        set: { out: "row.out + u128(deposits.amount)" }
"#,
        r"
export const flows = table({
  key: { user: address },
  columns: { in: u128.default(0), out: u128.default(0) },
})

on(deposits, (d) => {
  const f = flows.row(d.user)
  if (d.incoming) {
    f.in += u128(d.amount)
  } else {
    f.out += u128(d.amount)
  }
})
",
    );
}

/// An early `return` is the negation of its condition, folded into `when`. Negating a
/// comparison flips the operator rather than wrapping it.
#[test]
fn early_return_becomes_a_negated_when() {
    assert_same(
        r#"
  balances:
    key: [user]
    columns:
      user:    address
      balance: { type: u128, default: 0 }
    reduce:
      - on: withdrawals
        when: "withdrawals.amount != 0"
        key: { user: "withdrawals.user" }
        set: { balance: "row.balance - u128(withdrawals.amount)" }
"#,
        r"
export const balances = table({
  key: { user: address },
  columns: { balance: u128.default(0) },
})

on(withdrawals, (w) => {
  if (w.amount == 0) return
  balances.row(w.user).balance -= u128(w.amount)
})
",
    );
}

/// Nested conditions conjoin, in the order they were entered.
#[test]
fn nested_conditions_conjoin() {
    assert_same(
        r#"
  big:
    key: [user]
    columns:
      user:  address
      total: { type: u128, default: 0 }
    reduce:
      - on: deposits
        when: "deposits.amount > 100 && deposits.verified"
        key: { user: "deposits.user" }
        set: { total: "row.total + u128(deposits.amount)" }
"#,
        r"
export const big = table({
  key: { user: address },
  columns: { total: u128.default(0) },
})

on(deposits, (d) => {
  if (d.amount > 100) {
    if (d.verified) {
      big.row(d.user).total += u128(d.amount)
    }
  }
})
",
    );
}

/// `<source>.deleted` and `.delete()`.
#[test]
fn deletes() {
    assert_same(
        r#"
  balances:
    key: [user]
    columns:
      user:    address
      balance: { type: u128, default: 0 }
    reduce:
      - on: vaults.deleted
        key: { user: "vaults.address" }
        delete: true
"#,
        r"
export const balances = table({
  key: { user: address },
  columns: { balance: u128.default(0) },
})

on(vaults.deleted, (v) => {
  balances.row(v.address).delete()
})
",
    );
}

/// `?.` and `??` are how a `.ts` file spells an optional lookup into another table.
#[test]
fn cross_table_lookup() {
    let state = r#"
  markets: { mirror: vaults }
  sales:
    key: [id]
    columns:
      id:     u64
      amount: u64
      fee:    { type: u64, default: 0 }
    reduce:
      - on: deposits
        key: { id: "deposits.id" }
        set:
          amount: "deposits.amount"
          fee:    "deposits.amount * unwrap_or(markets[deposits.market].fee_bps, 0) / 10000"
"#;
    let dsl = r"
export const sales = table({
  key: { id: u64 },
  columns: { amount: u64, fee: u64.default(0) },
})

on(deposits, (d) => {
  const fee_bps = markets.get(d.market)?.fee_bps ?? 0
  const sale = sales.row(d.id)
  sale.amount = d.amount
  sale.fee = d.amount * fee_bps / 10000
})
";
    let yaml = format!("{SOURCES}\nstate:\n{state}");
    let from_yaml = nineveh_config::parse(&yaml).expect("the YAML is a valid config");
    let mut ctx = context();
    ctx.tables.push(TableInfo {
        name: "markets".into(),
        key_arity: 1,
        is_log: false,
    });
    let compiled = match compile(dsl, &ctx) {
        Ok(tables) => tables,
        Err(d) => panic!(
            "the DSL should compile:\n{}",
            d.render("vault.nineveh.ts", dsl)
        ),
    };
    // The mirror table stays in the YAML; the DSL contributes the reduce table.
    let mirror: Vec<StateTable> = from_yaml
        .state
        .iter()
        .filter(|t| t.name.as_str() == "markets")
        .cloned()
        .collect();
    let from_dsl = Config {
        state: mirror.into_iter().chain(compiled).collect(),
        ..from_yaml.clone()
    };
    assert_eq!(from_yaml.canonical(), from_dsl.canonical());
}

/// Precedence survives the round trip: the DSL's `?:` is the expression language's
/// `if … then … else`, and operands are parenthesised only where they must be.
#[test]
fn ternary_and_precedence() {
    assert_same(
        r#"
  capped:
    key: [user]
    columns:
      user:  address
      total: { type: u128, default: 0 }
    reduce:
      - on: deposits
        key: { user: "deposits.user" }
        set:
          total: "row.total + (if deposits.amount > 100 then u128(100) else u128(deposits.amount)) * 2"
"#,
        r"
export const capped = table({
  key: { user: address },
  columns: { total: u128.default(0) },
})

on(deposits, (d) => {
  capped.row(d.user).total += (d.amount > 100 ? u128(100) : u128(d.amount)) * 2
})
",
    );
}
