//! The `nineveh` binary: `validate` offline, and `init`, `run` and a resume against
//! testnet and Postgres when a key and a database are given.

#![allow(
    clippy::unwrap_used,
    clippy::panic,
    reason = "test-only crate: helpers panic on unexpected results"
)]

use std::fs;
use std::path::{Path, PathBuf};
use std::process::{Command, Output};
use std::sync::atomic::{AtomicU32, Ordering};

use nineveh_core::Version;
use nineveh_decode::LockBuilder;
use nineveh_testkit::vault;

/// A fresh project directory under the target's scratch space.
fn project_dir(name: &str) -> PathBuf {
    static NEXT: AtomicU32 = AtomicU32::new(0);
    let dir = Path::new(env!("CARGO_TARGET_TMPDIR")).join(format!(
        "cli-{name}-{}-{}",
        std::process::id(),
        NEXT.fetch_add(1, Ordering::Relaxed)
    ));
    let _ = fs::remove_dir_all(&dir);
    fs::create_dir_all(&dir).unwrap();
    dir
}

fn nineveh(dir: &Path, args: &[&str]) -> Output {
    Command::new(env!("CARGO_BIN_EXE_nineveh"))
        .args(args)
        .current_dir(dir)
        .env("NO_COLOR", "1")
        .env_remove("APTOS_API_KEY")
        .output()
        .unwrap()
}

fn text(bytes: &[u8]) -> String {
    String::from_utf8_lossy(bytes).into_owned()
}

/// The vault project with its lock, pinned to start at `start`.
fn vault_project(dir: &Path, yaml: &str, start: Option<u64>) {
    fs::write(dir.join("nineveh.yaml"), yaml).unwrap();
    let config = nineveh_config::parse(yaml).unwrap();
    let mut builder = LockBuilder::new(config.network);
    for module in vault::modules() {
        builder.add_module(module);
    }
    let lock = builder
        .build(&config.roots())
        .unwrap()
        .with_start_version(start.map(Version::new));
    fs::write(dir.join("nineveh.lock"), lock.to_json().unwrap()).unwrap();
}

#[test]
fn validate_accepts_a_project_and_says_where_it_starts() {
    let dir = project_dir("valid");
    vault_project(&dir, vault::CONFIG, Some(1_000));
    let out = nineveh(&dir, &["validate"]);
    assert!(out.status.success(), "{}", text(&out.stderr));
    let stdout = text(&out.stdout);
    assert!(stdout.contains("5 source(s), 7 state table(s)"), "{stdout}");
    assert!(stdout.contains("starting at version 1000"), "{stdout}");
}

#[test]
fn validate_reports_problems_at_their_lines() {
    let dir = project_dir("invalid");
    vault_project(&dir, vault::CONFIG, Some(1_000));
    let broken = vault::CONFIG.replace("deposits + 1", "deposits + nope");
    fs::write(dir.join("nineveh.yaml"), &broken).unwrap();
    let out = nineveh(&dir, &["validate"]);
    assert!(!out.status.success());
    let stderr = text(&out.stderr);
    assert!(stderr.contains("nope"), "{stderr}");
    assert!(stderr.contains("nineveh.yaml:"), "located: {stderr}");
}

#[test]
fn auto_needs_a_pinned_start() {
    let dir = project_dir("unpinned");
    vault_project(&dir, vault::CONFIG, None);
    let out = nineveh(&dir, &["validate"]);
    assert!(!out.status.success());
    let stderr = text(&out.stderr);
    assert!(stderr.contains("nineveh init"), "{stderr}");
}

#[test]
fn a_missing_lock_points_at_init() {
    let dir = project_dir("nolock");
    fs::write(dir.join("nineveh.yaml"), vault::CONFIG).unwrap();
    let out = nineveh(&dir, &["validate"]);
    assert!(!out.status.success());
    assert!(text(&out.stderr).contains("nineveh init"));
}

#[test]
fn replay_needs_confirmation() {
    let dir = project_dir("replay");
    vault_project(&dir, vault::CONFIG, Some(1_000));
    let out = Command::new(env!("CARGO_BIN_EXE_nineveh"))
        .args(["replay", "--database-url", "postgres:///unused"])
        .current_dir(&dir)
        .output()
        .unwrap();
    assert!(!out.status.success());
    assert!(text(&out.stderr).contains("--yes"), "{}", text(&out.stderr));
}

