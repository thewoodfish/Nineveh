use std::cmp::Ordering;
use std::fmt;
use std::str::FromStr;

/// An unsigned 256-bit integer: Move's `u256`.
pub type U256 = ruint::aliases::U256;

/// A signed 256-bit integer: Move's `i256`, stored in two's complement.
///
/// This type carries values exactly; checked arithmetic belongs to `nineveh-expr`.
#[derive(Clone, Copy, PartialEq, Eq, Hash)]
pub struct I256(U256);

impl I256 {
    pub const ZERO: Self = Self(U256::ZERO);
    pub const MAX: Self = Self(U256::MAX.wrapping_shr(1));
    pub const MIN: Self = Self(Self::MAX.0.wrapping_add(U256::from_limbs([1, 0, 0, 0])));

    /// The two's-complement bits.
    #[must_use]
    pub const fn to_bits(self) -> U256 {
        self.0
    }

    #[must_use]
    pub const fn from_bits(bits: U256) -> Self {
        Self(bits)
    }

    #[must_use]
    pub fn is_negative(self) -> bool {
        self.0.bit(255)
    }

    /// The absolute value as an unsigned integer. Exact even for [`I256::MIN`].
    #[must_use]
    pub fn unsigned_abs(self) -> U256 {
        if self.is_negative() {
            self.0.wrapping_neg()
        } else {
            self.0
        }
    }
}

impl Ord for I256 {
    fn cmp(&self, other: &Self) -> Ordering {
        // Flipping the sign bit maps two's complement onto offset binary, which
        // orders like the unsigned integers.
        let flip = |x: U256| x ^ Self::MIN.0;
        flip(self.0).cmp(&flip(other.0))
    }
}

impl PartialOrd for I256 {
    fn partial_cmp(&self, other: &Self) -> Option<Ordering> {
        Some(self.cmp(other))
    }
}

impl fmt::Display for I256 {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        if self.is_negative() {
            f.write_str("-")?;
        }
        fmt::Display::fmt(&self.unsigned_abs(), f)
    }
}

impl fmt::Debug for I256 {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(f, "I256({self})")
    }
}

/// Text that isn't a decimal integer in range for its type.
#[derive(Debug, Clone, PartialEq, Eq, thiserror::Error)]
#[error("`{input}` is not a {ty} (expected a decimal integer in range)")]
pub struct InvalidInteger {
    pub input: String,
    pub ty: &'static str,
}

/// Parse a strictly decimal, unsigned integer: no sign, prefix, separators or spaces.
///
/// # Errors
///
/// If `s` isn't all ASCII digits or doesn't fit in 256 bits.
pub fn parse_u256(s: &str) -> Result<U256, InvalidInteger> {
    let invalid = || InvalidInteger {
        input: s.to_owned(),
        ty: "u256",
    };
    if s.is_empty() || !s.bytes().all(|b| b.is_ascii_digit()) {
        return Err(invalid());
    }
    U256::from_str_radix(s, 10).map_err(|_| invalid())
}

impl FromStr for I256 {
    type Err = InvalidInteger;

    /// Parse a decimal integer with an optional leading `-`.
    fn from_str(s: &str) -> Result<Self, Self::Err> {
        let invalid = || InvalidInteger {
            input: s.to_owned(),
            ty: "i256",
        };
        let (negative, digits) = match s.strip_prefix('-') {
            Some(rest) => (true, rest),
            None => (false, s),
        };
        let magnitude = parse_u256(digits).map_err(|_| invalid())?;
        if negative {
            if magnitude > Self::MIN.0 {
                return Err(invalid());
            }
            Ok(Self(magnitude.wrapping_neg()))
        } else {
            if magnitude > Self::MAX.0 {
                return Err(invalid());
            }
            Ok(Self(magnitude))
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    const I256_MAX: &str =
        "57896044618658097711785492504343953926634992332820282019728792003956564819967";
    const I256_MIN: &str =
        "-57896044618658097711785492504343953926634992332820282019728792003956564819968";

    #[test]
    fn i256_round_trips_its_extremes() {
        assert_eq!(I256_MAX.parse::<I256>().unwrap(), I256::MAX);
        assert_eq!(I256_MIN.parse::<I256>().unwrap(), I256::MIN);
        assert_eq!(I256::MAX.to_string(), I256_MAX);
        assert_eq!(I256::MIN.to_string(), I256_MIN);
    }

    #[test]
    fn i256_rejects_out_of_range_and_malformed() {
        let past_max =
            "57896044618658097711785492504343953926634992332820282019728792003956564819968";
        let past_min = format!(
            "-{}",
            "57896044618658097711785492504343953926634992332820282019728792003956564819969"
        );
        for bad in [
            past_max, &past_min, "", "-", "+1", "1.0", " 1", "0x1", "--1",
        ] {
            assert!(bad.parse::<I256>().is_err(), "{bad:?}");
        }
    }

    #[test]
    fn i256_orders_by_value() {
        let mut values: Vec<I256> = ["5", "-1", "0", I256_MIN, I256_MAX, "-6294071852848"]
            .iter()
            .map(|s| s.parse().unwrap())
            .collect();
        values.sort();
        let sorted: Vec<String> = values.iter().map(ToString::to_string).collect();
        assert_eq!(
            sorted,
            [I256_MIN, "-6294071852848", "-1", "0", "5", I256_MAX]
        );
    }

    #[test]
    fn parse_u256_is_strictly_decimal() {
        assert_eq!(parse_u256("0").unwrap(), U256::ZERO);
        assert_eq!(parse_u256(&U256::MAX.to_string()).unwrap(), U256::MAX);
        for bad in ["", "0x10", "-1", "1_000", " 1", "1e3"] {
            assert!(parse_u256(bad).is_err(), "{bad:?}");
        }
    }
}
