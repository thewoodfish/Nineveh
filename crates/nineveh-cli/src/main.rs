//! `nineveh`: pin a project's layouts, validate it, and run or replay its pipeline.
//!
//! ```text
//! nineveh init                 # fetch ABIs, write nineveh.lock, resolve start_version: auto
//! nineveh validate             # check nineveh.yaml against the lock, offline
//! nineveh run [--serve]        # build the state in Postgres and keep it current
//! nineveh serve                # serve the state API and change feed
//! nineveh replay --yes         # drop the state and build it again
//! nineveh up                   # the control plane: create and run projects from Studio
//! ```
//!
//! `APTOS_API_KEY` is a Geomi key for the Transaction Stream and the REST and Indexer
//! APIs. `NINEVEH_DATABASE_URL` is the Postgres to build into. (Not `DATABASE_URL`: in a
//! source checkout that switches sqlx's macros to checking against a live database.)

use std::net::SocketAddr;
use std::path::PathBuf;
use std::process::ExitCode;

use anyhow::Result;
use clap::{Parser, Subcommand};
use secrecy::SecretString;
use tracing_subscriber::EnvFilter;

mod init;
mod project;
mod run;
mod serve;
mod shutdown;
mod up;

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
    /// Serve the project's state API and change feed, without running the pipeline.
    Serve {
        /// Postgres to read from.
        #[arg(long, env = "NINEVEH_DATABASE_URL", hide_env_values = true)]
        database_url: String,
        /// Postgres schema for the project's tables. Defaults to the project's name.
        #[arg(long)]
        schema: Option<String>,
        /// Address to listen on.
        #[arg(long, default_value = serve::DEFAULT_LISTEN)]
        listen: SocketAddr,
    },
    /// Build the project again from its start, beside the served build, and swap it in.
    Replay {
        #[command(flatten)]
        run: RunArgs,
        /// Confirm dropping the schema, its cursor and its change feed.
        #[arg(long)]
        yes: bool,
    },
    /// Run the control plane Studio drives: create projects from a contract address,
    /// and run and serve every project in the database, until Ctrl-C.
    Up {
        /// Postgres for the projects' registry and state.
        #[arg(long, env = "NINEVEH_DATABASE_URL", hide_env_values = true)]
        database_url: String,
        /// Address to listen on.
        #[arg(long, default_value = serve::DEFAULT_LISTEN)]
        listen: SocketAddr,
        /// Streams each project backfills with at once.
        #[arg(long, default_value_t = 4)]
        streams: usize,
        /// Versions per backfill range.
        #[arg(long, default_value_t = 1_000_000)]
        chunk: u64,
        #[command(flatten)]
        key: ApiKey,
        /// A Geomi key for testnet, over --api-key. Keys are per network.
        #[arg(long, env = "APTOS_API_KEY_TESTNET", hide_env_values = true)]
        api_key_testnet: Option<String>,
        /// A Geomi key for mainnet, over --api-key.
        #[arg(long, env = "APTOS_API_KEY_MAINNET", hide_env_values = true)]
        api_key_mainnet: Option<String>,
        /// A Geomi key for devnet, over --api-key.
        #[arg(long, env = "APTOS_API_KEY_DEVNET", hide_env_values = true)]
        api_key_devnet: Option<String>,
        /// A GitHub OAuth app's client id: with its secret, people sign in and projects
        /// need API keys. Without it, no sign-in, on loopback only.
        #[arg(long, env = "NINEVEH_GITHUB_CLIENT_ID")]
        github_client_id: Option<String>,
        /// The GitHub OAuth app's client secret.
        #[arg(long, env = "NINEVEH_GITHUB_CLIENT_SECRET", hide_env_values = true)]
        github_client_secret: Option<String>,
        /// Where browsers reach this control plane, for GitHub's callback. Defaults to
        /// `http://` and the listen address.
        #[arg(long, env = "NINEVEH_PUBLIC_URL")]
        public_url: Option<String>,
        /// Where Studio is: signing in ends there.
        #[arg(
            long,
            env = "NINEVEH_STUDIO_URL",
            default_value = "http://localhost:3000"
        )]
        studio_url: String,
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
    /// Also serve the state API and change feed, on this address or 127.0.0.1:4000.
    #[arg(long, num_args = 0..=1, default_missing_value = serve::DEFAULT_LISTEN)]
    serve: Option<SocketAddr>,
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
            serve: self.serve,
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
        Command::Serve {
            database_url,
            schema,
            listen,
        } => serve::serve(&paths, &database_url, schema, listen).await,
        Command::Replay { run, yes } => run::replay(&paths, &run.options(), yes).await,
        Command::Up {
            database_url,
            listen,
            streams,
            chunk,
            key,
            api_key_testnet,
            api_key_mainnet,
            api_key_devnet,
            github_client_id,
            github_client_secret,
            public_url,
            studio_url,
        } => {
            let github = match (github_client_id, github_client_secret) {
                (Some(id), Some(secret)) => Some((id, SecretString::from(secret))),
                (None, None) => None,
                _ => anyhow::bail!(
                    "GitHub sign-in needs both --github-client-id and \
                     NINEVEH_GITHUB_CLIENT_SECRET"
                ),
            };
            let network_keys = [
                (nineveh_core::Network::Testnet, api_key_testnet),
                (nineveh_core::Network::Mainnet, api_key_mainnet),
                (nineveh_core::Network::Devnet, api_key_devnet),
            ]
            .into_iter()
            .filter_map(|(network, key)| Some((network, SecretString::from(key?))))
            .collect();
            up::up(up::UpOptions {
                database_url: SecretString::from(database_url),
                listen,
                api_key: key.secret(),
                network_keys,
                streams: streams.max(1),
                chunk: chunk.max(1),
                github,
                public_url,
                studio_url,
            })
            .await
        }
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
