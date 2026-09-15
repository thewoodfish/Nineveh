//! A small client for the fullnode REST API: the ledger's current version, and module
//! ABIs with their bytecode.
//!
//! Only current state is read. The REST API is pruned (history starts around version
//! 11 billion on testnet), so anything historical comes from the stream
//! (`docs/research/spike-a-stream.md`).

use std::sync::Arc;
use std::time::Duration;

use nineveh_core::{Address, ChainId, Network, Version};
use reqwest::header::{AUTHORIZATION, HeaderMap, HeaderValue};
use secrecy::{ExposeSecret, SecretString};
use serde::Deserialize;

/// The ledger as a fullnode sees it now.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
#[non_exhaustive]
pub struct LedgerInfo {
    pub chain_id: ChainId,
    /// The latest committed version.
    pub ledger_version: Version,
    /// The oldest version the node still serves; earlier ones are pruned.
    pub oldest_ledger_version: Version,
    pub ledger_timestamp_micros: u64,
}

/// A published module: its bytecode and its ABI, as the fullnode renders it.
#[derive(Debug, Clone)]
#[non_exhaustive]
pub struct Module {
    pub bytecode: Vec<u8>,
    /// The `abi` object, for `nineveh_decode::ModuleAbi` to read.
    pub abi: serde_json::Value,
}

/// Why a REST request failed.
#[derive(Debug, thiserror::Error)]
#[non_exhaustive]
pub enum RestError {
    #[error("couldn't set up the HTTP client: {0}")]
    Client(String),

    #[error("the API key contains characters that can't be sent in an HTTP header")]
    InvalidApiKey,

    #[error("request to {url} failed: {source}")]
    Request {
        url: String,
        #[source]
        source: reqwest::Error,
    },

    #[error("{url} returned {status}: {message}")]
    Status {
        url: String,
        status: u16,
        message: String,
    },

    #[error("{url} returned a response Nineveh doesn't understand: {reason}")]
    Response { url: String, reason: String },
}

impl RestError {
    /// Whether trying again later can succeed: timeouts, dropped connections, rate
    /// limits and server errors.
    #[must_use]
    pub fn is_retryable(&self) -> bool {
        match self {
            Self::Request { source, .. } => {
                source.is_timeout() || source.is_connect() || source.is_request()
            }
            Self::Status { status, .. } => *status == 429 || *status >= 500,
            Self::Client(_) | Self::InvalidApiKey | Self::Response { .. } => false,
        }
    }
}

/// A fullnode REST API endpoint, such as Aptos Labs' hosted one.
#[derive(Debug, Clone)]
pub struct RestClient {
    http: reqwest::Client,
    /// Ends in `/v1`, without a trailing slash.
    base: String,
}

impl RestClient {
    /// Aptos Labs' hosted REST API for `network`. A Geomi API key raises the rate
    /// limit.
    ///
    /// # Errors
    ///
    /// If the key can't be sent as a header or the HTTP client can't be built.
    pub fn hosted(network: Network, api_key: Option<&SecretString>) -> Result<Self, RestError> {
        Self::new(format!("https://api.{network}.aptoslabs.com/v1"), api_key)
    }

    /// Any fullnode's REST API, given its `/v1` base URL.
    ///
    /// # Errors
    ///
    /// If the key can't be sent as a header or the HTTP client can't be built.
    pub fn new(base: impl Into<String>, api_key: Option<&SecretString>) -> Result<Self, RestError> {
        let mut headers = HeaderMap::new();
        if let Some(key) = api_key {
            let mut value = HeaderValue::from_str(&format!("Bearer {}", key.expose_secret()))
                .map_err(|_| RestError::InvalidApiKey)?;
            value.set_sensitive(true);
            headers.insert(AUTHORIZATION, value);
        }
        let http = reqwest::Client::builder()
            .use_preconfigured_tls(tls()?)
            .default_headers(headers)
            .user_agent(concat!("nineveh/", env!("CARGO_PKG_VERSION")))
            .connect_timeout(Duration::from_secs(10))
            .timeout(Duration::from_secs(60))
            .build()
            .map_err(|e| RestError::Client(e.to_string()))?;
        let base = base.into().trim_end_matches('/').to_owned();
        Ok(Self { http, base })
    }

    /// The ledger's current version and chain id.
    ///
    /// # Errors
    ///
    /// If the request fails or the response isn't ledger info.
    pub async fn ledger(&self) -> Result<LedgerInfo, RestError> {
        let url = self.base.clone();
        let text = self.get(&url).await?.ok_or_else(|| RestError::Status {
            url: url.clone(),
            status: 404,
            message: "not found".into(),
        })?;
        parse_ledger(&text).map_err(|reason| RestError::Response { url, reason })
    }

    /// The module `address::name`, or `None` if it isn't published.
    ///
    /// # Errors
    ///
    /// If the request fails or the response isn't a module.
    pub async fn module(&self, address: Address, name: &str) -> Result<Option<Module>, RestError> {
        let url = format!("{}/accounts/{address}/module/{name}", self.base);
        let Some(text) = self.get(&url).await? else {
            return Ok(None);
        };
        parse_module(&text)
            .map(Some)
            .map_err(|reason| RestError::Response { url, reason })
    }

