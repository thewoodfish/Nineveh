//! `nineveh init`: pin the layouts a project needs, and resolve `start_version: auto`.

use std::collections::BTreeSet;
use std::fs;
use std::future::Future;
use std::path::Path;
use std::time::Duration;

use anyhow::{Context, Result, anyhow, bail};
use nineveh_config::{Config, SourceKind, StartVersion};
use nineveh_core::{Address, StructName, Version};
use nineveh_decode::{BuildError, LockBuilder, Lockfile, ModuleAbi, resource_group_members};
use nineveh_ingest::{RestClient, RestError};
use secrecy::SecretString;
use tracing::{info, warn};

use crate::project::{Paths, read_config, report};

pub(crate) async fn init(paths: &Paths, api_key: Option<&SecretString>) -> Result<()> {
    let source = read_config(&paths.config)?;
    let config = &source.config;
    let rest = RestClient::hosted(config.network, api_key)?;

    let lock = pin_layouts(&rest, config).await?;
    let start = match config.start_version {
        StartVersion::Auto => Some(resolve_start(&rest, config).await?),
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
    let lock = lock.with_start_version(start);
    let text = lock.to_json()?;
    let unchanged = fs::read_to_string(&paths.lock).is_ok_and(|old| old == text);
    if unchanged {
        info!(lock = %paths.lock.display(), "up to date");
    } else {
        write_atomically(&paths.lock, &text)?;
        info!(
            lock = %paths.lock.display(),
            structs = lock.structs().count(),
            start = ?start.map(Version::get),
            "wrote"
        );
    }

    // Report config problems now, with the lock in hand, rather than at `run`.
    if let Err(diagnostics) = source.config.resolve(&lock) {
        report(&diagnostics.render(&source.name, &source.text));
        bail!(
            "{} has {} problem(s); the lock was written",
            source.name,
            diagnostics.as_slice().len()
        );
    }
    Ok(())
}

/// Fetch every module the config's structs need, until the lock is closed.
async fn pin_layouts(rest: &RestClient, config: &Config) -> Result<Lockfile> {
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
                bail!("module `{id}` is still missing after fetching it");
            }
            info!(module = %id, "fetching");
            let module = retry(|| rest.module(id.address, id.name.as_str()))
                .await?
                .ok_or_else(|| anyhow!("module `{id}` isn't published on {}", config.network))?;
            let abi: ModuleAbi = serde_json::from_value(module.abi)
                .with_context(|| format!("reading the ABI of `{id}`"))?;
            let members = resource_group_members(&module.bytecode)
                .with_context(|| format!("reading the metadata of `{id}`"))?;
            for (member, group) in members {
                builder.set_group(StructName::new(id.address, id.name.clone(), member), group);
            }
            builder.add_module(abi);
        }
    }
}

/// `start_version: auto`: the earliest first transaction of the sources' addresses
/// (ADR 0015).
async fn resolve_start(rest: &RestClient, config: &Config) -> Result<Version> {
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
        let first = retry(|| rest.first_transaction(address))
            .await?
            .ok_or_else(|| {
                anyhow!(
                    "no transaction on {} has touched {}; check the address, or set \
                     `start_version` explicitly",
                    config.network,
                    address.to_standard_string()
                )
            })?;
        info!(address = %address.to_standard_string(), first = first.get(), "first used");
        start = Some(start.map_or(first, |s| s.min(first)));
    }
    start.context("the config has no sources")
}

/// Call `f` until it succeeds or fails in a way retrying can't fix.
async fn retry<T, F, Fut>(mut f: F) -> Result<T, RestError>
where
    F: FnMut() -> Fut,
    Fut: Future<Output = Result<T, RestError>>,
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

/// Write `text` to `path` through a temporary file, so a crash never leaves half a
/// lock.
fn write_atomically(path: &Path, text: &str) -> Result<()> {
    let temporary = path.with_extension("lock.tmp");
    fs::write(&temporary, text).with_context(|| format!("writing {}", temporary.display()))?;
    fs::rename(&temporary, path).with_context(|| format!("writing {}", path.display()))?;
    Ok(())
}
