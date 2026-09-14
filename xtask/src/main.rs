//! Repository automation, run as `cargo xtask <command>`.
//!
//! - `codegen`: regenerate the Transaction Stream bindings from the vendored protos.
//! - `codegen --check`: fail if the checked-in bindings are stale (run in CI).
//!
//! Bindings are checked in rather than generated in a build script. That way
//! downstream builds need no protobuf compiler, and every proto bump shows up as a
//! reviewable diff.
//!
//! Codegen runs in two passes. The message types go to `nineveh-proto`, which needs
//! only `prost`, so the decoder can read transactions without an async runtime. The
//! gRPC client goes to `nineveh-ingest` and refers to those messages by extern path.

#![allow(clippy::print_stdout, clippy::print_stderr)]

use std::fs;
use std::path::{Path, PathBuf};
use std::process::ExitCode;

use anyhow::{Context, Result, bail};

/// Root proto file(s); imports are pulled in transitively.
const PROTO_ENTRYPOINTS: &[&str] = &["aptos/indexer/v1/raw_data.proto"];

fn main() -> ExitCode {
    match run() {
        Ok(()) => ExitCode::SUCCESS,
        Err(err) => {
            eprintln!("error: {err:#}");
            ExitCode::FAILURE
        }
    }
}

fn run() -> Result<()> {
    let args: Vec<String> = std::env::args().skip(1).collect();
    match args
        .iter()
        .map(String::as_str)
        .collect::<Vec<_>>()
        .as_slice()
    {
        ["codegen"] => codegen(Mode::Write),
        ["codegen", "--check"] => codegen(Mode::Check),
        _ => bail!("usage: cargo xtask codegen [--check]"),
    }
}

#[derive(Clone, Copy, PartialEq, Eq)]
enum Mode {
    Write,
    Check,
}

/// Where one codegen pass writes its bindings, relative to the workspace root.
#[derive(Clone, Copy)]
enum Pass {
    /// Message types only, into `nineveh-proto`.
    Messages,
    /// The gRPC client only, into `nineveh-ingest`.
    Client,
}

impl Pass {
    const ALL: [Self; 2] = [Self::Messages, Self::Client];

    fn checked_in(self) -> &'static str {
        match self {
            Self::Messages => "crates/nineveh-proto/src/generated",
            Self::Client => "crates/nineveh-ingest/src/proto/generated",
        }
    }

    fn check_dir(self) -> &'static str {
        match self {
            Self::Messages => "target/xtask/codegen-check/messages",
            Self::Client => "target/xtask/codegen-check/client",
        }
    }
}

fn codegen(mode: Mode) -> Result<()> {
    let root = workspace_root();
    let proto_dir = root.join("crates/nineveh-proto/proto");
    let entrypoints: Vec<PathBuf> = PROTO_ENTRYPOINTS
        .iter()
        .map(|p| proto_dir.join(p))
        .collect();

    let mut stale = Vec::new();
    for pass in Pass::ALL {
        let checked_in = root.join(pass.checked_in());
        let out_dir = match mode {
            Mode::Write => checked_in.clone(),
            Mode::Check => root.join(pass.check_dir()),
        };
        reset_dir(&out_dir)?;

        let fds = protox::compile(&entrypoints, [&proto_dir])
            .with_context(|| format!("compiling protos under {}", proto_dir.display()))?;
        let builder = tonic_prost_build::configure()
            .build_server(false)
            .build_transport(false)
            .emit_rerun_if_changed(false)
            .out_dir(&out_dir);
        let builder = match pass {
            Pass::Messages => builder.build_client(false),
            Pass::Client => builder
                .build_client(true)
                .extern_path(".aptos", "::nineveh_proto::aptos"),
        };
        builder
            .compile_fds(fds)
            .context("generating Rust bindings")?;
        // Packages with nothing left to generate come out empty; don't check them in.
        remove_empty(&out_dir)?;

        match mode {
            Mode::Write => println!("wrote bindings to {}", checked_in.display()),
            Mode::Check => stale.extend(compare_dirs(&out_dir, &checked_in)?),
        }
    }

    if stale.is_empty() {
        if mode == Mode::Check {
            println!("bindings are up to date");
        }
        Ok(())
    } else {
        bail!(
            "generated bindings are stale ({}); run `cargo xtask codegen` and commit the result",
            stale.join(", ")
        )
    }
}

fn workspace_root() -> PathBuf {
    let manifest_dir = Path::new(env!("CARGO_MANIFEST_DIR"));
    manifest_dir.parent().unwrap_or(manifest_dir).to_path_buf()
}

/// Remove generated `.rs` files, leaving anything hand-written alone.
fn reset_dir(dir: &Path) -> Result<()> {
    fs::create_dir_all(dir).with_context(|| format!("creating {}", dir.display()))?;
    for entry in rs_files(dir)? {
        fs::remove_file(&entry).with_context(|| format!("removing {}", entry.display()))?;
    }
    Ok(())
}

fn rs_files(dir: &Path) -> Result<Vec<PathBuf>> {
    let mut files = Vec::new();
    for entry in fs::read_dir(dir).with_context(|| format!("reading {}", dir.display()))? {
        let path = entry?.path();
        if path.extension().is_some_and(|ext| ext == "rs") {
            files.push(path);
        }
    }
    files.sort();
    Ok(files)
}

fn remove_empty(dir: &Path) -> Result<()> {
    for path in rs_files(dir)? {
        let contents =
            fs::read_to_string(&path).with_context(|| format!("reading {}", path.display()))?;
        if contents.trim().is_empty() {
            fs::remove_file(&path).with_context(|| format!("removing {}", path.display()))?;
        }
    }
    Ok(())
}

/// The paths, relative to the workspace, of files that differ between the two dirs.
fn compare_dirs(fresh: &Path, checked_in: &Path) -> Result<Vec<String>> {
    let names = |dir: &Path| -> Result<Vec<String>> {
        Ok(rs_files(dir)?
            .iter()
            .filter_map(|p| p.file_name().map(|n| n.to_string_lossy().into_owned()))
            .collect())
    };
    let (fresh_names, checked_names) = (names(fresh)?, names(checked_in)?);

    let mut stale = Vec::new();
    for name in fresh_names.iter().chain(&checked_names) {
        let a = fs::read(fresh.join(name)).ok();
        let b = fs::read(checked_in.join(name)).ok();
        let path = checked_in.join(name).display().to_string();
        if a != b && !stale.contains(&path) {
            stale.push(path);
        }
    }
    Ok(stale)
}