    /// The body of a `GET`, or `None` for a 404 that means "no such thing".
    async fn get(&self, url: &str) -> Result<Option<String>, RestError> {
        let failed = |source| RestError::Request {
            url: url.to_owned(),
            source,
        };
        let response = self.http.get(url).send().await.map_err(failed)?;
        let status = response.status();
        let text = response.text().await.map_err(failed)?;
        if status.is_success() {
            return Ok(Some(text));
        }
        let error: ApiError = serde_json::from_str(&text).unwrap_or_default();
        if status.as_u16() == 404
            && matches!(
                error.error_code.as_deref(),
                Some("module_not_found" | "account_not_found" | "resource_not_found")
            )
        {
            return Ok(None);
        }
        Err(RestError::Status {
            url: url.to_owned(),
            status: status.as_u16(),
            message: error.message.unwrap_or(text),
        })
    }
}

/// rustls with ring and the webpki roots, as the stream client uses.
fn tls() -> Result<rustls::ClientConfig, RestError> {
    let roots = rustls::RootCertStore {
        roots: webpki_roots::TLS_SERVER_ROOTS.to_vec(),
    };
    let mut config = rustls::ClientConfig::builder_with_provider(Arc::new(
        rustls::crypto::ring::default_provider(),
    ))
    .with_safe_default_protocol_versions()
    .map_err(|e| RestError::Client(e.to_string()))?
    .with_root_certificates(roots)
    .with_no_client_auth();
    config.alpn_protocols = vec![b"http/1.1".to_vec()];
    Ok(config)
}

#[derive(Debug, Default, Deserialize)]
struct ApiError {
    message: Option<String>,
    error_code: Option<String>,
}

#[derive(Deserialize)]
struct RawLedger {
    chain_id: u64,
    ledger_version: String,
    oldest_ledger_version: String,
    ledger_timestamp: String,
}

fn parse_ledger(text: &str) -> Result<LedgerInfo, String> {
    let raw: RawLedger = serde_json::from_str(text).map_err(|e| e.to_string())?;
    let number = |field: &str, value: &str| {
        value
            .parse::<u64>()
            .map_err(|_| format!("`{field}` isn't a u64: {value:?}"))
    };
    Ok(LedgerInfo {
        chain_id: ChainId::try_from(raw.chain_id).map_err(|e| e.to_string())?,
        ledger_version: Version::new(number("ledger_version", &raw.ledger_version)?),
        oldest_ledger_version: Version::new(number(
            "oldest_ledger_version",
            &raw.oldest_ledger_version,
        )?),
        ledger_timestamp_micros: number("ledger_timestamp", &raw.ledger_timestamp)?,
    })
}

#[derive(Deserialize)]
struct RawModule {
    bytecode: String,
    abi: serde_json::Value,
}

fn parse_module(text: &str) -> Result<Module, String> {
    let raw: RawModule = serde_json::from_str(text).map_err(|e| e.to_string())?;
    let hex = raw
        .bytecode
        .strip_prefix("0x")
        .ok_or("`bytecode` isn't 0x-prefixed hex")?;
    Ok(Module {
        bytecode: from_hex(hex).ok_or("`bytecode` isn't hex")?,
        abi: raw.abi,
    })
}

fn from_hex(hex: &str) -> Option<Vec<u8>> {
    if !hex.len().is_multiple_of(2) {
        return None;
    }
    hex.as_bytes()
        .chunks(2)
        .map(|pair| {
            let digit = |b: u8| char::from(b).to_digit(16);
            let byte = digit(pair[0])? * 16 + digit(pair[1])?;
            u8::try_from(byte).ok()
        })
        .collect()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn parses_ledger_info() {
        // Trimmed from testnet's `GET /v1`, 2026-09-15.
        let text = r#"{"chain_id":2,"epoch":"35737","ledger_version":"11214295950",
            "oldest_ledger_version":"11064495985","ledger_timestamp":"1789474380546205",
            "node_role":"full_node","oldest_block_height":"849011625","block_height":"855992690"}"#;
        let ledger = parse_ledger(text).unwrap();
        assert_eq!(ledger.chain_id, ChainId::TESTNET);
        assert_eq!(ledger.ledger_version, Version::new(11_214_295_950));
        assert_eq!(ledger.oldest_ledger_version, Version::new(11_064_495_985));
        assert_eq!(ledger.ledger_timestamp_micros, 1_789_474_380_546_205);
        assert!(parse_ledger(r#"{"chain_id":2}"#).is_err());
    }

    #[test]
    fn parses_modules() {
        let module = parse_module(r#"{"bytecode":"0xa11ceb0b","abi":{"name":"m"}}"#).unwrap();
        assert_eq!(module.bytecode, [0xa1, 0x1c, 0xeb, 0x0b]);
        assert_eq!(module.abi["name"], "m");
        assert!(parse_module(r#"{"bytecode":"0xa1c","abi":{}}"#).is_err());
        assert!(parse_module(r#"{"bytecode":"a11c","abi":{}}"#).is_err());
        assert!(parse_module(r#"{"bytecode":"0xzz","abi":{}}"#).is_err());
    }

    #[test]
    fn rate_limits_and_server_errors_are_retryable() {
        let status = |status| RestError::Status {
            url: String::new(),
            status,
            message: String::new(),
        };
        assert!(status(429).is_retryable());
        assert!(status(503).is_retryable());
        assert!(!status(400).is_retryable());
        assert!(!status(401).is_retryable());
    }
}
