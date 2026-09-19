//! The pipeline end to end, against a real Postgres and a scripted stream that drops
//! connections, ends early and fails.
//!
//! The headline property: for random workloads, random response sizes, sparse
//! (filtered-style) or dense streams, and any schedule of retryable failures, the
//! committed state and change feed equal a single in-memory fold.
//!
//! Needs `NINEVEH_TEST_DATABASE_URL`, like the store's tests: they skip without it,
//! except in CI, where they fail.

#![allow(
    clippy::unwrap_used,
    clippy::panic,
    clippy::print_stderr,
    reason = "test-only crate: helpers panic on unexpected results and note skips"
)]

use std::collections::VecDeque;
use std::future::{Future, pending, ready};
use std::sync::atomic::{AtomicU32, AtomicUsize, Ordering};
use std::sync::{Arc, Mutex};
use std::time::Duration;

use nineveh_config::{Project, parse};
use nineveh_core::{ChainId, Version};
use nineveh_decode::{DecodedTransaction, LockBuilder, Lockfile, Record};
use nineveh_engine::{Engine, MemoryState, TableId};
use nineveh_ingest::{Batch, IngestError};
use nineveh_pipeline::replay;
use nineveh_pipeline::{
    BatchStream, Outcome, Parallel, Phase, Pipeline, PipelineConfig, PipelineError, Source,
};
use nineveh_proto::transaction::Transaction;
use nineveh_store::Store;
use nineveh_testkit::vault::{self, Op};
use proptest::prelude::*;
use proptest::test_runner::{Config as RunnerConfig, TestRunner};
use sqlx::PgPool;
use sqlx::postgres::PgPoolOptions;

async fn pool() -> Option<PgPool> {
    let Ok(url) = std::env::var("NINEVEH_TEST_DATABASE_URL") else {
        assert!(
            std::env::var_os("CI").is_none(),
            "CI must set NINEVEH_TEST_DATABASE_URL"
        );
        eprintln!("skipping: set NINEVEH_TEST_DATABASE_URL to run the pipeline's tests");
        return None;
    };
    Some(
        PgPoolOptions::new()
            .max_connections(4)
            .connect(&url)
            .await
            .unwrap(),
    )
}

async fn fresh_schema(pool: &PgPool, name: &str) -> String {
    static NEXT: AtomicU32 = AtomicU32::new(0);
    let schema = format!(
        "p_{name}_{}_{}",
        std::process::id(),
        NEXT.fetch_add(1, Ordering::Relaxed)
    );
    Store::reset(pool, &schema).await.unwrap();
    schema
}

// --- a scripted stream ------------------------------------------------------------

/// What goes wrong on one open of the stream.
#[derive(Debug, Clone)]
enum Fault {
    /// The connection is refused (retryable).
    Refused,
    /// The server fails the stream after this many responses (retryable).
    FailAfter(usize),
    /// The server ends the stream after this many responses.
    EndAfter(usize),
    /// A failure retrying can't fix.
    Fatal,
}

fn fault() -> impl Strategy<Value = Option<Fault>> {
    prop_oneof![
        Just(None),
        Just(Some(Fault::Refused)),
        (0usize..4).prop_map(|n| Some(Fault::FailAfter(n))),
        (0usize..4).prop_map(|n| Some(Fault::EndAfter(n))),
    ]
}

/// Serves `transactions` from any version, in responses covering `sizes` versions each
/// (cycled). With `ranged`, responses carry processed ranges that tile the version
/// space, as a filtered stream's do, so versions can be sparse and responses empty.
struct Scripted {
    transactions: Arc<Vec<Transaction>>,
    sizes: Vec<u64>,
    ranged: bool,
    /// The chain's last version.
    end: Version,
    /// A fault for each open, in order; opens after the script runs out are clean.
    faults: Mutex<VecDeque<Option<Fault>>>,
    opens: AtomicUsize,
    /// After the last response, stay open with nothing to send, as the live stream
    /// does at the chain's tip, rather than end.
    quiet: bool,
}

impl Scripted {
    fn new(transactions: Vec<Transaction>, sizes: Vec<u64>, ranged: bool) -> Self {
        let end = Version::new(transactions.last().map_or(0, |tx| tx.version));
        Self {
            transactions: Arc::new(transactions),
            sizes,
            ranged,
            end,
            faults: Mutex::new(VecDeque::new()),
            opens: AtomicUsize::new(0),
            quiet: false,
        }
    }

