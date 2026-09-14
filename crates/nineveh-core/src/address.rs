use std::fmt;
use std::str::FromStr;

/// A 32-byte Aptos account address.
///
/// The Transaction Stream renders addresses with leading zeros stripped (`"0xa"`, and
/// 63-digit addresses whose first nibble is zero), so parsing accepts 1 to 64 hex
/// digits. Nineveh always stores and serves the full-width form (ADR 0008), so `0x1`
/// and `0x000…001` can never become two keys.
#[derive(Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Hash)]
pub struct Address([u8; Self::LENGTH]);

impl Address {
    pub const LENGTH: usize = 32;

    pub const ZERO: Self = Self([0; Self::LENGTH]);

    /// `0x1`, where the Aptos framework lives.
    pub const ONE: Self = Self::special(1);

    #[must_use]
    pub const fn new(bytes: [u8; Self::LENGTH]) -> Self {
        Self(bytes)
    }

    /// One of the special addresses `0x0` through `0xff`.
    #[must_use]
    pub const fn special(low: u8) -> Self {
        let mut bytes = [0; Self::LENGTH];
        bytes[Self::LENGTH - 1] = low;
        Self(bytes)
    }

    #[must_use]
    pub const fn as_bytes(&self) -> &[u8; Self::LENGTH] {
        &self.0
    }

    /// Whether AIP-40 writes this address in short form: `0x0` through `0xf`.
    #[must_use]
    pub fn is_special(&self) -> bool {
        let (high, low) = self.0.split_at(Self::LENGTH - 1);
        high.iter().all(|&b| b == 0) && low[0] < 0x10
    }

    /// The AIP-40 form Aptos uses in type names: short for special addresses
    /// (`0x1`), full width otherwise.
    ///
    /// [`Display`](fmt::Display) always writes the full width; use this where the
    /// output should read like an Aptos type string.
    #[must_use]
    pub fn to_standard_string(&self) -> String {
        if self.is_special() {
            format!("{:#x}", self.0[Self::LENGTH - 1])
        } else {
            self.to_string()
        }
    }
}

impl fmt::Display for Address {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str("0x")?;
        for byte in self.0 {
            write!(f, "{byte:02x}")?;
        }
        Ok(())
    }
}

impl fmt::Debug for Address {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(f, "Address({self})")
    }
}

/// Text that isn't a `0x`-prefixed address of 1 to 64 hex digits.
#[derive(Debug, Clone, PartialEq, Eq, thiserror::Error)]
#[error("invalid address `{input}`: {reason}")]
pub struct InvalidAddress {
    pub input: String,
    pub reason: &'static str,
}

impl FromStr for Address {
    type Err = InvalidAddress;

    fn from_str(s: &str) -> Result<Self, Self::Err> {
        let invalid = |reason| InvalidAddress {
            input: s.to_owned(),
            reason,
        };
        let digits = s
            .strip_prefix("0x")
            .ok_or_else(|| invalid("expected a `0x` prefix"))?;
        if digits.is_empty() {
            return Err(invalid("expected hex digits after `0x`"));
        }
        if digits.len() > 2 * Self::LENGTH {
            return Err(invalid("more than 64 hex digits"));
        }

        let mut bytes = [0; Self::LENGTH];
        // Fill from the right: the last digit is the low nibble of the last byte.
        for (i, c) in digits.bytes().rev().enumerate() {
            let nibble = hex_value(c).ok_or_else(|| invalid("not a hex digit"))?;
            let byte = &mut bytes[Self::LENGTH - 1 - i / 2];
            *byte |= if i % 2 == 0 { nibble } else { nibble << 4 };
        }
        Ok(Self(bytes))
    }
}

const fn hex_value(c: u8) -> Option<u8> {
    match c {
        b'0'..=b'9' => Some(c - b'0'),
        b'a'..=b'f' => Some(c - b'a' + 10),
        b'A'..=b'F' => Some(c - b'A' + 10),
        _ => None,
    }
}

impl serde::Serialize for Address {
    fn serialize<S: serde::Serializer>(&self, serializer: S) -> Result<S::Ok, S::Error> {
        serializer.collect_str(self)
    }
}

impl<'de> serde::Deserialize<'de> for Address {
    fn deserialize<D: serde::Deserializer<'de>>(deserializer: D) -> Result<Self, D::Error> {
        let s = <std::borrow::Cow<'de, str>>::deserialize(deserializer)?;
        s.parse().map_err(serde::de::Error::custom)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    const FULL_ONE: &str = "0x0000000000000000000000000000000000000000000000000000000000000001";

    #[test]
    fn short_and_full_forms_are_the_same_address() {
        let short: Address = "0x1".parse().unwrap();
        let full: Address = FULL_ONE.parse().unwrap();
        assert_eq!(short, full);
        assert_eq!(short, Address::ONE);
        assert_eq!(short.to_string(), FULL_ONE);
        assert_eq!(short.to_standard_string(), "0x1");
    }

    #[test]
    fn stripped_leading_zeros_parse_with_odd_digit_counts() {
        // A 63-digit proposer address from testnet-11196227807.pb.
        let odd = "0x".to_owned() + &"f".repeat(63);
        let addr: Address = odd.parse().unwrap();
        assert_eq!(addr.to_string(), format!("0x0{}", "f".repeat(63)));
        assert_eq!("0xa".parse::<Address>().unwrap(), Address::special(10));
    }

    #[test]
    fn standard_form_is_short_only_for_0x0_through_0xf() {
        assert_eq!(Address::special(0xf).to_standard_string(), "0xf");
        assert_eq!(Address::special(0x10).to_standard_string().len(), 66);
        assert_eq!(Address::ZERO.to_standard_string(), "0x0");
    }

    #[test]
    fn upper_case_hex_normalizes_to_lower_case() {
        let addr: Address = "0xABC".parse().unwrap();
        assert!(addr.to_string().ends_with("abc"));
    }

    #[test]
    fn rejects_malformed_addresses() {
        for bad in [
            "",
            "1",
            "0x",
            "0xg",
            "0x 1",
            &format!("0x{}", "0".repeat(65)),
        ] {
            assert!(bad.parse::<Address>().is_err(), "{bad:?} should not parse");
        }
    }
}