/// `init`, `run`, a resume and a rebuild after a rule change, against testnet and
/// Postgres. Run with
/// `APTOS_API_KEY` and `NINEVEH_TEST_DATABASE_URL` set:
/// `cargo test -p nineveh-cli --test cli -- --ignored`.
#[test]
#[ignore = "needs APTOS_API_KEY, NINEVEH_TEST_DATABASE_URL and the network"]
fn init_and_run_against_testnet() {
    let key = std::env::var("APTOS_API_KEY").expect("APTOS_API_KEY");
    let database = std::env::var("NINEVEH_TEST_DATABASE_URL").expect("NINEVEH_TEST_DATABASE_URL");
    let dir = project_dir("testnet");
    let market = "0x0e3117b978e079073756f6e1aafff9e4fcb028e51612c3a80c20a095fdfd4a02";
    let yaml = |start: &str, rule: &str| {
        format!(
            "\
name: market
network: testnet
start_version: {start}
sources:
  created: {{ event: {market}::user::CreateContractEvent }}
  closed:  {{ event: {market}::user::CloseContractEvent }}
state:
  created_log: {{ log: created }}
  open_contracts:
    key: [user_address]
    columns:
      user_address: address
      opened: {{ type: u64, default: 0 }}
    reduce:
      - {{ on: created, set: {{ opened: \"{rule}\" }} }}
"
        )
    };
    let run = |args: &[&str]| {
        Command::new(env!("CARGO_BIN_EXE_nineveh"))
            .args(args)
            .current_dir(&dir)
            .env("NO_COLOR", "1")
            .env("APTOS_API_KEY", &key)
            .env("NINEVEH_DATABASE_URL", &database)
            .output()
            .unwrap()
    };
    let schema = format!("cli_market_{}", std::process::id());

    // `auto` resolves through the Indexer API and is pinned.
    fs::write(dir.join("nineveh.yaml"), yaml("auto", "opened + 1")).unwrap();
    let out = run(&["init"]);
    assert!(out.status.success(), "{}", text(&out.stderr));
    let lock = fs::read_to_string(dir.join("nineveh.lock")).unwrap();
    assert!(lock.contains("\"start_version\": \"5774816547\""), "{lock}");

    // An explicit start, just before a known CreateContractEvent at 6000029471.
    fs::write(dir.join("nineveh.yaml"), yaml("6000029000", "opened + 1")).unwrap();
    assert!(run(&["init"]).status.success());
    let out = run(&[
        "replay",
        "--yes",
        "--schema",
        &schema,
        "--until",
        "6000029500",
    ]);
    assert!(out.status.success(), "{}", text(&out.stderr));
    assert!(
        text(&out.stderr).contains("cursor=Some(6000029500)"),
        "{}",
        text(&out.stderr)
    );

    // Resuming picks up after the cursor.
    let out = run(&["run", "--schema", &schema, "--until", "6000030000"]);
    assert!(out.status.success(), "{}", text(&out.stderr));
    let stderr = text(&out.stderr);
    assert!(stderr.contains("from=6000029501"), "{stderr}");
    assert!(stderr.contains("cursor=Some(6000030000)"), "{stderr}");

    // A changed rule rebuilds beside the served build, then swaps it in.
    let opened = || -> i64 {
        let rt = tokio::runtime::Runtime::new().unwrap();
        rt.block_on(async {
            let pool = sqlx::PgPool::connect(&database).await.unwrap();
            sqlx::query_scalar::<_, i64>(&format!(
                r#"SELECT coalesce(sum(opened), 0)::bigint FROM "{schema}".open_contracts"#
            ))
            .fetch_one(&pool)
            .await
            .unwrap()
        })
    };
    let before = opened();
    assert!(before > 0);
    fs::write(dir.join("nineveh.yaml"), yaml("6000029000", "opened + 2")).unwrap();
    let out = run(&["run", "--schema", &schema, "--until", "6000030000"]);
    assert!(out.status.success(), "{}", text(&out.stderr));
    let stderr = text(&out.stderr);
    assert!(stderr.contains("rebuilding"), "{stderr}");
    assert!(stderr.contains("swapped in the rebuild"), "{stderr}");
    assert_eq!(opened(), 2 * before, "the new rule's state is served");

    // The swapped-in build resumes like any other.
    let out = run(&["run", "--schema", &schema, "--until", "6000030000"]);
    assert!(out.status.success(), "{}", text(&out.stderr));
    assert!(
        !text(&out.stderr).contains("rebuilding"),
        "{}",
        text(&out.stderr)
    );
}

/// `nineveh serve` answers the state API and the change feed. Needs a Postgres
/// (`NINEVEH_TEST_DATABASE_URL`); skips without one, except in CI.
#[test]
fn serve_answers_the_api() {
    let Ok(database) = std::env::var("NINEVEH_TEST_DATABASE_URL") else {
        assert!(
            std::env::var_os("CI").is_none(),
            "CI must set NINEVEH_TEST_DATABASE_URL"
        );
        return;
    };
    let dir = project_dir("serve");
    vault_project(&dir, vault::CONFIG, Some(1_000));
    let port = {
        let listener = std::net::TcpListener::bind("127.0.0.1:0").unwrap();
        listener.local_addr().unwrap().port()
    };
    let mut server = Command::new(env!("CARGO_BIN_EXE_nineveh"))
        .args(["serve", "--listen", &format!("127.0.0.1:{port}")])
        .args(["--schema", &format!("cli_serve_{}", std::process::id())])
        .current_dir(&dir)
        .env("NINEVEH_DATABASE_URL", &database)
        .env("NO_COLOR", "1")
        .spawn()
        .unwrap();

    let get = |path: &str| -> Option<String> {
        use std::io::{Read, Write};
        let mut stream = std::net::TcpStream::connect(("127.0.0.1", port)).ok()?;
        stream
            .set_read_timeout(Some(std::time::Duration::from_secs(2)))
            .ok()?;
        write!(
            stream,
            "GET {path} HTTP/1.1\r\nHost: localhost\r\nConnection: close\r\n\r\n"
        )
        .ok()?;
        let mut response = String::new();
        let _ = stream.read_to_string(&mut response);
        Some(response)
    };
    let mut status = None;
    for _ in 0..100 {
        status = get("/v1/status");
        if status.is_some() {
            break;
        }
        std::thread::sleep(std::time::Duration::from_millis(100));
    }
    let tables = get("/v1/tables");
    let feed = get("/v1/changes?after=soon");
    let _ = server.kill();
    let _ = server.wait();

    let status = status.expect("the server came up");
    assert!(status.starts_with("HTTP/1.1 200"), "{status}");
    assert!(status.contains("\"project\":\"vault\""), "{status}");
    assert!(
        status.contains("access-control-allow-origin"),
        "CORS for Studio: {status}"
    );
    let tables = tables.unwrap();
    assert!(tables.contains("\"name\":\"balances\""), "{tables}");
    assert!(
        feed.unwrap().starts_with("HTTP/1.1 400"),
        "the feed is mounted"
    );
}