    fn quiet(mut self) -> Self {
        self.quiet = true;
        self
    }

    fn with_faults(self, faults: impl IntoIterator<Item = Option<Fault>>) -> Self {
        *self.faults.lock().unwrap() = faults.into_iter().collect();
        self
    }
}

impl Source for Scripted {
    type Stream = ScriptedStream;

    fn open(
        &self,
        from: Version,
        until: Option<Version>,
    ) -> impl Future<Output = Result<ScriptedStream, IngestError>> + Send {
        ready(self.open_now(from, until))
    }
}

impl Scripted {
    fn open_now(
        &self,
        from: Version,
        until: Option<Version>,
    ) -> Result<ScriptedStream, IngestError> {
        self.opens.fetch_add(1, Ordering::Relaxed);
        let fault = self.faults.lock().unwrap().pop_front().flatten();
        match fault {
            Some(Fault::Refused) => return Err(tonic::Status::unavailable("scripted").into()),
            Some(Fault::Fatal) => return Err(IngestError::MissingChainId),
            _ => {}
        }
        let last = until.map_or(self.end, |u| u.min(self.end));
        let mut responses = VecDeque::new();
        let mut first = from.get();
        let mut step = 0;
        while first <= last.get() {
            let size = self.sizes[step % self.sizes.len()].max(1);
            step += 1;
            let end = first.saturating_add(size - 1).min(last.get());
            let transactions: Vec<Transaction> = self
                .transactions
                .iter()
                .filter(|tx| (first..=end).contains(&tx.version))
                .cloned()
                .collect();
            if self.ranged || !transactions.is_empty() {
                responses.push_back(Batch {
                    chain_id: ChainId::try_from(2u64).unwrap(),
                    transactions,
                    processed_range: self.ranged.then(|| Version::new(first)..=Version::new(end)),
                });
            }
            first = end + 1;
        }
        Ok(ScriptedStream {
            responses,
            fault,
            served: 0,
            quiet: self.quiet,
        })
    }
}

struct ScriptedStream {
    responses: VecDeque<Batch>,
    fault: Option<Fault>,
    served: usize,
    quiet: bool,
}

impl BatchStream for ScriptedStream {
    // Lazy, so a `next` dropped before it's polled takes nothing: cancel-safe.
    async fn next(&mut self) -> Result<Option<Batch>, IngestError> {
        if self.quiet && self.responses.is_empty() && self.fault.is_none() {
            pending::<()>().await;
        }
        self.next_now()
    }
}

impl ScriptedStream {
    fn next_now(&mut self) -> Result<Option<Batch>, IngestError> {
        match self.fault {
            Some(Fault::FailAfter(n)) if self.served == n => {
                return Err(tonic::Status::unavailable("scripted").into());
            }
            Some(Fault::EndAfter(n)) if self.served == n => return Ok(None),
            _ => {}
        }
        self.served += 1;
        Ok(self.responses.pop_front())
    }
}

// --- helpers ----------------------------------------------------------------------

fn config(start: u64, until: Option<u64>) -> PipelineConfig {
    let mut config = PipelineConfig::new(Version::new(start));
    config.until = until.map(Version::new);
    config.backoff_initial = Duration::from_millis(1);
    config.backoff_max = Duration::from_millis(5);
    config.max_retries = Some(30);
    config
}

fn build(
    pool: &PgPool,
    schema: &str,
    source: Scripted,
    config: PipelineConfig,
) -> Pipeline<Scripted> {
    let (lock, project) = vault::project();
    Pipeline::new(
        source,
        pool.clone(),
        schema,
        Arc::new(project),
        Arc::new(lock),
        config,
    )
}

/// Spread versions out, as a filtered stream sees them: only every third version
/// matches.
fn sparse(mut transactions: Vec<Transaction>) -> Vec<Transaction> {
    for (i, tx) in transactions.iter_mut().enumerate() {
        tx.version = 1_001 + 3 * u64::try_from(i).unwrap();
    }
    transactions
}

/// The vault's tables: the seven state tables, then the engine's internal ones.
fn table_ids() -> Vec<TableId> {
    (0..7)
        .map(TableId::State)
        .chain([TableId::Handles, TableId::Buckets])
        .collect()
}

const DEPOSIT_LOG: u32 = 4;

