//! The example on the front page compiles.
//!
//! It is the first code anyone sees, and the one most likely to be copied. If it
//! doesn't work, the first thing a visitor learns is that the docs can't be trusted.
//! Its text is pinned here; `site/app/page.tsx` renders the same lines.

#![allow(clippy::panic, reason = "a failing example is reported with its text")]

use nineveh_config::{Action, TableKind};
use nineveh_dsl::{Context, SourceInfo, compile};

const LANDING: &str = r"export const sellers = table({
  key:     { seller: address },
  columns: { sold: u64.default(0), revenue: u64.default(0) },
})

on(sold, (s) => {
  const row = sellers.row(s.seller)
  row.sold    += 1
  row.revenue += s.price - s.fee
})
";

#[test]
fn the_front_page_example_compiles() {
    let ctx = Context {
        sources: vec![SourceInfo {
            name: "sold".into(),
            has_deletes: false,
        }],
        tables: Vec::new(),
    };
    let tables = match compile(LANDING, &ctx) {
        Ok(tables) => tables,
        Err(d) => panic!("{}", d.render("market.nineveh.ts", LANDING)),
    };
    let TableKind::Reduce { rules, .. } = &tables[0].kind else {
        panic!("not a reduce table")
    };
    let Action::Set(set) = &rules[0].action else {
        panic!("expected a set")
    };
    // `price - fee` keeps its parentheses, so it is added as one quantity.
    assert_eq!(set[0].1.text, "row.sold + 1");
    assert_eq!(set[1].1.text, "row.revenue + (sold.price - sold.fee)");
}
