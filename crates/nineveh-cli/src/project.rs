//! Reading `nineveh.yaml` and `nineveh.lock`, with problems rendered at their lines.

use std::fs;
use std::path::{Path, PathBuf};

use anyhow::{Context, Result};
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

/// A parsed config and the text of every file it came from, in the order spans number
/// them: `nineveh.yaml` first, then the `reducers:` file if there is one (ADR 0025).
pub(crate) struct Source {
    pub(crate) config: Config,
    pub(crate) text: String,
    pub(crate) name: String,
    /// The DSL file's name and text, when the config names one.
    pub(crate) reducers: Option<(String, String)>,
}

impl Source {
    /// Every file, for [`Diagnostics::render_files`](nineveh_config::Diagnostics).
    pub(crate) fn files(&self) -> Vec<(&str, &str)> {
        let mut files = vec![(self.name.as_str(), self.text.as_str())];
        if let Some((name, text)) = &self.reducers {
            files.push((name.as_str(), text.as_str()));
        }
        files
    }
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
    let config = match parse(&text) {
        Ok(config) => config,
        Err(diagnostics) => return Err(fail(&diagnostics, &[(&name, &text)])),
    };
    let Some(relative) = config.reducers.clone() else {
        return Ok(Source {
            config,
            text,
            name,
            reducers: None,
        });
    };

    // The `reducers:` file is named relative to the config, so a project moves whole.
    let dsl_path = path
        .parent()
        .unwrap_or_else(|| Path::new("."))
        .join(&relative);
    let dsl_name = display_name(&dsl_path);
    let dsl_text = fs::read_to_string(&dsl_path).with_context(|| {
        format!(
            "reading {}, which {name} names as its `reducers`",
            dsl_path.display()
        )
    })?;
    match nineveh_dsl::merge(config, &dsl_text) {
        Ok(config) => Ok(Source {
            config,
            text,
            name,
            reducers: Some((dsl_name, dsl_text)),
        }),
        Err(diagnostics) => Err(fail(
            &diagnostics,
            &[(&name, &text), (&dsl_name, &dsl_text)],
        )),
    }
}

pub(crate) fn load(paths: &Paths) -> Result<Loaded> {
    // `resolve` takes the config, so the sources are held aside to report against.
    let Source {
        config,
        text,
        name,
        reducers,
    } = read_config(&paths.config)?;
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
                "{name} says `start_version: auto`, but {} doesn\'t pin a start; run \
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
            let mut files = vec![(name.as_str(), text.as_str())];
            if let Some((dsl_name, dsl_text)) = &reducers {
                files.push((dsl_name.as_str(), dsl_text.as_str()));
            }
            Err(fail(&diagnostics, &files))
        }
    }
}

/// Report every problem against the file it's in, and fail.
fn fail(diagnostics: &nineveh_config::Diagnostics, files: &[(&str, &str)]) -> anyhow::Error {
    report(&diagnostics.render_files(files));
    let name = files.first().map_or("nineveh.yaml", |(name, _)| *name);
    anyhow::anyhow!("{name} has {} problem(s)", diagnostics.as_slice().len())
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
