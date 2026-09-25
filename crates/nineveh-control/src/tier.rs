//! What a project is allowed, and what happens when it wants more.
//!
//! One tier exists. Billing doesn't, so there is nothing to sell yet and nothing to
//! meter *for* — these numbers are here because the costs behind them are real, not
//! because someone is being charged. Every one of them corresponds to something
//! Nineveh pays for: stored bytes in Postgres, and the Geomi stream time a mainnet
//! project consumes (`docs/research/spike-a-stream.md`).
//!
//! Limits a user can hit are quoted in [`Limits::describe`], because "1 GB" in a
//! message beside the number is the difference between a wall and a reason.

use std::time::Duration;

use nineveh_core::Network;

/// What one project may use.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct Limits {
    /// What to call it in the UI.
    pub name: &'static str,
    /// Projects one account may have.
    pub projects: usize,
    /// Networks a project may follow. Mainnet is the expensive one: a deep mainnet
    /// backfill holds a stream for days and is the only cost here that scales with
    /// customers rather than with storage.
    pub networks: &'static [&'static str],
    /// How far before the chain's tip a new project may start.
    ///
    /// Not a storage limit — a backfill's cost is stream time, which the log never
    /// gives back. Starting at the tip is free; starting a year back is days of
    /// streaming before the project shows anything at all.
    pub look_back: Duration,
    /// Bytes of record log a project may keep before its oldest records are pruned
    /// (ADR 0024). The log is what makes a rebuild local instead of another backfill,
    /// so what this really caps is how far back a rebuild can reach.
    pub log_bytes: i64,
    /// Days of outbox kept, so a webhook receiver down over a weekend still catches
    /// up. Never pruned past an endpoint that is behind it.
    pub history_days: i32,
}

/// The tier every project is on.
///
/// Permanent, not a trial: paid plans are meant to add mainnet and production scale on
/// top of this rather than switch it off. Anything that would make these numbers shrink
/// for an existing account is a promise broken, not a pricing change.
///
/// The numbers: two projects is enough to have a real one and a scratch one. Testnet
/// and devnet cost stream time nobody else is competing for. A gigabyte is about two
/// million records at the 503 bytes each measured on the vault workload — months of a
/// normal app contract. Seven days of outbox is longer than any receiver should be
/// down and shorter than anyone would pay to store.
pub const FREE: Limits = Limits {
    name: "Free",
    projects: 2,
    networks: &["testnet", "devnet"],
    look_back: Duration::from_secs(6 * 60 * 60),
    log_bytes: 1024 * 1024 * 1024,
    history_days: 7,
};

/// Versions a network commits in a second, measured from the REST API over both a
/// one-hour and a half-day window (`docs/research/spike-a-stream.md`, 2026-09-19).
///
/// Testnet moves faster than mainnet despite carrying less real traffic: the block
/// cadence is the same and empty versions still count.
#[must_use]
pub const fn versions_per_second(network: Network) -> u64 {
    match network {
        Network::Mainnet => 148,
        Network::Testnet | Network::Devnet => 220,
    }
}

impl Limits {
    /// How far back of `network` this tier's look-back reaches, in versions.
    #[must_use]
    pub const fn look_back_versions(&self, network: Network) -> u64 {
        self.look_back.as_secs() * versions_per_second(network)
    }

    /// Whether a project on this tier may follow `network`.
    #[must_use]
    pub fn allows(&self, network: &str) -> bool {
        self.networks.contains(&network)
    }

    /// What to tell someone who has hit `limit`, in the terms they'd have set it in.
    ///
    /// Phrased as a fact about the tier rather than an error about them, and without
    /// an upsell: there is nothing to upgrade to yet.
    #[must_use]
    pub fn describe(&self, limit: Limit) -> String {
        match limit {
            Limit::Projects => format!(
                "The {} tier runs {} projects. Delete one to make room for another.",
                self.name, self.projects
            ),
            Limit::Network => format!(
                "The {} tier follows {}. Mainnet is coming soon.",
                self.name,
                english(self.networks)
            ),
            Limit::LookBack => format!(
                "The {} tier starts a project within {} of the chain's tip. Starting \
                 further back is coming soon.",
                self.name,
                hours(self.look_back)
            ),
            Limit::LogBytes => format!(
                "The {} tier keeps {} of history per project. Older records are \
                 dropped as new ones arrive; state tables are unaffected.",
                self.name,
                bytes(self.log_bytes)
            ),
        }
    }
}

/// A limit somebody ran into.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Limit {
    Projects,
    Network,
    LookBack,
    LogBytes,
}

/// `a`, `a and b`, `a, b and c`.
fn english(items: &[&str]) -> String {
    match items {
        [] => String::new(),
        [only] => (*only).to_owned(),
        [rest @ .., last] => format!("{} and {last}", rest.join(", ")),
    }
}

fn hours(d: Duration) -> String {
    match d.as_secs() / 3600 {
        1 => "an hour".to_owned(),
        h => format!("{h} hours"),
    }
}

/// Bytes as a person would write them: `1 GB`, `512 MB`.
fn bytes(n: i64) -> String {
    const GB: i64 = 1024 * 1024 * 1024;
    const MB: i64 = 1024 * 1024;
    if n >= GB && n % GB == 0 {
        format!("{} GB", n / GB)
    } else if n >= MB {
        format!("{} MB", n / MB)
    } else {
        format!("{n} bytes")
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn mainnet_is_not_on_the_free_tier() {
        assert!(FREE.allows("testnet"));
        assert!(FREE.allows("devnet"));
        assert!(!FREE.allows("mainnet"), "mainnet is the one that costs");
    }

    /// The messages are read by people, so they have to say the number.
    #[test]
    fn a_limit_explains_itself_in_its_own_units() {
        assert_eq!(
            FREE.describe(Limit::LogBytes),
            "The Free tier keeps 1 GB of history per project. Older records are \
             dropped as new ones arrive; state tables are unaffected."
        );
        assert_eq!(
            FREE.describe(Limit::Network),
            "The Free tier follows testnet and devnet. Mainnet is coming soon."
        );
        assert_eq!(
            FREE.describe(Limit::LookBack),
            "The Free tier starts a project within 6 hours of the chain's tip. \
             Starting further back is coming soon."
        );
    }

    /// The look-back is quoted in hours but enforced in versions, and the two
    /// networks move at different speeds, so the conversion has to be per network.
    #[test]
    fn six_hours_is_a_different_distance_on_each_network() {
        let testnet = FREE.look_back_versions(Network::Testnet);
        let mainnet = FREE.look_back_versions(Network::Mainnet);
        assert_eq!(testnet, 6 * 3600 * 220, "4.75M versions");
        assert_eq!(mainnet, 6 * 3600 * 148, "3.2M versions");
        assert!(
            testnet > mainnet,
            "testnet's counter moves faster, so six hours of it is further back"
        );
    }

    #[test]
    fn lists_read_as_sentences() {
        assert_eq!(english(&["a"]), "a");
        assert_eq!(english(&["a", "b"]), "a and b");
        assert_eq!(english(&["a", "b", "c"]), "a, b and c");
    }
}