/// Check the store holds exactly what one in-memory pass over `transactions` gives,
/// and the outbox holds its change feed.
async fn check_against_memory(pool: &PgPool, schema: &str, transactions: &[Transaction]) {
    let (lock, project) = vault::project();
    let decoded = vault::decode(&lock, &project, transactions);
    let mut memory = MemoryState::new();
    let clean = Engine::new(&project).fold(&memory, &decoded).unwrap();
    memory.apply(&clean);

    let store = Store::open(pool.clone(), schema, &project, &lock)
        .await
        .unwrap();
    for id in table_ids() {
        let mut stored = store.scan(id).await.unwrap();
        stored.sort();
        let expected: Vec<_> = memory
            .rows(id)
            .map(|(k, r)| {
                (
                    k.clone(),
                    (id != TableId::State(DEPOSIT_LOG)).then(|| r.clone()),
                )
            })
            .collect();
        assert_eq!(stored, expected, "{id:?} differs from the in-memory fold");
    }

    let feed: Vec<(i64, i32, String)> = sqlx::query_as(
        "SELECT version, seq, table_name FROM nineveh.changes
         WHERE schema_name = $1 ORDER BY version, seq",
    )
    .bind(schema)
    .fetch_all(pool)
    .await
    .unwrap();
    let names = |t: u32| {
        project.config().state[usize::try_from(t).unwrap()]
            .name
            .name
            .clone()
    };
    let expected: Vec<(i64, String)> = clean
        .changes
        .iter()
        .map(|c| (i64::try_from(c.version.get()).unwrap(), names(c.table)))
        .collect();
    let got: Vec<(i64, String)> = feed.into_iter().map(|(v, _, t)| (v, t)).collect();
    assert_eq!(got, expected, "outbox");
}

async fn balances(pool: &PgPool, schema: &str) -> Vec<(String, String)> {
    sqlx::query_as(&format!(
        r#"SELECT "user", balance::text FROM "{schema}".balances ORDER BY "user""#
    ))
    .fetch_all(pool)
    .await
    .unwrap()
}

// --- the property -----------------------------------------------------------------

#[derive(Debug)]
struct Case {
    ops: Vec<Op>,
    sizes: Vec<u64>,
    ranged: bool,
    faults: Vec<Option<Fault>>,
    max_batch: usize,
    decode_tasks: usize,
    cache_rows: usize,
    /// Parallel backfill: streams, chunk size, and where it stops (as an offset from
    /// the first version).
    parallel: Option<(usize, u64, u64)>,
    buffered: usize,
}

fn case() -> impl Strategy<Value = Case> {
    (
        proptest::collection::vec(vault::op(), 1..50),
        proptest::collection::vec(1u64..10, 1..6),
        any::<bool>(),
        proptest::collection::vec(fault(), 0..6),
        (1usize..16, 1usize..4, 0usize..40),
        proptest::option::of((1usize..4, 1u64..20, 0u64..160)),
        1usize..6,
    )
        .prop_map(
            |(
                ops,
                sizes,
                ranged,
                faults,
                (max_batch, decode_tasks, cache_rows),
                parallel,
                buffered,
            )| {
                Case {
                    ops,
                    sizes,
                    ranged,
                    faults,
                    max_batch,
                    decode_tasks,
                    cache_rows,
                    parallel,
                    buffered,
                }
            },
        )
}

async fn run_case(pool: &PgPool, case: Case) {
    let (transactions, _) = vault::transactions(&case.ops);
    let transactions = if case.ranged {
        sparse(transactions)
    } else {
        transactions
    };
    let Some(end) = transactions.last().map(|tx| tx.version) else {
        return;
    };
    let schema = fresh_schema(pool, "prop").await;
    let source =
        Scripted::new(transactions.clone(), case.sizes, case.ranged).with_faults(case.faults);
    let mut config = config(1_001, Some(end));
    config.max_batch_transactions = case.max_batch;
    config.decode_tasks = case.decode_tasks;
    config.cache_rows = case.cache_rows;
    config.buffered_transactions = case.buffered;
    config.parallel = case.parallel.map(|(streams, chunk, through)| {
        let mut parallel = Parallel::new(streams, Version::new(1_001 + through));
        parallel.chunk_versions = chunk;
        parallel
    });
    let pipeline = build(pool, &schema, source, config);

    let outcome = pipeline.run(pending()).await.unwrap();
    assert_eq!(
        outcome,
        Outcome::Finished {
            cursor: Some(Version::new(end))
        }
    );
    let status = pipeline.status().borrow().clone();
    assert_eq!(status.phase, Phase::Stopped);
    assert_eq!(status.cursor, Some(Version::new(end)));
    assert_eq!(status.versions, end - 1_000, "every version counted once");

    check_against_memory(pool, &schema, &transactions).await;
    Store::reset(pool, &schema).await.unwrap();
}

