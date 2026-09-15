//! Everything Nineveh reads from Aptos to set up and run a project, behind one trait:
//! Aptos Labs' hosted APIs in production, fixtures in tests.

use std::future::Future;

use nineveh_config::Project;
use nineveh_core::{Address, Identifier, Network, StructName, Version};
use nineveh_decode::{MetadataError, ModuleAbi, resource_group_members};
use nineveh_ingest::{RestClient, RestError, StreamConfig};
use nineveh_pipeline::{Source, StreamSource, stream_filter};
use secrecy::SecretString;

/// A module's ABI, and the resource groups its structs belong to (from its bytecode's
/// metadata), as pinning needs them.
#[derive(Debug, Clone)]
pub struct ModuleInfo {
    pub abi: ModuleAbi,
    /// `(member struct, its group)` for each of the module's resource-group members.
    pub groups: Vec<(Identifier, StructName)>,
}

/// The chain as Nineveh needs it: current state from the REST and Indexer APIs, and
/// history from the Transaction Stream.
pub trait Chain: Send + Sync + 'static {
    type Source: Source + 'static;

    /// The chain's latest committed version.
    fn tip(&self, network: Network) -> impl Future<Output = Result<Version, ChainError>> + Send;

    /// The ABIs of every module published at `address`.
    fn modules(
        &self,
        network: Network,
        address: Address,
    ) -> impl Future<Output = Result<Vec<ModuleAbi>, ChainError>> + Send;

    /// The module `address::name`, or `None` if it isn't published.
    fn module(
        &self,
        network: Network,
        address: Address,
        name: &str,
    ) -> impl Future<Output = Result<Option<ModuleInfo>, ChainError>> + Send;

    /// The first transaction that touched `address`, or `None` if none has (ADR 0015).
    fn first_transaction(
        &self,
        network: Network,
        address: Address,
    ) -> impl Future<Output = Result<Option<Version>, ChainError>> + Send;

    /// Where `project`'s transactions stream from, starting at `start`.
    fn source(&self, network: Network, start: Version, project: &Project) -> Self::Source;
}

#[derive(Debug, thiserror::Error)]
#[non_exhaustive]
pub enum ChainError {
    #[error(transparent)]
    Rest(#[from] RestError),

    #[error("reading the ABI of `{module}`: {source}")]
    Abi {
        module: String,
        #[source]
        source: serde_json::Error,
    },

    #[error("reading the metadata of `{module}`: {source}")]
    Metadata {
        module: String,
        #[source]
        source: MetadataError,
    },

    #[error("the {network} REST API reports chain {reported}, not {expected}")]
    WrongChain {
        network: Network,
        reported: String,
        expected: String,
    },
}

impl ChainError {
    /// Timeouts, dropped connections, rate limits and server errors can pass. A module
    /// Nineveh can't read, or the wrong chain, won't.
    #[must_use]
    pub fn is_retryable(&self) -> bool {
        match self {
            Self::Rest(e) => e.is_retryable(),
            Self::Abi { .. } | Self::Metadata { .. } | Self::WrongChain { .. } => false,
        }
    }
}

/// Aptos Labs' hosted APIs, with a Geomi API key.
#[derive(Debug, Clone)]
pub struct Hosted {
    api_key: Option<SecretString>,
}

impl Hosted {
    #[must_use]
    pub fn new(api_key: Option<SecretString>) -> Self {
        Self { api_key }
    }

    fn rest(&self, network: Network) -> Result<RestClient, ChainError> {
        Ok(RestClient::hosted(network, self.api_key.as_ref())?)
    }
}

impl Chain for Hosted {
    type Source = StreamSource;

    async fn tip(&self, network: Network) -> Result<Version, ChainError> {
        let ledger = self.rest(network)?.ledger().await?;
        if let Some(expected) = network.chain_id()
            && ledger.chain_id != expected
        {
            return Err(ChainError::WrongChain {
                network,
                reported: ledger.chain_id.to_string(),
                expected: expected.to_string(),
            });
        }
        Ok(ledger.ledger_version)
    }

    async fn modules(
        &self,
        network: Network,
        address: Address,
    ) -> Result<Vec<ModuleAbi>, ChainError> {
        self.rest(network)?
            .modules(address)
            .await?
            .into_iter()
            .map(|module| {
                let name = module.abi["name"].as_str().unwrap_or("?").to_owned();
                serde_json::from_value(module.abi).map_err(|source| ChainError::Abi {
                    module: format!("{}::{name}", address.to_standard_string()),
                    source,
                })
            })
            .collect()
    }

    async fn module(
        &self,
        network: Network,
        address: Address,
        name: &str,
    ) -> Result<Option<ModuleInfo>, ChainError> {
        let Some(module) = self.rest(network)?.module(address, name).await? else {
            return Ok(None);
        };
        let id = format!("{}::{name}", address.to_standard_string());
        let abi = serde_json::from_value(module.abi).map_err(|source| ChainError::Abi {
            module: id.clone(),
            source,
        })?;
        let groups = resource_group_members(&module.bytecode)
            .map_err(|source| ChainError::Metadata { module: id, source })?;
        Ok(Some(ModuleInfo { abi, groups }))
    }

    async fn first_transaction(
        &self,
        network: Network,
        address: Address,
    ) -> Result<Option<Version>, ChainError> {
        Ok(self.rest(network)?.first_transaction(address).await?)
    }

    fn source(&self, network: Network, start: Version, project: &Project) -> StreamSource {
        let mut stream = StreamConfig::hosted(network, start);
        stream.api_key.clone_from(&self.api_key);
        stream.filter = stream_filter(project);
        StreamSource::new(stream)
    }
}
