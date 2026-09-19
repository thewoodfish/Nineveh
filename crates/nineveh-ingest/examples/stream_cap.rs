//! Measure the organization's concurrent Transaction Stream limit.
//!
//! ```text
//! cargo run --release -p nineveh-ingest --example stream_cap -- \
//!     --network testnet [--max 24]
//! ```
//!
//! ADR 0021 sizes its backfill pool from one number nobody has measured: how many
//! streams this Geomi organization may hold open at once. Geomi caps it per
//! organization, shared across gRPC and WebSocket, and answers over it with a 429 — so
//! the limit is discoverable by opening streams until one is refused.
//!
//! It opens them one at a time, reads a batch from each to prove the connection is
//! really serving rather than merely accepted, and holds every earlier one open
//! meanwhile. It stops at the first refusal or at `--max`, then drops them all. Each
//! stream reads a single batch, so the bandwidth is a few megabytes whatever the
//! answer turns out to be.

#![allow(clippy::print_stdout, clippy::print_stderr)]

use std::time::Instant;

use anyhow::{Context as _, Result};
use clap::Parser;
use nineveh_core::{Network, Version};
use nineveh_ingest::{Compression, StreamConfig, TransactionStream};
use secrecy::SecretString;
use serde_json::json;

#[derive(Debug, Parser)]
struct Args {
    #[arg(long, default_value = "testnet")]
    network: Network,
    /// Stop here even if nothing has been refused. A ceiling this high is as good as
    /// no ceiling for the decision ADR 0021 has to make.
    #[arg(long, default_value_t = 24)]
    max: usize,
    /// Keep every stream open this long after the probe finishes, so a second probe
    /// can run against them and show whether the two share a pool.
    #[arg(long, default_value_t = 0)]
    hold_seconds: u64,
    /// Where each stream starts. Defaults to a version far enough back that every
    /// stream has something to read.
    #[arg(long)]
    start: Option<u64>,
    #[arg(long, env = "APTOS_API_KEY", hide_env_values = true)]
    api_key: Option<String>,
}

#[tokio::main]
async fn main() -> Result<()> {
    let args = Args::parse();
    let key = args
        .api_key
        .clone()
        .context("a Geomi key is required: set APTOS_API_KEY")?;
    let start = Version::new(args.start.unwrap_or(1_000_000));

    // Held for the whole probe: dropping one would free a slot and hide the limit.
    let mut open: Vec<TransactionStream> = Vec::new();
    let mut refused: Option<(usize, String)> = None;
    let began = Instant::now();

    for n in 1..=args.max {
        let mut config = StreamConfig::hosted(args.network, start);
        config.api_key = Some(SecretString::from(key.clone()));
        config.compression = Some(Compression::Zstd);

        let mut stream = match TransactionStream::connect(config).await {
            Ok(stream) => stream,
            Err(error) => {
                refused = Some((n, error.to_string()));
                break;
            }
        };
        // Accepting a connection is not the same as serving it: a cap can show up as a
        // stream that opens and then immediately ends. Read one batch to be sure.
        match stream.next_batch().await {
            Ok(Some(batch)) => {
                println!(
                    "{n:>3} open — newest read {}",
                    batch.transactions.len()
                );
            }
            Ok(None) => {
                refused = Some((n, "the stream opened and then closed without a batch".into()));
                break;
            }
            Err(error) => {
                refused = Some((n, error.to_string()));
                break;
            }
        }
        open.push(stream);
    }

    let report = json!({
        "network": args.network.to_string(),
        "concurrent_streams_held": open.len(),
        "refused_at": refused.as_ref().map(|(n, _)| *n),
        "refusal": refused.as_ref().map(|(_, why)| why.clone()),
        "reached_max_without_refusal": refused.is_none(),
        "seconds": began.elapsed().as_secs_f64(),
    });
    println!("{}", serde_json::to_string_pretty(&report)?);
    if args.hold_seconds > 0 {
        eprintln!("holding {} streams for {}s", open.len(), args.hold_seconds);
        tokio::time::sleep(std::time::Duration::from_secs(args.hold_seconds)).await;
    }
    if refused.is_none() {
        eprintln!(
            "note: {} streams opened without a refusal — the limit is at least that, \
             and --max is what stopped the probe, not Geomi.",
            open.len()
        );
    }
    Ok(())
}
