//! Sending a project's state changes to its webhook endpoints (ADR 0020).
//!
//! One task per endpoint, tailing the change outbox from the endpoint's own position
//! (`nineveh-store`'s [`webhooks`]) and posting batches. Because each endpoint keeps
//! its own position, a receiver that's down holds up only its own deliveries — never
//! the pipeline, and never another endpoint.
//!
//! Delivery is **at least once**: a batch is marked delivered only after a 2xx, so a
//! response lost on the way back is sent again. Every delivery carries the position of
//! its last change as an idempotency key, and receivers that keep rows should ignore a
//! change they've already applied.
//!
//! Each POST is signed with the endpoint's secret: `X-Nineveh-Signature: t=<unix>,
//! v1=<hex>`, the HMAC-SHA256 of `<t>.<body>`. The timestamp is in the signed text, so
//! a receiver can refuse an old one.

use std::net::{IpAddr, SocketAddr};
use std::time::{Duration, Instant, SystemTime, UNIX_EPOCH};

use hmac::{Hmac, Mac};
use nineveh_config::{Change, Webhook};
use nineveh_store::webhooks;
use serde_json::{Value, json};
use sha2::Sha256;
use sqlx::PgPool;
use tokio::sync::{broadcast, watch};
use tokio::task::JoinHandle;
use tracing::{info, warn};

/// Changes in one delivery at most.
const BATCH: i64 = 100;
/// How long one POST may take.
const TIMEOUT: Duration = Duration::from_secs(10);
/// How often to look for changes without a wake-up.
const POLL: Duration = Duration::from_secs(2);
/// How long to wait after the first failure, and at most after any.
const FIRST_WAIT: Duration = Duration::from_secs(1);
const LONGEST_WAIT: Duration = Duration::from_secs(300);
/// How long a checked address is used before it's resolved and checked again.
const RESOLVED_FOR: Duration = Duration::from_secs(300);

/// The webhook senders of one project, running until they're stopped.
#[derive(Debug)]
pub struct Deliveries {
    stop: watch::Sender<bool>,
    tasks: Vec<JoinHandle<()>>,
}

impl Deliveries {
    /// Start a sender for each endpoint of `schema`, and forget any endpoint the
    /// config no longer names.
    #[must_use]
    pub fn start(
        pool: &PgPool,
        schema: &str,
        hooks: &[Webhook],
        wake: Option<&broadcast::Receiver<()>>,
    ) -> Self {
        let (stop, stopped) = watch::channel(false);
        let mut tasks = Vec::new();
        let keep: Vec<String> = hooks.iter().map(|h| h.name.name.clone()).collect();
        let forget = {
            let (pool, schema) = (pool.clone(), schema.to_owned());
            tokio::spawn(async move {
                if let Err(error) = webhooks::forget_others(&pool, &schema, &keep).await {
                    warn!(%error, schema, "couldn't forget webhooks the config dropped");
                }
            })
        };
        tasks.push(forget);
        for hook in hooks {
            // Each sender needs wake-ups of its own; they all see every commit.
            let wake = wake.map(broadcast::Receiver::resubscribe);
            tasks.push(tokio::spawn(send(
                pool.clone(),
                schema.to_owned(),
                hook.clone(),
                wake,
                stopped.clone(),
            )));
        }
        Self { stop, tasks }
    }

    /// Stop every sender after its current delivery, and wait for them.
    pub async fn stop(self) {
        let _ = self.stop.send(true);
        for task in self.tasks {
            if let Err(error) = task.await {
                warn!(%error, "a webhook sender panicked");
            }
        }
    }
}

