//! `nineveh init`: pin the layouts a project needs, and resolve `start_version: auto`.

use std::fs;
use std::path::Path;

use anyhow::{Context, Result, bail};
use nineveh_control::{Hosted, pin};
use nineveh_core::Version;
use secrecy::SecretString;
use tracing::info;

use crate::project::{Paths, read_config, report};

pub(crate) async fn init(paths: &Paths, api_key: Option<&SecretString>) -> Result<()> {
    let source = read_config(&paths.config)?;
    let chain = Hosted::new(api_key.cloned());
    let lock = pin(&chain, &source.config).await?;
    let text = lock.to_json()?;
    let unchanged = fs::read_to_string(&paths.lock).is_ok_and(|old| old == text);
    if unchanged {
        info!(lock = %paths.lock.display(), "up to date");
    } else {
        write_atomically(&paths.lock, &text)?;
        info!(
            lock = %paths.lock.display(),
            structs = lock.structs().count(),
            start = ?lock.start_version().map(Version::get),
            "wrote"
        );
    }

    // Report config problems now, with the lock in hand, rather than at `run`.
    let files = source.files();
    let has_reducers = source.config.reducers.is_some();
    let project = match source.config.clone().resolve(&lock) {
        Ok(project) => project,
        Err(diagnostics) => {
            report(&diagnostics.render_files(&files));
            bail!(
                "{} has {} problem(s); the lock was written",
                source.name,
                diagnostics.as_slice().len()
            );
        }
    };

    // The declarations the editor reads, regenerated from what was just pinned. Only
    // for a project that has a DSL file to open (ADR 0025).
    if has_reducers {
        let path = paths.config.with_file_name("nineveh.d.ts");
        let text = nineveh_dsl::declarations_for(&project, &lock);
        if fs::read_to_string(&path).is_ok_and(|old| old == text) {
            info!(declarations = %path.display(), "up to date");
        } else {
            write_atomically(&path, &text)?;
            info!(declarations = %path.display(), "wrote");
        }
    }
    Ok(())
}

/// Write `text` to `path` through a temporary file, so a crash never leaves half a
/// lock.
fn write_atomically(path: &Path, text: &str) -> Result<()> {
    let mut name = path.file_name().unwrap_or_default().to_os_string();
    name.push(".tmp");
    let temporary = path.with_file_name(name);
    fs::write(&temporary, text).with_context(|| format!("writing {}", temporary.display()))?;
    fs::rename(&temporary, path).with_context(|| format!("writing {}", path.display()))?;
    Ok(())
}
