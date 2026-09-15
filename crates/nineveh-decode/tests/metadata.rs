//! Resource-group membership read from real module bytecode, as fetched from testnet.

#![allow(clippy::unwrap_used, reason = "test-only crate")]

use std::fs;
use std::path::Path;

use nineveh_core::{Identifier, StructName};
use nineveh_decode::resource_group_members;

fn module(name: &str) -> Vec<u8> {
    fs::read(
        Path::new(env!("CARGO_MANIFEST_DIR"))
            .join("../../fixtures/modules/testnet")
            .join(name),
    )
    .unwrap()
}

fn members(name: &str) -> Vec<(String, String)> {
    resource_group_members(&module(name))
        .unwrap()
        .into_iter()
        .map(|(member, group): (Identifier, StructName)| (member.to_string(), group.to_string()))
        .collect()
}

#[test]
fn object_members_belong_to_the_object_group() {
    let group = "0x1::object::ObjectGroup".to_owned();
    let mut got = members("0x1--object.mv");
    got.sort();
    assert_eq!(
        got,
        [
            ("ObjectCore".to_owned(), group.clone()),
            ("TombStone".to_owned(), group.clone()),
            ("Untransferable".to_owned(), group),
        ],
        "ObjectGroup itself is a group, not a member; Transfer is an event"
    );
}

#[test]
fn fungible_stores_are_group_members() {
    let got = members("0x1--fungible_asset.mv");
    assert_eq!(got.len(), 9);
    assert!(got.contains(&(
        "FungibleStore".to_owned(),
        "0x1::object::ObjectGroup".to_owned()
    )));
    assert!(
        !got.iter()
            .any(|(m, _)| m == "Deposit" || m == "FungibleStoreDeletion"),
        "events aren't members"
    );
}

#[test]
fn a_module_with_only_events_has_no_members() {
    let got =
        members("0x0e3117b978e079073756f6e1aafff9e4fcb028e51612c3a80c20a095fdfd4a02--user.mv");
    assert_eq!(got, Vec::<(String, String)>::new());
}

#[test]
fn truncated_bytecode_is_an_error_not_a_panic() {
    let bytes = module("0x1--object.mv");
    for cut in [5, 9, 40, 3100, bytes.len() - 1] {
        // Cutting inside the headers or the metadata table fails; cutting later leaves
        // the metadata whole.
        let _ = resource_group_members(&bytes[..cut]);
    }
    assert!(resource_group_members(&bytes[..3100]).is_err());
}