#[test]
fn any_responses_and_any_retryable_failures_give_the_in_memory_state() {
    let rt = tokio::runtime::Runtime::new().unwrap();
    let Some(pool) = rt.block_on(pool()) else {
        return;
    };
    let mut runner = TestRunner::new(RunnerConfig {
        cases: 40,
        failure_persistence: None,
        ..RunnerConfig::default()
    });
    runner
        .run(&case(), |case| {
            rt.block_on(run_case(&pool, case));
            Ok(())
        })
        .unwrap();
}

// --- resuming ---------------------------------------------------------------------

#[tokio::test]
async fn a_new_pipeline_resumes_from_the_cursor() {
    let Some(pool) = pool().await else { return };
    let (transactions, _) = vault::transactions(&[
        Op::Deposit { user: 0, amount: 5 },
        Op::CreateVault { vault: 0 },
        Op::Deposit { user: 1, amount: 7 },
        Op::SetPosition {
            vault: 0,
            user: 1,
            size: 3,
        },
        Op::Withdraw { user: 0, amount: 2 },
    ]);
    let schema = fresh_schema(&pool, "resume").await;

    let first = build(
        &pool,
        &schema,
        Scripted::new(transactions.clone(), vec![2], false),
        config(1_001, Some(1_003)),
    );
    first.run(pending()).await.unwrap();

    let source = Scripted::new(transactions.clone(), vec![2], false);
    let second = build(&pool, &schema, source, config(1_001, Some(1_005)));
    assert_eq!(
        second.run(pending()).await.unwrap(),
        Outcome::Finished {
            cursor: Some(Version::new(1_005))
        }
    );
    assert_eq!(second.status().borrow().versions, 2, "resumed at 1004");
    check_against_memory(&pool, &schema, &transactions).await;

    // Already done: nothing is streamed.
    let source = Scripted::new(transactions.clone(), vec![2], false);
    let third = build(&pool, &schema, source, config(1_001, Some(1_005)));
    third.run(pending()).await.unwrap();
    assert_eq!(third.status().borrow().commits, 0);
    Store::reset(&pool, &schema).await.unwrap();
}

#[tokio::test]
async fn shutdown_stops_a_live_pipeline_between_commits() {
    let Some(pool) = pool().await else { return };
    let (transactions, _) = vault::transactions(&[
        Op::Deposit { user: 0, amount: 5 },
        Op::Deposit { user: 1, amount: 7 },
    ]);
    let schema = fresh_schema(&pool, "live").await;
    // Live: no `until`. When the scripted chain runs out the stream ends, and the
    // pipeline reconnects and waits until it's told to stop.
    let mut live = config(1_001, None);
    live.max_retries = None;
    let pipeline = build(
        &pool,
        &schema,
        Scripted::new(transactions.clone(), vec![1], false),
        live,
    );
    let mut status = pipeline.status();
    let shutdown = async move {
        status
            .wait_for(|s| s.cursor == Some(Version::new(1_002)))
            .await
            .unwrap();
    };
    assert_eq!(
        pipeline.run(shutdown).await.unwrap(),
        Outcome::Stopped {
            cursor: Some(Version::new(1_002))
        }
    );
    assert_eq!(pipeline.status().borrow().phase, Phase::Stopped);
    check_against_memory(&pool, &schema, &transactions).await;
    Store::reset(&pool, &schema).await.unwrap();
}

/// At the chain's tip the stream goes quiet. What it has delivered must be committed
/// then, not held back until more arrives to fill the decode tasks.
#[tokio::test]
async fn a_quiet_stream_still_commits_what_it_delivered() {
    let Some(pool) = pool().await else { return };
    let (transactions, _) = vault::transactions(&[
        Op::Deposit { user: 0, amount: 5 },
        Op::Deposit { user: 1, amount: 7 },
    ]);
    let schema = fresh_schema(&pool, "quiet").await;
    let pipeline = build(
        &pool,
        &schema,
        Scripted::new(transactions.clone(), vec![1], false).quiet(),
        config(1_001, None),
    );
    let mut status = pipeline.status();
    let shutdown = async move {
        status
            .wait_for(|s| s.cursor == Some(Version::new(1_002)))
            .await
            .unwrap();
    };
    let outcome = tokio::time::timeout(Duration::from_secs(10), pipeline.run(shutdown))
        .await
        .expect("both responses are committed while the stream is quiet");
    assert_eq!(
        outcome.unwrap(),
        Outcome::Stopped {
            cursor: Some(Version::new(1_002))
        }
    );
    check_against_memory(&pool, &schema, &transactions).await;
    Store::reset(&pool, &schema).await.unwrap();
}

