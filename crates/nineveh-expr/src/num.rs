//! Exact, checked integer arithmetic for every Move integer type.
//!
//! Every integer is lifted to one representation, a sign and a 256-bit magnitude, which
//! holds every value of every type from `i256::MIN` to `u256::MAX`. Operations run on
//! that and the result is checked against the operand type's range. One code path for
//! twelve types is easier to get exactly right than twelve, and none of it can wrap:
//! every step is a checked `U256` operation.

use std::cmp::Ordering;

use nineveh_core::{I256, U256, Value};

use crate::types::IntType;

/// An integer of any Move type: `-mag` if `neg`, else `mag`. Zero is never negative.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) struct Int {
    neg: bool,
    mag: U256,
}

/// Why an integer operation has no result.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) enum IntError {
    /// The exact result is outside the type's range.
    OutOfRange,
    DivideByZero,
}

impl Int {
    pub(crate) const ZERO: Self = Self {
        neg: false,
        mag: U256::ZERO,
    };

    fn new(neg: bool, mag: U256) -> Self {
        Self {
            neg: neg && mag != U256::ZERO,
            mag,
        }
    }

    pub(crate) fn from_u128(v: u128) -> Self {
        Self::new(false, U256::from(v))
    }

    pub(crate) fn from_i128(v: i128) -> Self {
        Self::new(v < 0, U256::from(v.unsigned_abs()))
    }

    pub(crate) fn from_u256(v: U256) -> Self {
        Self::new(false, v)
    }

    pub(crate) fn from_i256(v: I256) -> Self {
        Self::new(v.is_negative(), v.unsigned_abs())
    }

    /// The integer held by `value`, if it's an integer.
    pub(crate) fn from_value(value: &Value) -> Option<(IntType, Self)> {
        Some(match value {
            Value::U8(v) => (IntType::U8, Self::from_u128(u128::from(*v))),
            Value::U16(v) => (IntType::U16, Self::from_u128(u128::from(*v))),
            Value::U32(v) => (IntType::U32, Self::from_u128(u128::from(*v))),
            Value::U64(v) => (IntType::U64, Self::from_u128(u128::from(*v))),
            Value::U128(v) => (IntType::U128, Self::from_u128(*v)),
            Value::U256(v) => (IntType::U256, Self::from_u256(*v)),
            Value::I8(v) => (IntType::I8, Self::from_i128(i128::from(*v))),
            Value::I16(v) => (IntType::I16, Self::from_i128(i128::from(*v))),
            Value::I32(v) => (IntType::I32, Self::from_i128(i128::from(*v))),
            Value::I64(v) => (IntType::I64, Self::from_i128(i128::from(*v))),
            Value::I128(v) => (IntType::I128, Self::from_i128(*v)),
            Value::I256(v) => (IntType::I256, Self::from_i256(*v)),
            _ => return None,
        })
    }

    /// This integer as a value of type `ty`, if it's in range.
    pub(crate) fn to_value(self, ty: IntType) -> Result<Value, IntError> {
        let out = || IntError::OutOfRange;
        if self.neg && !ty.is_signed() {
            return Err(out());
        }
        let small_unsigned =
            || -> Result<u128, IntError> { u128::try_from(self.mag).map_err(|_| out()) };
        let small_signed = || -> Result<i128, IntError> {
            let mag = u128::try_from(self.mag).map_err(|_| out())?;
            if self.neg {
                // i128::MIN's magnitude is one more than i128::MAX.
                0i128.checked_sub_unsigned(mag).ok_or_else(out)
            } else {
                i128::try_from(mag).map_err(|_| out())
            }
        };
        Ok(match ty {
            IntType::U8 => Value::U8(u8::try_from(small_unsigned()?).map_err(|_| out())?),
            IntType::U16 => Value::U16(u16::try_from(small_unsigned()?).map_err(|_| out())?),
            IntType::U32 => Value::U32(u32::try_from(small_unsigned()?).map_err(|_| out())?),
            IntType::U64 => Value::U64(u64::try_from(small_unsigned()?).map_err(|_| out())?),
            IntType::U128 => Value::U128(small_unsigned()?),
            IntType::U256 => Value::U256(self.mag),
            IntType::I8 => Value::I8(i8::try_from(small_signed()?).map_err(|_| out())?),
            IntType::I16 => Value::I16(i16::try_from(small_signed()?).map_err(|_| out())?),
            IntType::I32 => Value::I32(i32::try_from(small_signed()?).map_err(|_| out())?),
            IntType::I64 => Value::I64(i64::try_from(small_signed()?).map_err(|_| out())?),
            IntType::I128 => Value::I128(small_signed()?),
            IntType::I256 => {
                let limit = I256::MIN.unsigned_abs();
                if self.mag > limit || (!self.neg && self.mag == limit) {
                    return Err(out());
                }
                let bits = if self.neg {
                    self.mag.wrapping_neg()
                } else {
                    self.mag
                };
                Value::I256(I256::from_bits(bits))
            }
        })
    }

    pub(crate) fn neg(self) -> Self {
        Self::new(!self.neg, self.mag)
    }