/// One endpoint's sender: from where it left off, for as long as it's wanted.
async fn send(
    pool: PgPool,
    schema: String,
    hook: Webhook,
    mut wake: Option<broadcast::Receiver<()>>,
    mut stopped: watch::Receiver<bool>,
) {
    let name = hook.name.name.clone();
    let Some(endpoint) = start(&pool, &schema, &name, &mut stopped).await else {
        return;
    };
    let mut at = endpoint.cursor.unwrap_or((0, -1));
    let mut target = Target::new(&hook.url);
    let mut wait = FIRST_WAIT;
    info!(
        schema,
        endpoint = name,
        url = hook.url,
        "delivering changes"
    );

    loop {
        if *stopped.borrow() {
            return;
        }
        let read = match unsent(&pool, &schema, &hook, at).await {
            Ok(read) => read,
            Err(error) => {
                warn!(%error, schema, endpoint = name, "couldn't read the change feed");
                pause(wait, &mut stopped).await;
                wait = longer(wait);
                continue;
            }
        };
        let Some(read) = read else {
            // Nothing waiting: sleep until a commit, or look again shortly.
            idle(&mut wake, &mut stopped).await;
            // A rebuild's swap moves every endpoint onto the new build's feed
            // (ADR 0016), so take the stored place if it isn't where we left it.
            if let Ok(Some(stored)) = webhooks::get(&pool, &schema, &name).await
                && let Some(moved) = stored.cursor
                && moved != at
            {
                info!(
                    schema,
                    endpoint = name,
                    "following the feed to its new build"
                );
                at = moved;
            }
            continue;
        };
        if read.wanted.is_empty() {
            // Only changes this endpoint didn't ask for: move past them.
            let (version, seq) = read.last;
            if let Err(error) = webhooks::advance(&pool, &schema, &name, version, seq).await {
                warn!(%error, schema, endpoint = name, "couldn't record the endpoint's place");
            }
            at = read.last;
            continue;
        }
        let body = payload(&schema, &name, &read.wanted, hook.rows).to_string();
        match target.post(&body, &endpoint.secret, read.last).await {
            Ok(()) => {
                let (version, seq) = read.last;
                if let Err(error) = webhooks::delivered(&pool, &schema, &name, version, seq).await {
                    // The delivery happened; losing the record only repeats it.
                    warn!(%error, schema, endpoint = name, "couldn't record a delivery");
                }
                at = read.last;
                wait = FIRST_WAIT;
            }
            Err(reason) => {
                warn!(
                    schema,
                    endpoint = name,
                    reason,
                    "a delivery failed; retrying"
                );
                let failures = webhooks::failed(&pool, &schema, &name, &reason)
                    .await
                    .unwrap_or_default();
                if failures == 1 || failures % 20 == 0 {
                    warn!(
                        schema,
                        endpoint = name,
                        failures,
                        "this endpoint is failing"
                    );
                }
                pause(wait, &mut stopped).await;
                wait = longer(wait);
            }
        }
    }
}

/// The endpoint's secret and position, starting it at the newest change if it's new.
/// Keeps trying while the database is unreachable; gives up when stopped.
async fn start(
    pool: &PgPool,
    schema: &str,
    name: &str,
    stopped: &mut watch::Receiver<bool>,
) -> Option<webhooks::Endpoint> {
    let mut wait = FIRST_WAIT;
    loop {
        if *stopped.borrow() {
            return None;
        }
        match webhooks::ensure(pool, schema, name).await {
            Ok(endpoint) if endpoint.cursor.is_some() => return Some(endpoint),
            // A new endpoint starts at the end of the feed: configuring one shouldn't
            // deliver the project's whole history. An empty feed has no history to
            // skip, so it keeps no position and starts from the beginning.
            Ok(endpoint) => match newest(pool, schema).await {
                None => return Some(endpoint),
                Some((version, seq)) => {
                    if let Err(error) = webhooks::start_at(pool, schema, name, version, seq).await {
                        warn!(%error, schema, endpoint = name, "couldn't place a new endpoint");
                    } else {
                        return Some(webhooks::Endpoint {
                            cursor: Some((version, seq)),
                            ..endpoint
                        });
                    }
                }
            },
            Err(error) => warn!(%error, schema, endpoint = name, "couldn't read the endpoint"),
        }
        pause(wait, stopped).await;
        wait = longer(wait);
    }
}

/// One change, as a delivery carries it.
struct Delivery {
    version: i64,
    seq: i32,
    table: String,
    op: String,
    key: Value,
    row: Option<Value>,
}

