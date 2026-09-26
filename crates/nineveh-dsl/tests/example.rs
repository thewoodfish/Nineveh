//! The market example's two files agree.
//!
//! `examples/03-market` is what the "Your first backend" tutorial walks through, so a
//! reader pastes both files in and expects them to compile together. Going through
//! `merge` rather than `compile` is the point: it checks the DSL against the config
//! that names it, so a source renamed in one file and not the other fails here rather
//! than in front of the reader.

#![allow(clippy::panic, reason = "a failing example is reported with its text")]

use std::fs;
use std::path::{Path, PathBuf};

use nineveh_config::TableKind;

/// The published address is a placeholder until the contract is deployed; parsing only
/// cares that it is an address, so any valid one stands in.
const PLACEHOLDER: &str = "0xMARKET";

fn read(path: &Path) -> String {
    fs::read_to_string(path).unwrap_or_else(|e| panic!("{}: {e}", path.display()))
}

#[test]
fn the_market_example_compiles() {
    let dir = PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("../../examples/03-market");
    let yaml = read(&dir.join("nineveh.yaml")).replace(PLACEHOLDER, "0x1");
    let dsl = read(&dir.join("market.nineveh.ts"));

    let config = nineveh_config::parse(&yaml)
        .unwrap_or_else(|d| panic!("{}", d.render("nineveh.yaml", &yaml)));
    assert_eq!(
        config.reducers.as_deref(),
        Some("./market.nineveh.ts"),
        "the config should name the reducers file the tutorial writes"
    );

    let config = nineveh_dsl::merge(config, &dsl).unwrap_or_else(|d| {
        panic!(
            "{}",
            d.render_files(&[("nineveh.yaml", &yaml), ("market.nineveh.ts", &dsl)])
        )
    });

    // The mirrors and the log come from the YAML; the two reduce tables from the DSL.
    let reduce: Vec<&str> = config
        .state
        .iter()
        .filter(|t| matches!(t.kind, TableKind::Reduce { .. }))
        .map(|t| t.name.as_str())
        .collect();
    assert_eq!(reduce, ["sellers", "buyers"]);
}
