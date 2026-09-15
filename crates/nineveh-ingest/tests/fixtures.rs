//! Integrity checks for the real-transaction fixtures under `fixtures/`.
//!
//! Fixtures are protobuf-encoded `Transaction`s captured from the live Transaction
//! Stream by the `stream_probe` example. They're named `<network>-<version>.pb`. The
//! decoder's snapshot tests (M1) build on them, so a fixture that doesn't decode or
//! doesn't match its name must fail loudly here first.

#![allow(
    clippy::unwrap_used,
    reason = "test-only crate: helpers panic on a malformed fixture tree"
)]

use std::fs;
use std::path::{Path, PathBuf};

use nineveh_core::Network;
use nineveh_ingest::proto::transaction::Transaction;
use prost::Message;

fn fixtures_dir() -> PathBuf {
    Path::new(env!("CARGO_MANIFEST_DIR")).join("../../fixtures")
}

fn fixture_files() -> Vec<PathBuf> {
    let mut files = Vec::new();
    for network in Network::ALL {
        let dir = fixtures_dir().join(network.as_str());
        let Ok(entries) = fs::read_dir(&dir) else {
            continue;
        };
        for entry in entries {
            let path = entry.unwrap().path();
            if path.extension().is_some_and(|e| e == "pb") {
                files.push(path);
            }
        }
    }
    files.sort();
    files
}

#[test]
fn fixtures_exist() {
    assert!(
        !fixture_files().is_empty(),
        "no fixtures found under {}",
        fixtures_dir().display()
    );
}

#[test]
fn every_fixture_decodes_and_matches_its_name() {
    for path in fixture_files() {
        let name = path.file_stem().unwrap().to_string_lossy().into_owned();
        let (network, version) = name
            .split_once('-')
            .unwrap_or_else(|| panic!("{name}: expected <network>-<version>.pb"));
        let network: Network = network.parse().unwrap();
        let version: u64 = version.parse().unwrap();

        let dir = path.parent().unwrap().file_name().unwrap();
        assert_eq!(
            dir,
            network.as_str(),
            "{name}: stored under the wrong network"
        );

        let bytes = fs::read(&path).unwrap();
        let tx = Transaction::decode(bytes.as_slice())
            .unwrap_or_else(|e| panic!("{name}: doesn't decode as a Transaction: {e}"));
        assert_eq!(
            tx.version, version,
            "{name}: version doesn't match file name"
        );
        assert!(tx.info.is_some(), "{name}: missing TransactionInfo");
    }
}

/// The REST client against testnet: ledger info, a module whose bytecode matches the
/// fixture, and a module that doesn't exist. Run with `APTOS_API_KEY` set:
/// `cargo test -p nineveh-ingest --test fixtures -- --ignored`.
#[tokio::test]
#[ignore = "needs APTOS_API_KEY and the network"]
async fn rest_client_reads_testnet() {
    use nineveh_core::{Address, ChainId, Network};
    use nineveh_ingest::RestClient;

    let key = std::env::var("APTOS_API_KEY")
        .map(secrecy::SecretString::from)
        .ok();
    let rest = RestClient::hosted(Network::Testnet, key.as_ref()).unwrap();

    let ledger = rest.ledger().await.unwrap();
    assert_eq!(ledger.chain_id, ChainId::TESTNET);
    assert!(ledger.oldest_ledger_version < ledger.ledger_version);

    let object = rest.module(Address::ONE, "object").await.unwrap().unwrap();
    let fixture = fs::read(
        Path::new(env!("CARGO_MANIFEST_DIR")).join("../../fixtures/modules/testnet/0x1--object.mv"),
    )
    .unwrap();
    assert_eq!(object.bytecode.get(..4), fixture.get(..4));
    assert_eq!(object.abi["name"], "object");

    assert!(
        rest.module(Address::ONE, "no_such_module")
            .await
            .unwrap()
            .is_none()
    );
    assert!(
        rest.module(Address::special(0xa), "nothing")
            .await
            .unwrap()
            .is_none(),
        "an account without modules"
    );

    // First use of an address, from the Indexer API: genesis for the framework, the
    // market contract before its fixture transaction, nothing for an unused address.
    assert_eq!(
        rest.first_transaction(Address::ONE).await.unwrap(),
        Some(nineveh_core::Version::GENESIS)
    );
    let market: Address = "0x0e3117b978e079073756f6e1aafff9e4fcb028e51612c3a80c20a095fdfd4a02"
        .parse()
        .unwrap();
    let first = rest.first_transaction(market).await.unwrap().unwrap();
    assert!(first.get() < 6_000_029_471, "{first}");
    let unused: Address = "0xdeadbeef".parse().unwrap();
    assert_eq!(rest.first_transaction(unused).await.unwrap(), None);
}
