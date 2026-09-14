//! Repository automation, run as `cargo xtask <command>`.
//!
//! - `codegen`: regenerate the Transaction Stream bindings from the vendored protos.
//! - `codegen --check`: fail if the checked-in bindings are stale (run in CI).
//!
//! Bindings are checked in rather than generated in a build script. That way
//! downstream builds need no protobuf compiler, and every proto bump shows up as a
//! reviewable diff.

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

fn codegen(mode: Mode) -> Result<()> {
    let root = workspace_root();
    let proto_dir = root.join("crates/nineveh-ingest/proto");
    let checked_in = root.join("crates/nineveh-ingest/src/proto/generated");

    let out_dir = match mode {
        Mode::Write => checked_in.clone(),
        Mode::Check => root.join("target/xtask/codegen-check"),
    };
    reset_dir(&out_dir)?;

    let entrypoints: Vec<PathBuf> = PROTO_ENTRYPOINTS
        .iter()
        .map(|p| proto_dir.join(p))
        .collect();
    let fds = protox::compile(&entrypoints, [&proto_dir])
        .with_context(|| format!("compiling protos under {}", proto_dir.display()))?;

    tonic_prost_build::configure()
        .build_client(true)
        .build_server(false)
        .build_transport(false)
        .emit_rerun_if_changed(false)
        .out_dir(&out_dir)
        .compile_fds(fds)
        .context("generating Rust bindings")?;

    match mode {
        Mode::Write => {
            println!("wrote bindings to {}", checked_in.display());
            Ok(())
        }
        Mode::Check => compare_dirs(&out_dir, &checked_in),
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

fn compare_dirs(fresh: &Path, checked_in: &Path) -> Result<()> {
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
        if a != b && !stale.contains(name) {
            stale.push(name.clone());
        }
    }

    if stale.is_empty() {
        println!("bindings are up to date");
        Ok(())
    } else {
        bail!(
            "generated bindings are stale ({}); run `cargo xtask codegen` and commit the result",
            stale.join(", ")
        )
    }
}
