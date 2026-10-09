//! Which contracts are actually being used, ranked, straight from the chain.
//!
//! Directories and landing pages say what a project *is*; they don't say whether
//! anything is calling it. We lost two evenings to a protocol whose site was up, whose
//! repository looked current, and whose contract address had never sent a transaction.
//! The chain cannot be wrong about that.
//!
//! This reads a recent window of the Transaction Stream, counts the entry functions
//! every user transaction called, and ranks the modules behind them. The output is a
//! queue of contracts that were provably transacting minutes ago.
//!
//! ```text
//! cargo run --release -p nineveh-ingest --example live_contracts -- --minutes 20
//! ```
//!
//! Needs a Geomi key for the network (`APTOS_API_KEY`). Ranking by unique senders as
//! well as by transactions matters: one bot in a loop makes a lot of transactions and
//! is not a lot of users — on a six-minute mainnet window an oracle keeper posted
//! 10,439 transactions from one address, and an SBT mint 69 from sixty-three.
//!
//! **A short window is biased towards busy contracts, not important ones.** A perps
//! venue trades every second; a lending market with hundreds of millions in it can go
//! minutes between transactions and not appear at all. Absence here means "not
//! transacting in this window", never "dead" — which is the opposite of what a
//! directory tells you, and the two together are the useful pair. Widen `--minutes`
//! when looking for the slower kind.

#![allow(clippy::print_stdout, clippy::print_stderr)]

use std::collections::{BTreeSet, HashMap, HashSet};

use anyhow::{Context as _, Result};
use clap::Parser;
use nineveh_core::{Network, Version};
use nineveh_ingest::proto::transaction::transaction::TxnData;
use nineveh_ingest::proto::transaction::{Transaction, transaction_payload};
use nineveh_ingest::{Compression, RestClient, StreamConfig, TransactionStream};
use secrecy::SecretString;

/// Versions a second, per network, for turning minutes into a window.
/// Measured in `docs/research/spike-a-stream.md`.
const fn versions_per_second(network: Network) -> u64 {
    match network {
        Network::Mainnet => 148,
        Network::Testnet | Network::Devnet => 220,
    }
}

/// Addresses whose activity says nothing about a product: the framework itself.
const FRAMEWORK: [&str; 5] = ["0x1", "0x3", "0x4", "0x5", "0xa"];

#[derive(Parser, Debug)]
#[command(about = "Rank contracts by how much they are actually being used")]
struct Args {
    #[arg(long, default_value = "mainnet")]
    network: Network,
    /// How far back to sample. The window ends at the chain's current version.
    #[arg(long, default_value_t = 20)]
    minutes: u64,
    /// Start here instead of `minutes` before the tip.
    #[arg(long)]
    start: Option<u64>,
    /// Modules to leave out, as `0xabc` or `0xabc::module`. Repeat or comma-separate.
    #[arg(long, value_delimiter = ',')]
    skip: Vec<String>,
    /// Don't report a module with fewer transactions than this.
    #[arg(long, default_value_t = 2)]
    min: u64,
    /// How many to print.
    #[arg(long, default_value_t = 30)]
    top: usize,
    #[arg(long, env = "APTOS_API_KEY", hide_env_values = true)]
    api_key: Option<String>,
}

/// What one module did in the window.
#[derive(Default)]
struct Use {
    transactions: u64,
    failed: u64,
    senders: HashSet<String>,
    functions: BTreeSet<String>,
    last_seen_secs: u64,
    last_version: u64,
}

fn main() -> Result<()> {
    tokio::runtime::Runtime::new()?.block_on(run())
}

