//! M0 spike tool: measure the Transaction Stream and find fixture candidates.
//!
//! ```text
//! cargo run --release -p nineveh-ingest --example stream_probe -- \
//!     --network testnet --start 7000000000 --count 20000 [--compression none] \
//!     [--event-filter 0x1::coin::CoinDeposit] [--capture-dir fixtures/testnet]
//! ```
//!
//! It prints a JSON report covering throughput, response sizes, transaction and
//! write-set mixes, the most common event, resource and table types, and the first
//! example of each JSON rendering convention the decoder must handle. With
//! `--capture-dir`, it also saves each of those first examples as a protobuf-encoded
//! `Transaction` fixture.

#![allow(clippy::print_stdout, clippy::print_stderr)]

use std::collections::{BTreeMap, HashMap};
use std::fs;
use std::path::PathBuf;
use std::time::Instant;

use anyhow::{Context, Result};
use clap::{Parser, ValueEnum};
use nineveh_core::{Network, Version};
use nineveh_ingest::proto::indexer::{
    BooleanTransactionFilter, EventFilter, LogicalOrFilters, MoveStructTagFilter,
    boolean_transaction_filter,
};
use nineveh_ingest::proto::transaction::{
    Event, Transaction, WriteSetChange, transaction::TransactionType, transaction::TxnData,
    write_set_change::Change,
};
use nineveh_ingest::{Compression, StreamConfig, TransactionStream};
use prost::Message;
use secrecy::SecretString;
use serde_json::{Value, json};

#[derive(Debug, Parser)]
struct Args {
    #[arg(long, default_value = "testnet")]
    network: Network,
    /// First version to stream.
    #[arg(long)]
    start: u64,
    /// Number of transactions to stream.
    #[arg(long, default_value_t = 10_000)]
    count: u64,
    #[arg(long)]
    batch_size: Option<u64>,
    #[arg(long, value_enum, default_value = "zstd")]
    compression: CompressionArg,
    /// Server-side event filter as `address::module::name`. Repeat to OR them together.
    #[arg(long = "event-filter")]
    event_filters: Vec<String>,
    #[arg(long, env = "APTOS_API_KEY", hide_env_values = true)]
    api_key: Option<String>,
    /// Save the first example of each rendering convention as a `.pb` fixture here.
    #[arg(long)]
    capture_dir: Option<PathBuf>,
    /// Also save these exact versions, if they fall inside the streamed range.
    #[arg(long = "capture-version")]
    capture_versions: Vec<u64>,
    /// How many of the most common types to report.
    #[arg(long, default_value_t = 15)]
    top: usize,
}

#[derive(Debug, Clone, Copy, ValueEnum)]
enum CompressionArg {
    None,
    Gzip,
    Zstd,
}

#[tokio::main]
async fn main() -> Result<()> {
    let args = Args::parse();

    let mut config = StreamConfig::hosted(args.network, Version::new(args.start));
    config.transactions_count = Some(args.count);
    config.batch_size = args.batch_size;
    config.api_key = args.api_key.clone().map(SecretString::from);
    config.compression = match args.compression {
        CompressionArg::None => None,
        CompressionArg::Gzip => Some(Compression::Gzip),
        CompressionArg::Zstd => Some(Compression::Zstd),
    };
    if !args.event_filters.is_empty() {
        config.filter = Some(event_filter_for(&args.event_filters)?);
    }

    let started = Instant::now();
    let mut stream = TransactionStream::connect(config)
        .await
        .context("opening the stream")?;
    let mut report = Report::default();

    while let Some(batch) = stream.next_batch().await? {
        if report.responses == 0 {
            report.first_response_ms = Some(started.elapsed().as_millis());
            report.chain_id = Some(batch.chain_id.get());
        }
        report.responses += 1;
        report.responses_with_processed_range += u64::from(batch.processed_range.is_some());
        let mut response_bytes = 0;
        for tx in &batch.transactions {
            let size = tx.encoded_len();
            response_bytes += size;
            report.observe(tx, size, &args)?;
        }
        report.max_response_bytes = report.max_response_bytes.max(response_bytes);
        report.max_response_txns = report.max_response_txns.max(batch.transactions.len());
    }

    let elapsed = started.elapsed().as_secs_f64();
    println!(
        "{}",
        serde_json::to_string_pretty(&report.finish(&args, elapsed, stream.next_version()))?
    );
    Ok(())
}