/// What one read of the outbox found: the changes this endpoint asked for, and the
/// position to move to whether or not it wanted them.
struct Read {
    wanted: Vec<Delivery>,
    last: (i64, i32),
}

/// The next changes after `at` from the tables this endpoint follows.
async fn unsent(
    pool: &PgPool,
    schema: &str,
    hook: &Webhook,
    at: (i64, i32),
) -> Result<Option<Read>, sqlx::Error> {
    let tables: Vec<String> = hook
        .on
        .iter()
        .map(|s| s.table.name.clone())
        .collect::<std::collections::BTreeSet<_>>()
        .into_iter()
        .collect();
    let rows: Vec<(i64, i32, String, String, Value, Option<Value>)> = sqlx::query_as(
        "SELECT version, seq, table_name, op, key, new_row FROM nineveh.changes
         WHERE schema_name = $1 AND (version, seq) > ($2, $3) AND table_name = ANY($4)
         ORDER BY version, seq LIMIT $5",
    )
    .bind(schema)
    .bind(at.0)
    .bind(at.1)
    .bind(&tables)
    .bind(BATCH)
    .fetch_all(pool)
    .await?;
    let Some(&(version, seq, ..)) = rows.last() else {
        return Ok(None);
    };
    let wanted = rows
        .into_iter()
        .filter(|(_, _, table, op, _, _)| asked_for(hook, table, op))
        .map(|(version, seq, table, op, key, row)| Delivery {
            version,
            seq,
            table,
            op,
            key,
            row,
        })
        .collect();
    Ok(Some(Read {
        wanted,
        last: (version, seq),
    }))
}

/// Whether this endpoint asked for this change.
fn asked_for(hook: &Webhook, table: &str, op: &str) -> bool {
    hook.on.iter().any(|s| {
        s.table.name == table
            && match s.change {
                Change::Changed => true,
                Change::Inserted => op == "insert",
                Change::Updated => op == "update",
                Change::Deleted => op == "delete",
            }
    })
}

/// The body of one delivery.
fn payload(schema: &str, endpoint: &str, changes: &[Delivery], rows: bool) -> Value {
    json!({
        "project": schema,
        "endpoint": endpoint,
        "changes": changes
            .iter()
            .map(|change| {
                let mut one = json!({
                    "table": change.table,
                    "op": change.op,
                    "version": change.version.to_string(),
                    "seq": change.seq,
                    "key": change.key,
                });
                if rows && let Some(map) = one.as_object_mut() {
                    map.insert("row".into(), change.row.clone().unwrap_or(Value::Null));
                }
                one
            })
            .collect::<Vec<_>>(),
    })
}

/// The newest change in a schema's feed, or the position before its first.
async fn newest(pool: &PgPool, schema: &str) -> Option<(i64, i32)> {
    sqlx::query_as(
        "SELECT version, seq FROM nineveh.changes WHERE schema_name = $1
         ORDER BY version DESC, seq DESC LIMIT 1",
    )
    .bind(schema)
    .fetch_optional(pool)
    .await
    .ok()
    .flatten()
}

/// Where deliveries go, with the address checked before anything is sent there.
struct Target {
    url: String,
    /// Whether the URL asks for the local machine, which only a `localhost` URL does.
    local: bool,
    client: Option<(reqwest::Client, Instant)>,
}

impl Target {
    fn new(url: &str) -> Self {
        let host = reqwest::Url::parse(url)
            .ok()
            .and_then(|u| u.host_str().map(str::to_owned))
            .unwrap_or_default();
        Self {
            url: url.to_owned(),
            local: matches!(host.as_str(), "localhost" | "127.0.0.1" | "::1"),
            client: None,
        }
    }