// --- halting ----------------------------------------------------------------------

#[tokio::test]
async fn a_failing_rule_halts_at_its_version_with_everything_before_committed() {
    let Some(pool) = pool().await else { return };
    // `balance - u128(amount)` underflows: user 0 withdraws more than they have.
    let transactions = vec![
        vault::transaction(1_001, vec![vault::event("DepositEvent", 0, 5)], vec![]),
        vault::transaction(1_002, vec![vault::event("DepositEvent", 1, 3)], vec![]),
        vault::transaction(1_003, vec![vault::event("WithdrawEvent", 0, 9)], vec![]),
        vault::transaction(1_004, vec![vault::event("DepositEvent", 1, 1)], vec![]),
    ];
    let schema = fresh_schema(&pool, "halt").await;
    // One response, one group: the halt is in the middle of a batch.
    let pipeline = build(
        &pool,
        &schema,
        Scripted::new(transactions.clone(), vec![10], false),
        config(1_001, Some(1_004)),
    );
    let err = pipeline.run(pending()).await.unwrap_err();
    assert!(matches!(err, PipelineError::Halt(_)), "{err}");
    assert!(!err.is_retryable());
    assert_eq!(err.halted_at(), Some(Version::new(1_003)));

    let status = pipeline.status().borrow().clone();
    assert_eq!(status.phase, Phase::Halted);
    assert_eq!(status.cursor, Some(Version::new(1_002)));
    assert!(status.last_error.unwrap().contains("1003"));
    check_against_memory(&pool, &schema, &transactions[..2]).await;

    // Running again halts at the same place, committing nothing more.
    let again = pipeline_after(&pool, &schema, &transactions).await;
    assert_eq!(again.halted_at(), Some(Version::new(1_003)));
    Store::reset(&pool, &schema).await.unwrap();
}

async fn pipeline_after(
    pool: &PgPool,
    schema: &str,
    transactions: &[Transaction],
) -> PipelineError {
    let pipeline = build(
        pool,
        schema,
        Scripted::new(transactions.to_vec(), vec![1], false),
        config(1_001, Some(1_004)),
    );
    let err = pipeline.run(pending()).await.unwrap_err();
    assert_eq!(pipeline.status().borrow().commits, 0);
    err
}

#[tokio::test]
async fn a_record_that_doesnt_decode_halts_at_its_version() {
    let Some(pool) = pool().await else { return };
    let mut bad = vault::event("DepositEvent", 1, 3);
    bad.data = r#"{"user":"0x1","amount":"lots"}"#.into();
    let transactions = vec![
        vault::transaction(1_001, vec![vault::event("DepositEvent", 0, 5)], vec![]),
        vault::transaction(1_002, vec![], vec![]),
        vault::transaction(1_003, vec![bad], vec![]),
        vault::transaction(1_004, vec![vault::event("DepositEvent", 1, 1)], vec![]),
    ];
    let schema = fresh_schema(&pool, "decode").await;
    let pipeline = build(
        &pool,
        &schema,
        Scripted::new(transactions.clone(), vec![2], false),
        config(1_001, Some(1_004)),
    );
    let err = pipeline.run(pending()).await.unwrap_err();
    assert!(matches!(err, PipelineError::Decode(_)), "{err}");
    assert_eq!(err.halted_at(), Some(Version::new(1_003)));
    assert_eq!(
        pipeline.status().borrow().cursor,
        Some(Version::new(1_002)),
        "the transaction before it, which has no records, is covered too"
    );
    assert_eq!(
        balances(&pool, &schema).await,
        [(vault::user(0).to_string(), "5".to_owned())]
    );
    Store::reset(&pool, &schema).await.unwrap();
}