fn event_filter_for(types: &[String]) -> Result<BooleanTransactionFilter> {
    let filters = types
        .iter()
        .map(|ty| {
            let mut parts = ty.splitn(3, "::");
            let (Some(address), Some(module), Some(name)) =
                (parts.next(), parts.next(), parts.next())
            else {
                anyhow::bail!("event filter `{ty}` must look like address::module::name");
            };
            Ok(BooleanTransactionFilter {
                filter: Some(boolean_transaction_filter::Filter::ApiFilter(
                    nineveh_ingest::proto::indexer::ApiFilter {
                        filter: Some(
                            nineveh_ingest::proto::indexer::api_filter::Filter::EventFilter(
                                EventFilter {
                                    struct_type: Some(MoveStructTagFilter {
                                        address: Some(address.to_owned()),
                                        module: Some(module.to_owned()),
                                        name: Some(name.to_owned()),
                                    }),
                                    data_substring_filter: None,
                                },
                            ),
                        ),
                    },
                )),
            })
        })
        .collect::<Result<Vec<_>>>()?;
    Ok(BooleanTransactionFilter {
        filter: Some(boolean_transaction_filter::Filter::LogicalOr(
            LogicalOrFilters { filters },
        )),
    })
}

/// A rendering convention or record shape the decoder has to handle, with the first
/// place it was seen.
#[derive(Debug)]
struct Example {
    version: u64,
    type_str: String,
    snippet: String,
}

#[derive(Debug, Default)]
struct Report {
    responses: u64,
    responses_with_processed_range: u64,
    first_response_ms: Option<u128>,
    chain_id: Option<u8>,
    txns: u64,
    bytes: usize,
    max_txn_bytes: usize,
    max_response_bytes: usize,
    max_response_txns: usize,
    failed_txns: u64,
    txn_types: BTreeMap<String, u64>,
    change_kinds: BTreeMap<String, u64>,
    event_types: HashMap<String, u64>,
    resource_types: HashMap<String, u64>,
    table_value_types: HashMap<String, u64>,
    examples: BTreeMap<&'static str, Example>,
    captured: Vec<String>,
}