    pub(crate) fn abs(self) -> Self {
        Self::new(false, self.mag)
    }

    pub(crate) fn add(self, other: Self) -> Result<Self, IntError> {
        if self.neg == other.neg {
            let mag = self
                .mag
                .checked_add(other.mag)
                .ok_or(IntError::OutOfRange)?;
            Ok(Self::new(self.neg, mag))
        } else if self.mag >= other.mag {
            Ok(Self::new(self.neg, self.mag - other.mag))
        } else {
            Ok(Self::new(other.neg, other.mag - self.mag))
        }
    }

    pub(crate) fn sub(self, other: Self) -> Result<Self, IntError> {
        self.add(other.neg())
    }

    pub(crate) fn mul(self, other: Self) -> Result<Self, IntError> {
        let mag = self
            .mag
            .checked_mul(other.mag)
            .ok_or(IntError::OutOfRange)?;
        Ok(Self::new(self.neg != other.neg, mag))
    }

    /// Division truncating toward zero, like Rust and Move.
    pub(crate) fn div(self, other: Self) -> Result<Self, IntError> {
        if other.mag == U256::ZERO {
            return Err(IntError::DivideByZero);
        }
        Ok(Self::new(self.neg != other.neg, self.mag / other.mag))
    }

    /// The remainder of truncating division: it takes the dividend's sign.
    pub(crate) fn rem(self, other: Self) -> Result<Self, IntError> {
        if other.mag == U256::ZERO {
            return Err(IntError::DivideByZero);
        }
        Ok(Self::new(self.neg, self.mag % other.mag))
    }
}

impl Ord for Int {
    fn cmp(&self, other: &Self) -> Ordering {
        match (self.neg, other.neg) {
            (false, false) => self.mag.cmp(&other.mag),
            (true, true) => other.mag.cmp(&self.mag),
            (true, false) => Ordering::Less,
            (false, true) => Ordering::Greater,
        }
    }
}

impl PartialOrd for Int {
    fn partial_cmp(&self, other: &Self) -> Option<Ordering> {
        Some(self.cmp(other))
    }
}

/// Parse a literal's digits (no sign, `_` separators allowed) into an unsigned
/// magnitude.
pub(crate) fn parse_digits(digits: &str) -> Option<U256> {
    let clean: String = digits.chars().filter(|&c| c != '_').collect();
    nineveh_core::parse_u256(&clean).ok()
}

#[cfg(test)]
mod tests {
    use super::*;

    fn v(value: &Value) -> Int {
        Int::from_value(value).unwrap().1
    }

    #[test]
    fn extremes_round_trip() {
        for value in [
            Value::U8(u8::MAX),
            Value::U256(U256::MAX),
            Value::I8(i8::MIN),
            Value::I128(i128::MIN),
            Value::I128(i128::MAX),
            Value::I256(I256::MIN),
            Value::I256(I256::MAX),
        ] {
            let (ty, int) = Int::from_value(&value).unwrap();
            assert_eq!(int.to_value(ty), Ok(value));
        }
    }

    #[test]
    fn range_checks_on_the_way_out() {
        assert_eq!(
            Int::from_u128(256).to_value(IntType::U8),
            Err(IntError::OutOfRange)
        );
        assert_eq!(
            Int::from_i128(-1).to_value(IntType::U256),
            Err(IntError::OutOfRange)
        );
        assert_eq!(
            Int::from_i128(-129).to_value(IntType::I8),
            Err(IntError::OutOfRange)
        );
        assert_eq!(
            Int::from_i128(128).to_value(IntType::I8),
            Err(IntError::OutOfRange)
        );
        let min = v(&Value::I256(I256::MIN));
        assert_eq!(min.neg().to_value(IntType::I256), Err(IntError::OutOfRange));
    }

    #[test]
    fn u256_overflow_is_an_error_not_a_wrap() {
        let max = v(&Value::U256(U256::MAX));
        assert_eq!(max.add(Int::from_u128(1)), Err(IntError::OutOfRange));
        assert_eq!(max.mul(Int::from_u128(2)), Err(IntError::OutOfRange));
        assert_eq!(
            Int::ZERO
                .sub(Int::from_u128(1))
                .unwrap()
                .to_value(IntType::U256),
            Err(IntError::OutOfRange)
        );
    }

    #[test]
    fn division_truncates_toward_zero() {
        let q = Int::from_i128(-7).div(Int::from_i128(2)).unwrap();
        assert_eq!(q.to_value(IntType::I64), Ok(Value::I64(-3)));
        let r = Int::from_i128(-7).rem(Int::from_i128(2)).unwrap();
        assert_eq!(r.to_value(IntType::I64), Ok(Value::I64(-1)));
        assert_eq!(
            Int::from_i128(1).div(Int::ZERO),
            Err(IntError::DivideByZero)
        );
    }

    #[test]
    fn zero_is_never_negative() {
        assert_eq!(Int::from_i128(5).sub(Int::from_i128(5)).unwrap(), Int::ZERO);
        assert_eq!(Int::ZERO.neg(), Int::ZERO);
    }
}