#[tokio::test]
async fn fatal_stream_errors_and_rebuilds_are_not_retried() {
    let Some(pool) = pool().await else { return };
    let (transactions, _) = vault::transactions(&[Op::Deposit { user: 0, amount: 5 }]);
    let schema = fresh_schema(&pool, "fatal").await;

    let source =
        Scripted::new(transactions.clone(), vec![1], false).with_faults([Some(Fault::Fatal)]);
    let pipeline = build(&pool, &schema, source, config(1_001, Some(1_001)));
    let err = pipeline.run(pending()).await.unwrap_err();
    assert!(
        matches!(err, PipelineError::Ingest(IngestError::MissingChainId)),
        "{err}"
    );
    assert_eq!(pipeline.status().borrow().phase, Phase::Halted);

    // A store built from another config needs a rebuild, not a retry.
    let changed = vault::CONFIG.replace("deposits + 1", "deposits + 2");
    let config_changed = parse(&changed).unwrap();
    let mut builder = LockBuilder::new(config_changed.network);
    for module in vault::modules() {
        builder.add_module(module);
    }
    let lock: Lockfile = builder.build(&config_changed.roots()).unwrap();
    let project: Project = config_changed.resolve(&lock).unwrap();
    let other = Pipeline::new(
        Scripted::new(transactions.clone(), vec![1], false),
        pool.clone(),
        &schema,
        Arc::new(project),
        Arc::new(lock),
        config(1_001, Some(1_001)),
    );
    let err = other.run(pending()).await.unwrap_err();
    assert!(
        matches!(
            err,
            PipelineError::Store(nineveh_store::StoreError::Rebuild { .. })
        ),
        "{err}"
    );
    Store::reset(&pool, &schema).await.unwrap();
}

#[tokio::test]
async fn retries_give_up_after_the_limit_and_reset_on_progress() {
    let Some(pool) = pool().await else { return };
    let (transactions, _) = vault::transactions(&[
        Op::Deposit { user: 0, amount: 5 },
        Op::Deposit { user: 1, amount: 7 },
        Op::Deposit { user: 2, amount: 9 },
    ]);
    let schema = fresh_schema(&pool, "retries").await;

    // Three failures in a row, but each run commits one response first: progress
    // resets the count, so a limit of one is never exceeded.
    let source = Scripted::new(transactions.clone(), vec![1], false).with_faults([
        Some(Fault::FailAfter(1)),
        Some(Fault::FailAfter(1)),
        Some(Fault::FailAfter(1)),
    ]);
    let mut limited = config(1_001, Some(1_003));
    limited.max_retries = Some(1);
    let pipeline = build(&pool, &schema, source, limited.clone());
    pipeline.run(pending()).await.unwrap();
    assert_eq!(pipeline.status().borrow().cursor, Some(Version::new(1_003)));
    Store::reset(&pool, &schema).await.unwrap();

    // Two refusals with no progress between them exceed it.
    let schema = fresh_schema(&pool, "retries").await;
    let source = Scripted::new(transactions.clone(), vec![1], false)
        .with_faults([Some(Fault::Refused), Some(Fault::Refused)]);
    let pipeline = build(&pool, &schema, source, limited);
    let err = pipeline.run(pending()).await.unwrap_err();
    assert!(err.is_retryable(), "{err}");
    assert_eq!(pipeline.status().borrow().phase, Phase::Halted);
    assert_eq!(pipeline.status().borrow().cursor, None);
    Store::reset(&pool, &schema).await.unwrap();
}

/// The CLI runs pipelines on spawned tasks, so `run` must be `Send` for any source
/// that is. This only has to compile.
#[allow(dead_code, reason = "a compile-time check")]
fn the_pipeline_can_run_on_a_spawned_task(pipeline: Pipeline<Scripted>) {
    drop(tokio::spawn(async move { pipeline.run(pending()).await }));
}

