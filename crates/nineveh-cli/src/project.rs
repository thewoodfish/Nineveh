//! Reading `nineveh.yaml` and `nineveh.lock`, with problems rendered at their lines.

use std::fs;
use std::path::{Path, PathBuf};

use anyhow::{Context, Result, bail};
use nineveh_config::{Config, Project, StartVersion, parse};
use nineveh_core::Version;
use nineveh_decode::Lockfile;

/// Where a project's files are.
#[derive(Debug, Clone)]
pub(crate) struct Paths {
    pub(crate) config: PathBuf,
    pub(crate) lock: PathBuf,
}

impl Paths {
    /// `config`, and the lock beside it unless one is given.
    pub(crate) fn new(config: PathBuf, lock: Option<PathBuf>) -> Self {
        let lock = lock.unwrap_or_else(|| config.with_file_name("nineveh.lock"));
        Self { config, lock }
    }
}

/// A parsed config and its text, for rendering problems.
pub(crate) struct Source {
    pub(crate) config: Config,
    pub(crate) text: String,
    pub(crate) name: String,
}

/// A project ready to run: config resolved against its lock.
pub(crate) struct Loaded {
    pub(crate) project: Project,
    pub(crate) lock: Lockfile,
    /// Where a new build starts: the config's `start_version`, or for `auto`, the
    /// version `nineveh init` pinned in the lock (ADR 0015).
    pub(crate) start: Version,
}

pub(crate) fn read_config(path: &Path) -> Result<Source> {
    let text = fs::read_to_string(path).with_context(|| format!("reading {}", path.display()))?;
    let name = display_name(path);
    match parse(&text) {
        Ok(config) => Ok(Source { config, text, name }),
        Err(diagnostics) => {
            report(&diagnostics.render(&name, &text));
            bail!("{name} has {} problem(s)", diagnostics.as_slice().len())
        }
    }
}

pub(crate) fn load(paths: &Paths) -> Result<Loaded> {
    let Source { config, text, name } = read_config(&paths.config)?;
    let lock_text = fs::read_to_string(&paths.lock).with_context(|| {
        format!(
            "reading {}; run `nineveh init` to pin the layouts {name} needs",
            paths.lock.display()
        )
    })?;
    let lock = Lockfile::from_json(&lock_text)
        .with_context(|| format!("reading {}", paths.lock.display()))?;
    let start = match config.start_version {
        StartVersion::Version(v) => Version::new(v),
        StartVersion::Auto => lock.start_version().with_context(|| {
            format!(
                "{name} says `start_version: auto`, but {} doesn't pin a start; run \
                 `nineveh init` to resolve it",
                paths.lock.display()
            )
        })?,
    };
    match config.resolve(&lock) {
        Ok(project) => Ok(Loaded {
            project,
            lock,
            start,
        }),
        Err(diagnostics) => {
            report(&diagnostics.render(&name, &text));
            bail!("{name} has {} problem(s)", diagnostics.as_slice().len())
        }
    }
}

fn display_name(path: &Path) -> String {
    path.file_name().map_or_else(
        || path.display().to_string(),
        |n| n.to_string_lossy().into_owned(),
    )
}

/// Print rendered diagnostics for the user.
#[allow(clippy::print_stderr, reason = "the CLI reports problems on stderr")]
pub(crate) fn report(rendered: &str) {
    eprintln!("{rendered}");
}
