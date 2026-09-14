use std::time::Duration;

use nineveh_core::{ChainId, Network, Version};
use secrecy::SecretString;

use crate::proto::indexer::BooleanTransactionFilter;

/// Default cap on a single decoded response. Large transactions (module publishes, big
/// write sets) can make a batch several megabytes, far over tonic's 4 MiB default. The
/// cap is still bounded so a bad response can't exhaust memory.
pub const DEFAULT_MAX_MESSAGE_SIZE: usize = 128 * 1024 * 1024;

/// Where and how to open a Transaction Stream.
#[derive(Debug, Clone)]
#[non_exhaustive]
pub struct StreamConfig {
    /// gRPC endpoint, e.g. `https://grpc.testnet.aptoslabs.com:443`.
    pub endpoint: String,
    /// Geomi API key, sent as `authorization: Bearer <key>`. Anonymous access works,
    /// but at a low per-IP rate limit.
    pub api_key: Option<SecretString>,
    /// Refuse the stream if it reports a different chain. Guards against pointing a
    /// testnet project at mainnet data.
    pub expected_chain_id: Option<ChainId>,
    /// First version to deliver.
    pub starting_version: Version,
    /// Stop after covering this many versions. `None` streams indefinitely. With a
    /// filter, the server counts versions *scanned*, not transactions matched: on
    /// mainnet, 5,000 versions yielded 65 matches.
    pub transactions_count: Option<u64>,
    /// Transactions per response, if overriding the server's default.
    pub batch_size: Option<u64>,
    /// Server-side filter. Only transactions that match are delivered. The filter can
    /// match events, senders and entry functions, but not write set changes.
    pub filter: Option<BooleanTransactionFilter>,
    /// Response compression to request from the server. Defaults to zstd: the stream
    /// is mostly JSON text, and on testnet zstd delivered 22× the transactions per
    /// second of an uncompressed stream (see `docs/research/spike-a-stream.md`).
    pub compression: Option<Compression>,
    /// Upper bound on one decoded response, in bytes.
    pub max_message_size: usize,
    pub connect_timeout: Duration,
}

impl StreamConfig {
    /// Stream from Aptos Labs' hosted Transaction Stream for `network`.
    #[must_use]
    pub fn hosted(network: Network, starting_version: Version) -> Self {
        let mut config = Self::new(
            format!("https://grpc.{network}.aptoslabs.com:443"),
            starting_version,
        );
        config.expected_chain_id = network.chain_id();
        config
    }

    /// Stream from any Transaction Stream-compatible endpoint.
    #[must_use]
    pub fn new(endpoint: impl Into<String>, starting_version: Version) -> Self {
        Self {
            endpoint: endpoint.into(),
            api_key: None,
            expected_chain_id: None,
            starting_version,
            transactions_count: None,
            batch_size: None,
            filter: None,
            compression: Some(Compression::Zstd),
            max_message_size: DEFAULT_MAX_MESSAGE_SIZE,
            connect_timeout: Duration::from_secs(10),
        }
    }
}

/// Response compression supported by the client.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Compression {
    Gzip,
    Zstd,
}

impl From<Compression> for tonic::codec::CompressionEncoding {
    fn from(compression: Compression) -> Self {
        match compression {
            Compression::Gzip => Self::Gzip,
            Compression::Zstd => Self::Zstd,
        }
    }
}
