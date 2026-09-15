//! `nineveh`: pin a project's layouts, validate it, and run or replay its pipeline.
//!
//! ```text
//! nineveh init                 # fetch ABIs, write nineveh.lock, resolve start_version: auto
//! nineveh validate             # check nineveh.yaml against the lock, offline
//! nineveh run                  # build the state in Postgres and keep it current
//! nineveh replay --yes         # drop the state and build it again
//! ```
//!
//! `APTOS_API_KEY` is a Geomi key for the Transaction Stream and the REST and Indexer
//! APIs. `NINEVEH_DATABASE_URL` is the Postgres to build into. (Not `DATABASE_URL`: in a
//! source checkout that switches sqlx's macros to checking against a live database.)

use std::path::PathBuf;
use std::process::ExitCode;

use anyhow::Result;
use clap::{Parser, Subcommand};
use secrecy::SecretString;
use tracing_subscriber::EnvFilter;

mod init;
mod project;
mod run;

use project::Paths;
use run::RunOptions;

#[derive(Debug, Parser)]
#[command(name = "nineveh", version, about = "A reactive backend for Aptos apps")]
struct Cli {
    /// The project's config.
    #[arg(long, global = true, default_value = "nineveh.yaml")]
    config: PathBuf,
    /// The lock. Defaults to `nineveh.lock` beside the config.
    #[arg(long, global = true)]
    lock: Option<PathBuf>,
    #[command(subcommand)]
    command: Command,
}

#[derive(Debug, Subcommand)]
enum Command {
    /// Fetch the ABIs the config needs, pin them in nineveh.lock, and resolve
    /// `start_version: auto`.
    Init {
        #[command(flatten)]
        key: ApiKey,
    },
    /// Check nineveh.yaml against nineveh.lock and report every problem. Offline.
    Validate,
    /// Build the project's state in Postgres and keep it current. Ctrl-C stops after
    /// the current commit; running again resumes.
    Run(RunArgs),
    /// Drop the project's state and build it again from its start.
    Replay {
        #[command(flatten)]
        run: RunArgs,
        /// Confirm dropping the schema, its cursor and its change feed.
        #[arg(long)]
        yes: bool,
    },
}

#[derive(Debug, clap::Args)]
struct ApiKey {
    /// Geomi API key.
    #[arg(long = "api-key", env = "APTOS_API_KEY", hide_env_values = true)]
    api_key: Option<String>,
}

#[derive(Debug, clap::Args)]
struct RunArgs {
    /// Postgres to build into.
    #[arg(long, env = "NINEVEH_DATABASE_URL", hide_env_values = true)]
    database_url: String,
    /// Postgres schema for the project's tables. Defaults to the project's name.
    #[arg(long)]
    schema: Option<String>,
    /// Stop once this version is committed.
    #[arg(long)]
    until: Option<u64>,
    /// Streams to backfill with at once, up to the chain's current version.
    #[arg(long, default_value_t = 4)]
    streams: usize,
    /// Versions per backfill range.
    #[arg(long, default_value_t = 1_000_000)]
    chunk: u64,
    #[command(flatten)]
    key: ApiKey,
}

impl RunArgs {
    fn options(&self) -> RunOptions {
        RunOptions {
            database_url: SecretString::from(self.database_url.clone()),
            schema: self.schema.clone(),
            until: self.until,
            streams: self.streams.max(1),
            chunk: self.chunk.max(1),
            api_key: self.key.secret(),
        }
    }
}

impl ApiKey {
    fn secret(&self) -> Option<SecretString> {
        self.api_key.clone().map(SecretString::from)
    }
}

#[tokio::main]
async fn main() -> ExitCode {
    tracing_subscriber::fmt()
        .with_env_filter(
            EnvFilter::try_from_default_env().unwrap_or_else(|_| EnvFilter::new("info,sqlx=warn")),
        )
        .with_writer(std::io::stderr)
        .init();
    let cli = Cli::parse();
    match dispatch(cli).await {
        Ok(()) => ExitCode::SUCCESS,
        Err(error) => {
            #[allow(clippy::print_stderr, reason = "the CLI reports its failure")]
            {
                eprintln!("error: {error:#}");
            }
            ExitCode::FAILURE
        }
    }
}

async fn dispatch(cli: Cli) -> Result<()> {
    let paths = Paths::new(cli.config, cli.lock);
    match cli.command {
        Command::Init { key } => init::init(&paths, key.secret().as_ref()).await,
        Command::Validate => validate(&paths),
        Command::Run(args) => run::run(&paths, &args.options()).await,
        Command::Replay { run, yes } => run::replay(&paths, &run.options(), yes).await,
    }
}

fn validate(paths: &Paths) -> Result<()> {
    let loaded = project::load(paths)?;
    let config = loaded.project.config();
    #[allow(clippy::print_stdout, reason = "the command's output")]
    {
        println!(
            "{} is valid: {} source(s), {} state table(s), starting at version {}",
            paths.config.display(),
            config.sources.len(),
            config.state.len(),
            loaded.start
        );
    }
    Ok(())
}