/// The record log holds what the decoder produced, and folding from the log gives the
/// same state as folding from the stream (ADR 0022).
///
/// This is the property a rebuild depends on: it replays these records instead of
/// re-reading the chain, so if the log and the stream disagree, a rebuild quietly
/// produces different rows than the run that built the tables.
#[tokio::test]
async fn the_record_log_replays_to_the_same_state() {
    let Some(pool) = pool().await else { return };
    let (transactions, _) = vault::transactions(&[
        Op::Deposit { user: 0, amount: 5 },
        Op::CreateVault { vault: 0 },
        Op::Deposit { user: 1, amount: 7 },
        Op::SetPosition {
            vault: 0,
            user: 1,
            size: 3,
        },
        Op::Withdraw { user: 0, amount: 2 },
    ]);
    let schema = fresh_schema(&pool, "recordlog").await;
    // The record log is keyed by project, not by schema (ADR 0022), and every other
    // test in this file runs the project called "vault". Give this one its own.
    let name = format!("vault_log_{}", std::process::id());
    let (lock, project) = vault::project_named(&name);
    nineveh_store::records::forget(&pool, &name).await.unwrap();

    let last = transactions.last().unwrap().version;
    Pipeline::new(
        Scripted::new(transactions.clone(), vec![2], false),
        pool.clone(),
        &schema,
        Arc::new(vault::project_named(&name).1),
        Arc::new(lock.clone()),
        config(1_001, Some(last)),
    )
    .run(pending())
    .await
    .unwrap();

    // What the decoder makes of the same transactions, in one pass.
    let decoded = vault::decode(&lock, &project, &transactions);
    let expected: Vec<_> = decoded
        .iter()
        .flat_map(|tx| tx.records.iter().map(move |r| (tx.version, r.clone())))
        .collect();

    let logged =
        nineveh_store::records::read(&pool, &name, Version::new(0), Version::new(u64::MAX / 2))
            .await
            .unwrap();
    assert_eq!(
        logged.len(),
        expected.len(),
        "the log holds every record the decoder produced"
    );

    for (got, (version, want)) in logged.iter().zip(&expected) {
        assert_eq!(got.version, *version, "records keep their version");
        assert_eq!(
            got.record.source,
            project.source_name(want.source).unwrap().as_str(),
            "records name their source"
        );
        let back = Record::from_stored(got.record.clone(), want.source).unwrap();
        assert_eq!(&back, want, "a record changed on its way through the log");
    }

    // The point of all of it: replaying the log folds to the same state the stream did.
    let from_log: Vec<DecodedTransaction> = {
        let mut grouped: Vec<DecodedTransaction> = Vec::new();
        for entry in &logged {
            let record = Record::from_stored(
                entry.record.clone(),
                project.source_by_name(&entry.record.source).unwrap(),
            )
            .unwrap();
            match grouped.last_mut() {
                Some(tx) if tx.version == entry.version => tx.records.push(record),
                _ => grouped.push(DecodedTransaction {
                    version: entry.version,
                    timestamp_micros: entry.timestamp_micros,
                    success: entry.success,
                    sender: None,
                    records: vec![record],
                }),
            }
        }
        grouped
    };

    let engine = Engine::new(&project);
    let mut from_stream_state = MemoryState::new();
    let from_stream = engine.fold(&from_stream_state, &decoded).unwrap();
    from_stream_state.apply(&from_stream);

    let mut from_log_state = MemoryState::new();
    let replayed = engine.fold(&from_log_state, &from_log).unwrap();
    from_log_state.apply(&replayed);

    for id in table_ids() {
        let mut a: Vec<_> = from_stream_state
            .rows(id)
            .map(|(k, r)| (k.clone(), r.clone()))
            .collect();
        let mut b: Vec<_> = from_log_state
            .rows(id)
            .map(|(k, r)| (k.clone(), r.clone()))
            .collect();
        a.sort();
        b.sort();
        assert_eq!(a, b, "replaying the log gave different rows for {id:?}");
    }

    nineveh_store::records::forget(&pool, &name).await.unwrap();
}

