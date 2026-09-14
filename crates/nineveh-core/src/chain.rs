use std::fmt;
use std::str::FromStr;

/// A transaction version: the position of a transaction in the Aptos ledger.
///
/// Versions are dense and strictly increasing, starting at 0 for genesis. Nineveh uses
/// the version as its processing cursor: every stateful step records the last version
/// it committed and resumes from the one after it.
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Hash)]
pub struct Version(u64);

impl Version {
    /// The genesis transaction.
    pub const GENESIS: Self = Self(0);

    #[must_use]
    pub const fn new(version: u64) -> Self {
        Self(version)
    }

    #[must_use]
    pub const fn get(self) -> u64 {
        self.0
    }

    /// The version after this one, or `None` at `u64::MAX`.
    #[must_use]
    pub const fn next(self) -> Option<Self> {
        match self.0.checked_add(1) {
            Some(v) => Some(Self(v)),
            None => None,
        }
    }
}

impl fmt::Display for Version {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        self.0.fmt(f)
    }
}

impl From<u64> for Version {
    fn from(version: u64) -> Self {
        Self(version)
    }
}

/// The chain identifier carried by every Transaction Stream response.
///
/// Aptos encodes it as a `u8` on-chain; the stream widens it to `u64`, so converting
/// from the wire value is fallible.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub struct ChainId(u8);

impl ChainId {
    pub const MAINNET: Self = Self(1);
    pub const TESTNET: Self = Self(2);

    #[must_use]
    pub const fn new(id: u8) -> Self {
        Self(id)
    }

    #[must_use]
    pub const fn get(self) -> u8 {
        self.0
    }
}

impl fmt::Display for ChainId {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        self.0.fmt(f)
    }
}

/// A wire chain id that doesn't fit Aptos' `u8` chain id.
#[derive(Debug, Clone, Copy, PartialEq, Eq, thiserror::Error)]
#[error("chain id {0} is out of range: Aptos chain ids are 0..=255")]
pub struct InvalidChainId(pub u64);

impl TryFrom<u64> for ChainId {
    type Error = InvalidChainId;

    fn try_from(id: u64) -> Result<Self, Self::Error> {
        u8::try_from(id).map(Self).map_err(|_| InvalidChainId(id))
    }
}

/// A public Aptos network.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum Network {
    Mainnet,
    Testnet,
    Devnet,
}

impl Network {
    pub const ALL: [Self; 3] = [Self::Mainnet, Self::Testnet, Self::Devnet];

    #[must_use]
    pub const fn as_str(self) -> &'static str {
        match self {
            Self::Mainnet => "mainnet",
            Self::Testnet => "testnet",
            Self::Devnet => "devnet",
        }
    }

    /// The chain id this network is known to use, if it's stable.
    ///
    /// Devnet is wiped and re-created periodically with a new chain id, so there is
    /// nothing fixed to check against.
    #[must_use]
    pub const fn chain_id(self) -> Option<ChainId> {
        match self {
            Self::Mainnet => Some(ChainId::MAINNET),
            Self::Testnet => Some(ChainId::TESTNET),
            Self::Devnet => None,
        }
    }
}

impl fmt::Display for Network {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str(self.as_str())
    }
}

/// A network name that isn't one of `mainnet`, `testnet` or `devnet`.
#[derive(Debug, Clone, PartialEq, Eq, thiserror::Error)]
#[error("unknown network `{0}`: expected one of mainnet, testnet, devnet")]
pub struct UnknownNetwork(pub String);

impl FromStr for Network {
    type Err = UnknownNetwork;

    fn from_str(s: &str) -> Result<Self, Self::Err> {
        Self::ALL
            .into_iter()
            .find(|n| n.as_str() == s)
            .ok_or_else(|| UnknownNetwork(s.to_owned()))
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn version_next_is_none_at_max() {
        assert_eq!(Version::GENESIS.next(), Some(Version::new(1)));
        assert_eq!(Version::new(u64::MAX).next(), None);
    }

    #[test]
    fn chain_id_rejects_out_of_range_wire_values() {
        assert_eq!(ChainId::try_from(2), Ok(ChainId::TESTNET));
        assert_eq!(ChainId::try_from(256), Err(InvalidChainId(256)));
    }

    #[test]
    fn network_round_trips_through_its_name() {
        for network in Network::ALL {
            assert_eq!(network.as_str().parse::<Network>(), Ok(network));
        }
        assert!("Mainnet".parse::<Network>().is_err());
    }
}