impl Report {
    fn observe(&mut self, tx: &Transaction, size: usize, args: &Args) -> Result<()> {
        self.txns += 1;
        self.bytes += size;
        self.max_txn_bytes = self.max_txn_bytes.max(size);

        let kind = TransactionType::try_from(tx.r#type).map_or("UNKNOWN", |t| t.as_str_name());
        *self.txn_types.entry(kind.to_owned()).or_default() += 1;

        let info = tx.info.as_ref();
        if info.is_some_and(|i| !i.success) {
            self.failed_txns += 1;
            self.example("txn.failed", tx.version, kind, "");
        }

        for event in events(tx) {
            *self.event_types.entry(event.type_str.clone()).or_default() += 1;
            self.observe_event(tx.version, event);
        }
        for change in info.map(|i| i.changes.as_slice()).unwrap_or_default() {
            self.observe_change(tx.version, change);
        }

        if let Some(dir) = &args.capture_dir {
            let wanted = args.capture_versions.contains(&tx.version)
                || self.examples.values().any(|e| e.version == tx.version);
            let name = format!("{}-{}.pb", args.network, tx.version);
            if wanted && !self.captured.contains(&name) {
                fs::create_dir_all(dir)?;
                fs::write(dir.join(&name), tx.encode_to_vec())?;
                self.captured.push(name);
            }
        }
        Ok(())
    }

    fn observe_event(&mut self, version: u64, event: &Event) {
        let ty = event.type_str.as_str();
        let is_module_event = event
            .key
            .as_ref()
            .is_none_or(|k| k.creation_number == 0 && is_zero_address(&k.account_address));
        let shape = if is_module_event {
            "event.module_event"
        } else {
            "event.handle_event"
        };
        self.example(shape, version, ty, &event.data);
        if ty.contains('<') {
            self.example("event.generic_type", version, ty, &event.data);
        }
        self.json_conventions(version, ty, &event.data);
    }

    fn observe_change(&mut self, version: u64, change: &WriteSetChange) {
        let Some(change) = &change.change else { return };
        let kind = match change {
            Change::WriteResource(r) => {
                *self.resource_types.entry(r.type_str.clone()).or_default() += 1;
                if r.type_str.contains('<') {
                    self.example("resource.generic_type", version, &r.type_str, &r.data);
                }
                match r.type_str.as_str() {
                    "0x1::object::ObjectCore" => {
                        self.example("resource.object_core", version, &r.type_str, &r.data);
                    }
                    "0x1::fungible_asset::FungibleStore" => {
                        self.example("resource.fungible_store", version, &r.type_str, &r.data);
                    }
                    _ => {}
                }
                self.json_conventions(version, &r.type_str, &r.data);
                "write_resource"
            }
            Change::DeleteResource(r) => {
                self.example("resource.delete", version, &r.type_str, "");
                "delete_resource"
            }
            Change::WriteTableItem(t) => {
                let (key_type, value_type, value) = t.data.as_ref().map_or(("", "", ""), |d| {
                    (d.key_type.as_str(), d.value_type.as_str(), d.value.as_str())
                });
                *self
                    .table_value_types
                    .entry(value_type.to_owned())
                    .or_default() += 1;
                let described = format!("{key_type} => {value_type}");
                self.example("table.write", version, &described, value);
                if value_type.contains("smart_table::") {
                    self.example("table.smart_table", version, &described, value);
                }
                if value_type.contains("big_ordered_map::")
                    || key_type.contains("big_ordered_map::")
                {
                    self.example("table.big_ordered_map", version, &described, value);
                }
                if t.data.is_none() {
                    self.example("table.write_without_data", version, &t.handle, &t.key);
                }
                self.json_conventions(version, &described, value);
                "write_table_item"
            }
            Change::DeleteTableItem(t) => {
                let described = t
                    .data
                    .as_ref()
                    .map_or_else(String::new, |d| d.key_type.clone());
                self.example("table.delete", version, &described, &t.key);
                "delete_table_item"
            }
            Change::WriteModule(m) => {
                self.example("module.write", version, &m.address, "");
                "write_module"
            }
            Change::DeleteModule(_) => "delete_module",
        };
        *self.change_kinds.entry(kind.to_owned()).or_default() += 1;
    }

    fn json_conventions(&mut self, version: u64, ty: &str, data: &str) {
        let markers: [(&'static str, &str); 4] = [
            ("json.option_vec", "{\"vec\":"),
            ("json.object_inner", "{\"inner\":\"0x"),
            ("json.enum_variant", "\"__variant__\""),
            ("json.table_handle", "{\"handle\":\"0x"),
        ];
        for (name, marker) in markers {
            if data.contains(marker) {
                self.example(name, version, ty, data);
            }
        }
    }

    fn example(&mut self, name: &'static str, version: u64, type_str: &str, data: &str) {
        self.examples.entry(name).or_insert_with(|| Example {
            version,
            type_str: type_str.to_owned(),
            snippet: data.chars().take(600).collect(),
        });
    }

    fn finish(self, args: &Args, elapsed_secs: f64, next_version: Version) -> Value {
        let top = |counts: HashMap<String, u64>| -> Vec<Value> {
            let mut counts: Vec<_> = counts.into_iter().collect();
            counts.sort_by(|a, b| b.1.cmp(&a.1).then_with(|| a.0.cmp(&b.0)));
            counts
                .into_iter()
                .take(args.top)
                .map(|(ty, n)| json!({ "type": ty, "count": n }))
                .collect()
        };
        #[allow(clippy::cast_precision_loss)]
        let (txns_per_sec, mib_per_sec) = (
            self.txns as f64 / elapsed_secs,
            self.bytes as f64 / elapsed_secs / (1024.0 * 1024.0),
        );
        let examples: BTreeMap<_, _> = self
            .examples
            .into_iter()
            .map(|(name, e)| {
                (
                    name,
                    json!({ "version": e.version, "type": e.type_str, "snippet": e.snippet }),
                )
            })
            .collect();

        json!({
            "network": args.network.as_str(),
            "chain_id": self.chain_id,
            "start": args.start,
            "next_version": next_version.get(),
            "compression": format!("{:?}", args.compression).to_lowercase(),
            "filtered": !args.event_filters.is_empty(),
            "throughput": {
                "elapsed_secs": (elapsed_secs * 100.0).round() / 100.0,
                "first_response_ms": self.first_response_ms,
                "txns": self.txns,
                "txns_per_sec": txns_per_sec.round(),
                "decoded_mib_per_sec": (mib_per_sec * 100.0).round() / 100.0,
                "avg_txn_bytes": self.bytes.checked_div(usize::try_from(self.txns).unwrap_or(usize::MAX)),
                "max_txn_bytes": self.max_txn_bytes,
            },
            "responses": {
                "count": self.responses,
                "with_processed_range": self.responses_with_processed_range,
                "max_bytes": self.max_response_bytes,
                "max_txns": self.max_response_txns,
            },
            "failed_txns": self.failed_txns,
            "txn_types": self.txn_types,
            "change_kinds": self.change_kinds,
            "top_event_types": top(self.event_types),
            "top_resource_types": top(self.resource_types),
            "top_table_value_types": top(self.table_value_types),
            "examples": examples,
            "captured": self.captured,
        })
    }
}

fn events(tx: &Transaction) -> &[Event] {
    match &tx.txn_data {
        Some(TxnData::User(u)) => &u.events,
        Some(TxnData::BlockMetadata(b)) => &b.events,
        Some(TxnData::Genesis(g)) => &g.events,
        Some(TxnData::Validator(v)) => &v.events,
        Some(TxnData::StateCheckpoint(_) | TxnData::BlockEpilogue(_)) | None => &[],
    }
}

fn is_zero_address(address: &str) -> bool {
    address.trim_start_matches("0x").chars().all(|c| c == '0')
}