/// A rebuild from the record log lands on the same rows as a rebuild from the chain,
/// and the log refuses rather than guesses when it can't cover the range (ADR 0022).
///
/// This is the whole point of keeping records: editing a rule triggers a rebuild, and
/// a rebuild that has to re-read the chain costs days of a scarce stream for a contract
/// a few months old. Replaying is only worth doing if it lands in exactly the same
/// place, so that is what's asserted.
#[tokio::test]
async fn a_rebuild_from_the_log_matches_a_rebuild_from_the_chain() {
    let Some(pool) = pool().await else { return };
    let (transactions, _) = vault::transactions(&[
        Op::Deposit { user: 0, amount: 5 },
        Op::CreateVault { vault: 0 },
        Op::Deposit { user: 1, amount: 7 },
        Op::SetPosition {
            vault: 0,
            user: 1,
            size: 3,
        },
        Op::Withdraw { user: 0, amount: 2 },
    ]);
    let name = format!("vault_rebuild_{}", std::process::id());
    let (lock, project) = vault::project_named(&name);
    nineveh_store::records::forget(&pool, &name).await.unwrap();

    let last = Version::new(transactions.last().unwrap().version);
    let from_chain = fresh_schema(&pool, "fromchain").await;
    Pipeline::new(
        Scripted::new(transactions.clone(), vec![2], false),
        pool.clone(),
        &from_chain,
        Arc::new(vault::project_named(&name).1),
        Arc::new(lock.clone()),
        config(1_001, Some(last.get())),
    )
    .run(pending())
    .await
    .unwrap();

    let lock_hash = nineveh_store::lock_hash(&lock).unwrap();
    let start = Version::new(1_001);
    replay::available(
        &pool,
        &name,
        &lock_hash,
        &project.source_names(),
        start,
        last,
    )
    .await
    .unwrap()
    .expect("the log covers what the run just wrote");

    // Rebuild into a second schema, reading only the log.
    let from_log = fresh_schema(&pool, "fromlog").await;
    let mut store = Store::open(pool.clone(), &from_log, &project, &lock)
        .await
        .unwrap();
    replay::rebuild(&pool, &mut store, &project, &name, start, last, 10_000)
        .await
        .unwrap();

    let chain_store = Store::open(pool.clone(), &from_chain, &project, &lock)
        .await
        .unwrap();
    assert_eq!(
        store.cursor(),
        chain_store.cursor(),
        "both builds end at the same version"
    );
    for id in table_ids() {
        let (mut a, mut b) = (
            chain_store.scan(id).await.unwrap(),
            store.scan(id).await.unwrap(),
        );
        a.sort();
        b.sort();
        assert_eq!(a, b, "the log rebuilt {id:?} differently from the chain");
    }

    nineveh_store::records::forget(&pool, &name).await.unwrap();
}

/// The log declines what it can't serve, rather than building something partial.
///
/// Each of these sends the rebuild back to the stream, which is always correct and
/// only slower — the failure mode to avoid is replaying a log that doesn't cover the
/// range and quietly producing different rows.
#[tokio::test]
async fn the_log_refuses_what_it_cannot_cover() {
    let Some(pool) = pool().await else { return };
    let (transactions, _) = vault::transactions(&[Op::Deposit { user: 0, amount: 5 }]);
    let name = format!("vault_refuse_{}", std::process::id());
    let (lock, project) = vault::project_named(&name);
    nineveh_store::records::forget(&pool, &name).await.unwrap();

    let last = Version::new(transactions.last().unwrap().version);
    let schema = fresh_schema(&pool, "refuse").await;
    Pipeline::new(
        Scripted::new(transactions.clone(), vec![2], false),
        pool.clone(),
        &schema,
        Arc::new(vault::project_named(&name).1),
        Arc::new(lock.clone()),
        config(1_001, Some(last.get())),
    )
    .run(pending())
    .await
    .unwrap();

    let lock_hash = nineveh_store::lock_hash(&lock).unwrap();
    let start = Version::new(1_001);
    let sources = project.source_names();

    assert_eq!(
        replay::available(
            &pool,
            &name,
            "another-lock",
            &project.source_names(),
            start,
            last
        )
        .await
        .unwrap(),
        Err(replay::Unavailable::LockChanged),
        "records decoded against other layouts are not replayed"
    );
    assert!(
        matches!(
            replay::available(
                &pool,
                &name,
                &lock_hash,
                &sources,
                start,
                Version::new(9_999_999),
            )
            .await
            .unwrap(),
            Err(replay::Unavailable::EndsEarly { .. })
        ),
        "a target past the log is refused"
    );
    assert!(
        matches!(
            replay::available(&pool, &name, &lock_hash, &sources, Version::new(1), last,)
                .await
                .unwrap(),
            Err(replay::Unavailable::StartsLate { .. })
        ),
        "a start before the log is refused"
    );
    assert_eq!(
        replay::available(&pool, "never-logged", &lock_hash, &sources, start, last,)
            .await
            .unwrap(),
        Err(replay::Unavailable::Empty),
        "a project with no log is refused"
    );

    // A source the log has never seen has no history to replay: back to the stream.
    let mut added = sources.clone();
    added.push("a_source_added_later".to_owned());
    assert_eq!(
        replay::available(&pool, &name, &lock_hash, &added, start, last)
            .await
            .unwrap(),
        Err(replay::Unavailable::SourceAdded {
            name: "a_source_added_later".to_owned()
        }),
        "adding a source backfills; it doesn't replay"
    );

    nineveh_store::records::forget(&pool, &name).await.unwrap();
}
