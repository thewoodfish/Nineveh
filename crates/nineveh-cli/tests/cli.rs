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
    assert!(stdout.contains("5 source(s), 8 state table(s)"), "{stdout}");
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

/// `nineveh up` serves the control API, with CORS for Studio. Needs a Postgres
/// (`NINEVEH_TEST_DATABASE_URL`); skips without one, except in CI.
#[test]
fn up_serves_the_control_api() {
    let Ok(database) = std::env::var("NINEVEH_TEST_DATABASE_URL") else {
        assert!(
            std::env::var_os("CI").is_none(),
            "CI must set NINEVEH_TEST_DATABASE_URL"
        );
        return;
    };
    let port = {
        let listener = std::net::TcpListener::bind("127.0.0.1:0").unwrap();
        listener.local_addr().unwrap().port()
    };
    let mut server = Command::new(env!("CARGO_BIN_EXE_nineveh"))
        .args(["up", "--listen", &format!("127.0.0.1:{port}")])
        .current_dir(project_dir("up"))
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
            "GET {path} HTTP/1.1\r\nHost: localhost\r\nOrigin: http://localhost:3000\r\n\
             Connection: close\r\n\r\n"
        )
        .ok()?;
        let mut response = String::new();
        let _ = stream.read_to_string(&mut response);
        Some(response)
    };
    let mut projects = None;
    for _ in 0..100 {
        projects = get("/control/v1/projects");
        if projects.is_some() {
            break;
        }
        std::thread::sleep(std::time::Duration::from_millis(100));
    }
    let unknown = get("/projects/no_such_project/v1/status");
    let _ = server.kill();
    let _ = server.wait();

    let projects = projects.expect("the control plane came up");
    assert!(projects.starts_with("HTTP/1.1 200"), "{projects}");
    assert!(projects.contains("\r\n\r\n["), "a JSON list: {projects}");
    assert!(
        projects.contains("access-control-allow-origin"),
        "CORS for Studio: {projects}"
    );
    assert!(unknown.unwrap().starts_with("HTTP/1.1 404"));
}

/// Without sign-in, `nineveh up` refuses to listen beyond loopback (ADR 0018).
#[test]
fn up_without_sign_in_stays_on_loopback() {
    let output = Command::new(env!("CARGO_BIN_EXE_nineveh"))
        .args(["up", "--listen", "0.0.0.0:4999"])
        .current_dir(project_dir("up_public"))
        .env("NINEVEH_DATABASE_URL", "postgres://unused/nineveh")
        .env_remove("NINEVEH_GITHUB_CLIENT_ID")
        .env_remove("NINEVEH_GITHUB_CLIENT_SECRET")
        .env("NO_COLOR", "1")
        .output()
        .unwrap();
    assert!(!output.status.success());
    let stderr = text(&output.stderr);
    assert!(stderr.contains("only listens on loopback"), "{stderr}");

    let half = Command::new(env!("CARGO_BIN_EXE_nineveh"))
        .args(["up", "--github-client-id", "Iv1.x"])
        .current_dir(project_dir("up_half"))
        .env("NINEVEH_DATABASE_URL", "postgres://unused/nineveh")
        .env_remove("NINEVEH_GITHUB_CLIENT_SECRET")
        .env("NO_COLOR", "1")
        .output()
        .unwrap();
    assert!(!half.status.success());
    assert!(
        text(&half.stderr).contains("needs both"),
        "{}",
        text(&half.stderr)
    );
}

/// A project whose reduce tables and handlers live in a `.nineveh.ts` file (ADR 0025).
const DSL_YAML: &str = "\
name: vault
network: testnet
reducers: ./vault.nineveh.ts
sources:
  deposits:    { event: 0xcafe::vault::DepositEvent }
  withdrawals: { event: 0xcafe::vault::WithdrawEvent }
state:
  deposit_log: { log: deposits }
";

const DSL: &str = r"
export const balances = table({
  key:     { user: address },
  columns: { balance: u128.default(0), deposits: u64.default(0) },
})

on(deposits, (d) => {
  const b = balances.row(d.user)
  b.balance  += u128(d.amount)
  b.deposits += 1
})

on(withdrawals, (w) => {
  if (w.amount == 0) return
  balances.row(w.user).balance -= u128(w.amount)
})
";

/// Write a DSL project: the YAML, the handlers, and a lock for the vault's modules.
fn dsl_project(dir: &Path, dsl: &str) {
    vault_project(dir, DSL_YAML, Some(1_000));
    fs::write(dir.join("vault.nineveh.ts"), dsl).unwrap();
}

#[test]
fn validate_accepts_a_project_written_in_the_dsl() {
    let dir = project_dir("dsl-valid");
    dsl_project(&dir, DSL);
    let out = nineveh(&dir, &["validate"]);
    assert!(out.status.success(), "{}", text(&out.stderr));
    let stdout = text(&out.stdout);
    // The log from the YAML, and `balances` from the DSL file.
    assert!(stdout.contains("2 source(s), 2 state table(s)"), "{stdout}");
}

/// A problem in the handlers is reported against the handlers, at its line — not
/// against `nineveh.yaml`, which is where the spans would land without file identity.
#[test]
fn a_problem_in_the_handlers_points_at_the_handlers() {
    let dir = project_dir("dsl-bad-column");
    dsl_project(&dir, &DSL.replace("b.deposits += 1", "b.depsits += 1"));
    let out = nineveh(&dir, &["validate"]);
    assert!(!out.status.success());
    let stderr = text(&out.stderr);
    assert!(
        stderr.contains("`balances` has no column `depsits`"),
        "{stderr}"
    );
    assert!(stderr.contains("--> vault.nineveh.ts:10:5"), "{stderr}");
    assert!(stderr.contains("did you mean `deposits`?"), "{stderr}");
}

/// A type error surfaces at resolution, after the lock is read, and still lands in the
/// DSL file rather than in the YAML.
#[test]
fn a_type_error_in_the_handlers_points_at_the_handlers() {
    let dir = project_dir("dsl-type-error");
    // `amount` is a u64; the column is u128, and nothing converts implicitly.
    dsl_project(
        &dir,
        &DSL.replace("b.balance  += u128(d.amount)", "b.balance += d.amount"),
    );
    let out = nineveh(&dir, &["validate"]);
    assert!(!out.status.success());
    let stderr = text(&out.stderr);
    assert!(stderr.contains("vault.nineveh.ts"), "{stderr}");
    assert!(!stderr.contains("--> nineveh.yaml"), "{stderr}");
}

/// A table declared in both files is a mistake worth naming.
#[test]
fn a_table_cannot_be_declared_twice() {
    let dir = project_dir("dsl-clash");
    dsl_project(
        &dir,
        r"
export const deposit_log = table({
  key:     { user: address },
  columns: { n: u64.default(0) },
})

on(deposits, (d) => {
  deposit_log.row(d.user).n += 1
})
",
    );
    let out = nineveh(&dir, &["validate"]);
    assert!(!out.status.success());
    let stderr = text(&out.stderr);
    assert!(
        stderr.contains("`deposit_log` is already a table in nineveh.yaml"),
        "{stderr}"
    );
}

/// A missing `reducers:` file says which config asked for it.
#[test]
fn a_missing_reducers_file_says_who_wanted_it() {
    let dir = project_dir("dsl-missing");
    vault_project(&dir, DSL_YAML, Some(1_000));
    let out = nineveh(&dir, &["validate"]);
    assert!(!out.status.success());
    let stderr = text(&out.stderr);
    assert!(
        stderr.contains("which nineveh.yaml names as its `reducers`"),
        "{stderr}"
    );
}