    /// Post `body`, signed with `secret`.
    async fn post(&mut self, body: &str, secret: &str, last: (i64, i32)) -> Result<(), String> {
        let client = self.client().await?.clone();
        let url = self.url.clone();
        let timestamp = SystemTime::now()
            .duration_since(UNIX_EPOCH)
            .map_or(0, |since| since.as_secs());
        let response = client
            .post(url)
            .header("content-type", "application/json")
            .header("x-nineveh-delivery", format!("{}.{}", last.0, last.1))
            .header(
                "x-nineveh-signature",
                format!("t={timestamp},v1={}", sign(secret, timestamp, body)),
            )
            .body(body.to_owned())
            .send()
            .await
            .map_err(|e| e.to_string())?;
        let status = response.status();
        if status.is_success() {
            return Ok(());
        }
        let said = response.text().await.unwrap_or_default();
        let said: String = said.chars().take(200).collect();
        Err(format!("{status}: {said}"))
    }

    /// A client pinned to an address that has been checked, resolved again from time
    /// to time in case the endpoint moves.
    async fn client(&mut self) -> Result<&reqwest::Client, String> {
        let fresh = self
            .client
            .as_ref()
            .is_some_and(|(_, at)| at.elapsed() < RESOLVED_FOR);
        if !fresh {
            let url = reqwest::Url::parse(&self.url).map_err(|e| e.to_string())?;
            let host = url.host_str().ok_or("the URL has no host")?.to_owned();
            let port = url.port_or_known_default().unwrap_or(443);
            let addresses = checked_addresses(&host, port, self.local).await?;
            let client = reqwest::Client::builder()
                .use_preconfigured_tls(tls())
                .user_agent(concat!("nineveh/", env!("CARGO_PKG_VERSION")))
                .timeout(TIMEOUT)
                // A redirect could point somewhere the address check never saw.
                .redirect(reqwest::redirect::Policy::none())
                // Every address was checked, and reqwest tries them in turn, so a host
                // answering on both IPv4 and IPv6 still connects.
                .resolve_to_addrs(&host, &addresses)
                .build()
                .map_err(|e| e.to_string())?;
            self.client = Some((client, Instant::now()));
        }
        self.client
            .as_ref()
            .map(|(client, _)| client)
            .ok_or_else(|| "no client".to_owned())
    }
}

/// rustls with ring and the webpki roots, as the rest of Nineveh's clients use.
fn tls() -> rustls::ClientConfig {
    let roots = rustls::RootCertStore {
        roots: webpki_roots::TLS_SERVER_ROOTS.to_vec(),
    };
    rustls::ClientConfig::builder_with_provider(std::sync::Arc::new(
        rustls::crypto::ring::default_provider(),
    ))
    .with_safe_default_protocol_versions()
    .unwrap_or_else(|_| unreachable!("ring supports the default protocol versions"))
    .with_root_certificates(roots)
    .with_no_client_auth()
}

/// Resolve `host` and refuse anything but a public address, so a webhook URL can't be
/// used to reach whatever else is on Nineveh's network. A URL that plainly says
/// `localhost` is allowed to mean it, for developing against your own machine.
async fn checked_addresses(host: &str, port: u16, local: bool) -> Result<Vec<SocketAddr>, String> {
    let addresses: Vec<SocketAddr> = tokio::net::lookup_host((host, port))
        .await
        .map_err(|e| format!("can't resolve {host}: {e}"))?
        .collect();
    if addresses.is_empty() {
        return Err(format!("{host} resolves to nothing"));
    }
    // One bad address is enough to refuse: a host that answers with both a public and
    // a private address must not be reachable through the private one.
    if !local && let Some(bad) = addresses.iter().find(|a| !is_public(a.ip())) {
        return Err(format!(
            "{host} resolves to {}, which isn't a public address",
            bad.ip()
        ));
    }
    Ok(addresses)
}