async fn run() -> Result<()> {
    let args = Args::parse();
    let key = args.api_key.clone().map(SecretString::from);

    let rest = RestClient::hosted(args.network, key.as_ref())?;
    let tip = rest
        .ledger()
        .await
        .context("reading the chain's tip")?
        .ledger_version;
    let window = args.minutes * 60 * versions_per_second(args.network);
    let start = args
        .start
        .unwrap_or_else(|| tip.get().saturating_sub(window));

    eprintln!(
        "sampling {} from version {start} to the tip at {tip} ({} versions, about {} minutes)",
        args.network,
        tip.get().saturating_sub(start),
        args.minutes,
    );

    let mut config = StreamConfig::hosted(args.network, Version::new(start));
    config.transactions_count = Some(tip.get().saturating_sub(start));
    config.compression = Some(Compression::Zstd);
    config.api_key = key;
    let mut stream = TransactionStream::connect(config)
        .await
        .context("opening the stream")?;

    let mut seen: HashMap<String, Use> = HashMap::new();
    let mut transactions = 0u64;
    while let Some(batch) = stream.next_batch().await? {
        for tx in &batch.transactions {
            transactions += 1;
            observe(tx, &mut seen);
        }
    }

    report(&args, &seen, transactions);
    Ok(())
}

/// Count one transaction against the module its entry function belongs to.
fn observe(tx: &Transaction, seen: &mut HashMap<String, Use>) {
    let Some(TxnData::User(user)) = &tx.txn_data else {
        return;
    };
    let Some(request) = &user.request else { return };
    let Some(payload) = &request.payload else {
        return;
    };
    // Only entry functions: a script has no module to credit, and a write set is
    // governance rather than use.
    let Some(transaction_payload::Payload::EntryFunctionPayload(entry)) = &payload.payload else {
        return;
    };

    // `0xabc::module::function` — the module is everything before the last `::`.
    let id = &entry.entry_function_id_str;
    let Some((module, function)) = id.rsplit_once("::") else {
        return;
    };

    let tally = seen.entry(module.to_owned()).or_default();
    tally.transactions += 1;
    if !tx.info.as_ref().is_some_and(|i| i.success) {
        tally.failed += 1;
    }
    tally.senders.insert(request.sender.clone());
    // A handful is enough to say what the contract is for; a popular module would
    // otherwise collect hundreds and tell you nothing.
    if tally.functions.len() < 4 {
        tally.functions.insert(function.to_owned());
    }
    if tx.version >= tally.last_version {
        tally.last_version = tx.version;
        tally.last_seen_secs = tx
            .timestamp
            .as_ref()
            .map_or(0, |t| t.seconds.unsigned_abs());
    }
}

/// Whether this module is one we deliberately aren't looking at.
fn skipped(module: &str, skip: &[String]) -> bool {
    let address = module.split("::").next().unwrap_or(module);
    // `0x000…1` and `0x1` are the same address; compare on the trimmed form.
    let trimmed = address.trim_start_matches("0x").trim_start_matches('0');
    if FRAMEWORK
        .iter()
        .any(|f| f.trim_start_matches("0x") == trimmed)
    {
        return true;
    }
    skip.iter()
        .any(|s| module == s || address == s || module.starts_with(&format!("{s}::")))
}

fn report(args: &Args, seen: &HashMap<String, Use>, transactions: u64) {
    let mut ranked: Vec<(&String, &Use)> = seen
        .iter()
        .filter(|(module, used)| used.transactions >= args.min && !skipped(module, &args.skip))
        .collect();
    // Senders first: a contract used by many people is a better prospect than one
    // driven by a single loop, however busy that loop is.
    ranked.sort_by(|a, b| {
        (b.1.senders.len(), b.1.transactions).cmp(&(a.1.senders.len(), a.1.transactions))
    });

    eprintln!(
        "{transactions} transactions, {} modules called, {} after filtering\n",
        seen.len(),
        ranked.len()
    );
    println!(
        "{:<22} {:<26} {:>6} {:>8} {:>7}  CALLS",
        "ADDRESS", "MODULE", "TXNS", "SENDERS", "FAILED",
    );
    for (module, used) in ranked.into_iter().take(args.top) {
        let (address, name) = module.split_once("::").unwrap_or((module, ""));
        let short = if address.len() > 20 {
            format!("{}…{}", &address[..10], &address[address.len() - 6..])
        } else {
            address.to_owned()
        };
        println!(
            "{short:<22} {name:<26} {:>6} {:>8} {:>7}  {}",
            used.transactions,
            used.senders.len(),
            used.failed,
            used.functions
                .iter()
                .cloned()
                .collect::<Vec<_>>()
                .join(", "),
        );
    }
}
