//! Pinning a config's layouts in a lock, and resolving `start_version: auto`: what
//! `nineveh init` does, and what creating a project in Studio does.

use std::collections::BTreeSet;
use std::future::Future;
use std::time::Duration;

use nineveh_config::{Config, SourceKind, StartVersion};
use nineveh_core::{Address, Network, StructName, Version};
use nineveh_decode::{BuildError, LockBuilder, Lockfile, ModuleId};
use tracing::{info, warn};

use crate::chain::{Chain, ChainError};

#[derive(Debug, thiserror::Error)]
#[non_exhaustive]
pub enum PinError {
    #[error(transparent)]
    Chain(#[from] ChainError),

    #[error(transparent)]
    Build(#[from] BuildError),

    #[error("module `{0}` isn't published on {1}")]
    NotPublished(ModuleId, Network),

    #[error("module `{0}` is still missing after fetching it")]
    StillMissing(ModuleId),

    #[error(
        "no transaction on {network} has touched {address}; check the address, or set \
         `start_version` explicitly"
    )]
    NeverUsed { network: Network, address: String },

    #[error("the config has no sources")]
    NoSources,
}

impl PinError {
    /// Only a failed request can pass on a retry; the rest are about the config or the
    /// chain's contents.
    #[must_use]
    pub fn is_retryable(&self) -> bool {
        match self {
            Self::Chain(e) => e.is_retryable(),
            Self::Build(_)
            | Self::NotPublished(..)
            | Self::StillMissing(_)
            | Self::NeverUsed { .. }
            | Self::NoSources => false,
        }
    }
}

/// The lock for `config`: every layout its structs need, fetched from the chain, and
/// for `start_version: auto`, the start (ADR 0015).
///
/// # Errors
///
/// If a module can't be fetched or read, a struct isn't in its module, or no
/// transaction has touched a source's address.
pub async fn pin<C: Chain>(chain: &C, config: &Config) -> Result<Lockfile, PinError> {
    let lock = pin_layouts(chain, config).await?;
    let start = match config.start_version {
        StartVersion::Auto => Some(resolve_start(chain, config).await?),
        StartVersion::Version(v) => {
            if config
                .sources
                .iter()
                .any(|s| matches!(s.kind, SourceKind::Table { .. }))
            {
                warn!(
                    "`start_version: {v}` is explicit: table sources learn their tables from \
                     the parents' writes, so tables created before it won't be followed \
                     (ADR 0012); `auto` starts early enough"
                );
            }
            None
        }
    };
    Ok(lock.with_start_version(start))
}

/// Fetch every module the config's structs need, until the lock is closed.
async fn pin_layouts<C: Chain>(chain: &C, config: &Config) -> Result<Lockfile, PinError> {
    let roots = config.roots();
    let mut builder = LockBuilder::new(config.network);
    let mut fetched = BTreeSet::new();
    loop {
        let missing = match builder.build(&roots) {
            Ok(lock) => return Ok(lock),
            Err(BuildError::MissingModules(missing)) => missing,
            Err(error) => return Err(error.into()),
        };
        for id in missing {
            if !fetched.insert(id.clone()) {
                return Err(PinError::StillMissing(id));
            }
            info!(module = %id, "fetching");
            let module = retry(|| chain.module(config.network, id.address, id.name.as_str()))
                .await?
                .ok_or_else(|| PinError::NotPublished(id.clone(), config.network))?;
            for (member, group) in module.groups {
                builder.set_group(StructName::new(id.address, id.name.clone(), member), group);
            }
            builder.add_module(module.abi);
        }
    }
}

/// `start_version: auto`: the earliest first transaction of the sources' addresses
/// (ADR 0015).
async fn resolve_start<C: Chain>(chain: &C, config: &Config) -> Result<Version, PinError> {
    let addresses: BTreeSet<Address> = config
        .sources
        .iter()
        .map(|source| match &source.kind {
            SourceKind::Event(tag) | SourceKind::Resource(tag) => tag.name.address,
            SourceKind::Table { parent, .. } => parent.name.address,
        })
        .collect();
    let mut start: Option<Version> = None;
    for address in addresses {
        let first = retry(|| chain.first_transaction(config.network, address))
            .await?
            .ok_or_else(|| PinError::NeverUsed {
                network: config.network,
                address: address.to_standard_string(),
            })?;
        info!(address = %address.to_standard_string(), first = first.get(), "first used");
        start = Some(start.map_or(first, |s| s.min(first)));
    }
    start.ok_or(PinError::NoSources)
}

/// Call `f` until it succeeds or fails in a way retrying can't fix.
pub(crate) async fn retry<T, F, Fut>(mut f: F) -> Result<T, ChainError>
where
    F: FnMut() -> Fut,
    Fut: Future<Output = Result<T, ChainError>>,
{
    let mut delay = Duration::from_millis(250);
    for _ in 0..4 {
        match f().await {
            Err(error) if error.is_retryable() => {
                warn!(%error, ?delay, "retrying");
                tokio::time::sleep(delay).await;
                delay = delay.saturating_mul(2);
            }
            result => return result,
        }
    }
    f().await
}