/// Whether an address is one the open internet could reach: not this machine, not the
/// private network, and not the link-local range cloud metadata services live in.
fn is_public(ip: IpAddr) -> bool {
    match ip {
        IpAddr::V4(v4) => {
            !(v4.is_private()
                || v4.is_loopback()
                || v4.is_link_local()
                || v4.is_broadcast()
                || v4.is_documentation()
                || v4.is_unspecified()
                || v4.is_multicast()
                // 100.64.0.0/10, carrier-grade NAT, and 0.0.0.0/8.
                || (v4.octets()[0] == 100 && (64..128).contains(&v4.octets()[1]))
                || v4.octets()[0] == 0)
        }
        IpAddr::V6(v6) => {
            if let Some(v4) = v6.to_ipv4_mapped() {
                return is_public(IpAddr::V4(v4));
            }
            !(v6.is_loopback()
                || v6.is_unspecified()
                || v6.is_multicast()
                // fc00::/7 unique-local and fe80::/10 link-local.
                || (v6.segments()[0] & 0xfe00) == 0xfc00
                || (v6.segments()[0] & 0xffc0) == 0xfe80)
        }
    }
}

/// `HMAC-SHA256(secret, "<timestamp>.<body>")`, as hex.
fn sign(secret: &str, timestamp: u64, body: &str) -> String {
    let mut mac = <Hmac<Sha256> as Mac>::new_from_slice(secret.as_bytes()).unwrap_or_else(|_| {
        <Hmac<Sha256> as Mac>::new_from_slice(&[]).unwrap_or_else(|_| unreachable!())
    });
    mac.update(format!("{timestamp}.").as_bytes());
    mac.update(body.as_bytes());
    let mut hex = String::with_capacity(64);
    for byte in mac.finalize().into_bytes() {
        use std::fmt::Write as _;
        // Writing to a String can't fail.
        let _ = write!(hex, "{byte:02x}");
    }
    hex
}

/// Wait for a commit, a couple of seconds, or the word to stop.
async fn idle(wake: &mut Option<broadcast::Receiver<()>>, stopped: &mut watch::Receiver<bool>) {
    match wake {
        Some(wake) => {
            tokio::select! {
                _ = wake.recv() => {}
                () = tokio::time::sleep(POLL) => {}
                _ = stopped.changed() => {}
            }
        }
        None => {
            tokio::select! {
                () = tokio::time::sleep(POLL) => {}
                _ = stopped.changed() => {}
            }
        }
    }
}

/// Wait, unless we're asked to stop first.
async fn pause(how_long: Duration, stopped: &mut watch::Receiver<bool>) {
    tokio::select! {
        () = tokio::time::sleep(how_long) => {}
        _ = stopped.changed() => {}
    }
}

fn longer(wait: Duration) -> Duration {
    wait.saturating_mul(2).min(LONGEST_WAIT)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn only_public_addresses_are_delivered_to() {
        let ip = |s: &str| s.parse::<IpAddr>().unwrap();
        for private in [
            "127.0.0.1",
            "10.0.0.1",
            "192.168.1.1",
            "172.16.0.1",
            // What a cloud instance's credentials live behind.
            "169.254.169.254",
            "0.0.0.0",
            "100.64.0.1",
            "::1",
            "fd00::1",
            "fe80::1",
            "::ffff:127.0.0.1",
        ] {
            assert!(!is_public(ip(private)), "{private} isn't public");
        }
        for public in ["1.1.1.1", "93.184.216.34", "2606:4700::1111"] {
            assert!(is_public(ip(public)), "{public} is public");
        }
    }

    #[test]
    fn signatures_cover_the_timestamp_and_the_body() {
        let one = sign("whsec_abc", 1_700_000_000, r#"{"changes":[]}"#);
        assert_eq!(one.len(), 64);
        // A different time or body is a different signature, so neither can be swapped.
        assert_ne!(one, sign("whsec_abc", 1_700_000_001, r#"{"changes":[]}"#));
        assert_ne!(one, sign("whsec_abc", 1_700_000_000, r#"{"changes":[1]}"#));
        assert_ne!(one, sign("whsec_xyz", 1_700_000_000, r#"{"changes":[]}"#));
        // A known answer, so a receiver's implementation can be checked against ours:
        // HMAC-SHA256("secret", "0.body").
        assert_eq!(
            sign("secret", 0, "body"),
            "5a5fb86992f15ef304996b793c8b328ede0dd35d46ee020e7bdc1ebc8d9ac6a0"
        );
    }
}
