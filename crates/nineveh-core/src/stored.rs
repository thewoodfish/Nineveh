//! A lossless encoding of [`Value`], for Nineveh's own storage.
//!
//! [`Value`]'s own `Serialize` renders the API's shape (ADR 0008): wide integers as
//! strings, `Option` unwrapped, `Object<T>` as its address. That is a contract with
//! users, and it cannot be read back — a JSON string doesn't say whether it was a
//! `u64` or a `u128`, and an unwrapped `Option` doesn't say it was one.
//!
//! The record log (ADR 0022) needs a round trip, so it gets its own shape, tagged by
//! variant and carrying every discriminator the API's drops. The two are separate on
//! purpose: one is a contract with users, the other is a contract with ourselves, and
//! they are free to change independently.
//!
//! Integers wider than 32 bits travel as decimal strings. JSON numbers cannot hold a
//! `u128` or a `u256` exactly, and nothing is gained by finding out where the limit is.

use std::str::FromStr as _;

use serde::{Deserialize, Serialize};

use crate::{Address, Fields, I256, Identifier, Value, parse_u256};

/// [`Value`] in a shape that survives a round trip through JSON.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub enum Stored {
    Bool(bool),
    U8(u8),
    U16(u16),
    U32(u32),
    U64(String),
    U128(String),
    U256(String),
    I8(i8),
    I16(i16),
    I32(i32),
    I64(String),
    I128(String),
    I256(String),
    Address(String),
    String(String),
    /// Hex, without a `0x` prefix.
    Bytes(String),
    Vector(Vec<Stored>),
    Option(Option<Box<Stored>>),
    Struct(Vec<(String, Stored)>),
    Variant {
        name: String,
        fields: Vec<(String, Stored)>,
    },
}

/// A stored value that doesn't decode: written by an older or broken encoder, or
/// corrupted in the database.
#[derive(Debug, thiserror::Error)]
#[error("stored value is not a {expected}: {reason}")]
pub struct InvalidStored {
    pub expected: &'static str,
    pub reason: String,
}

impl InvalidStored {
    fn new(expected: &'static str, reason: &impl ToString) -> Self {
        Self {
            expected,
            reason: reason.to_string(),
        }
    }
}

fn fields_out(fields: &Fields) -> Vec<(String, Stored)> {
    fields
        .iter()
        .map(|(name, value)| (name.to_string(), Stored::from(value)))
        .collect()
}

fn fields_in(fields: Vec<(String, Stored)>) -> Result<Fields, InvalidStored> {
    fields
        .into_iter()
        .map(|(name, value)| {
            let name =
                Identifier::from_str(&name).map_err(|e| InvalidStored::new("identifier", &e))?;
            Ok((name, Value::try_from(value)?))
        })
        .collect()
}

impl From<&Value> for Stored {
    fn from(value: &Value) -> Self {
        match value {
            Value::Bool(b) => Self::Bool(*b),
            Value::U8(n) => Self::U8(*n),
            Value::U16(n) => Self::U16(*n),
            Value::U32(n) => Self::U32(*n),
            Value::U64(n) => Self::U64(n.to_string()),
            Value::U128(n) => Self::U128(n.to_string()),
            Value::U256(n) => Self::U256(n.to_string()),
            Value::I8(n) => Self::I8(*n),
            Value::I16(n) => Self::I16(*n),
            Value::I32(n) => Self::I32(*n),
            Value::I64(n) => Self::I64(n.to_string()),
            Value::I128(n) => Self::I128(n.to_string()),
            Value::I256(n) => Self::I256(n.to_string()),
            Value::Address(a) => Self::Address(a.to_string()),
            Value::String(s) => Self::String(s.clone()),
            Value::Bytes(b) => Self::Bytes(hex(b)),
            Value::Vector(items) => Self::Vector(items.iter().map(Self::from).collect()),
            Value::Option(inner) => {
                Self::Option(inner.as_ref().map(|v| Box::new(Self::from(v.as_ref()))))
            }
            Value::Struct(fields) => Self::Struct(fields_out(fields)),
            Value::Variant { name, fields } => Self::Variant {
                name: name.to_string(),
                fields: fields_out(fields),
            },
        }
    }
}

impl TryFrom<Stored> for Value {
    type Error = InvalidStored;

    fn try_from(stored: Stored) -> Result<Self, Self::Error> {
        let int = |what: &'static str, s: &str| {
            s.parse::<u64>().map_err(|e| InvalidStored::new(what, &e))
        };
        Ok(match stored {
            Stored::Bool(b) => Self::Bool(b),
            Stored::U8(n) => Self::U8(n),
            Stored::U16(n) => Self::U16(n),
            Stored::U32(n) => Self::U32(n),
            Stored::U64(s) => Self::U64(int("u64", &s)?),
            Stored::U128(s) => Self::U128(s.parse().map_err(|e| InvalidStored::new("u128", &e))?),
            Stored::U256(s) => {
                Self::U256(parse_u256(&s).map_err(|e| InvalidStored::new("u256", &e))?)
            }
            Stored::I8(n) => Self::I8(n),
            Stored::I16(n) => Self::I16(n),
            Stored::I32(n) => Self::I32(n),
            Stored::I64(s) => Self::I64(s.parse().map_err(|e| InvalidStored::new("i64", &e))?),
            Stored::I128(s) => Self::I128(s.parse().map_err(|e| InvalidStored::new("i128", &e))?),
            Stored::I256(s) => {
                Self::I256(I256::from_str(&s).map_err(|e| InvalidStored::new("i256", &e))?)
            }
            Stored::Address(s) => {
                Self::Address(Address::from_str(&s).map_err(|e| InvalidStored::new("address", &e))?)
            }
            Stored::String(s) => Self::String(s),
            Stored::Bytes(s) => Self::Bytes(unhex(&s)?),
            Stored::Vector(items) => Self::Vector(
                items
                    .into_iter()
                    .map(Self::try_from)
                    .collect::<Result<_, _>>()?,
            ),
            Stored::Option(inner) => Self::Option(match inner {
                Some(v) => Some(Box::new(Self::try_from(*v)?)),
                None => None,
            }),
            Stored::Struct(fields) => Self::Struct(fields_in(fields)?),
            Stored::Variant { name, fields } => Self::Variant {
                name: Identifier::from_str(&name)
                    .map_err(|e| InvalidStored::new("identifier", &e))?,
                fields: fields_in(fields)?,
            },
        })
    }
}

fn hex(bytes: &[u8]) -> String {
    use std::fmt::Write as _;
    let mut out = String::with_capacity(bytes.len() * 2);
    for byte in bytes {
        // Writing to a String can't fail.
        let _ = write!(out, "{byte:02x}");
    }
    out
}

fn unhex(s: &str) -> Result<Vec<u8>, InvalidStored> {
    if !s.len().is_multiple_of(2) {
        return Err(InvalidStored::new("bytes", &"odd number of hex digits"));
    }
    (0..s.len())
        .step_by(2)
        .map(|i| {
            let pair = s.get(i..i + 2).ok_or_else(|| {
                InvalidStored::new("bytes", &"hex digit pair straddles a character")
            })?;
            u8::from_str_radix(pair, 16).map_err(|e| InvalidStored::new("bytes", &e))
        })
        .collect()
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::U256;

    fn round_trip(value: &Value) {
        let stored = Stored::from(value);
        let json = serde_json::to_string(&stored).expect("serializes");
        let back: Stored = serde_json::from_str(&json).expect("deserializes");
        let value_back = Value::try_from(back).expect("converts back");
        assert_eq!(&value_back, value, "round trip changed {json}");
    }

    fn ident(s: &str) -> Identifier {
        Identifier::from_str(s).expect("valid identifier")
    }

    #[test]
    fn every_variant_survives_a_round_trip() {
        for value in [
            Value::Bool(true),
            Value::U8(u8::MAX),
            Value::U16(u16::MAX),
            Value::U32(u32::MAX),
            Value::U64(u64::MAX),
            Value::U128(u128::MAX),
            Value::U256(U256::MAX),
            Value::I8(i8::MIN),
            Value::I16(i16::MIN),
            Value::I32(i32::MIN),
            Value::I64(i64::MIN),
            Value::I128(i128::MIN),
            Value::I256(I256::MIN),
            Value::I256(I256::MAX),
            Value::Address(Address::special(1)),
            Value::String("hello ✨".to_owned()),
            Value::Bytes(vec![0, 1, 0xfe, 0xff]),
            Value::Bytes(Vec::new()),
            Value::Vector(vec![Value::U8(1), Value::U8(2)]),
            Value::Option(None),
            Value::Option(Some(Box::new(Value::U64(7)))),
            Value::Struct(vec![(ident("a"), Value::Bool(false))]),
            Value::Variant {
                name: ident("Win"),
                fields: vec![(ident("by"), Value::U64(3))],
            },
        ] {
            round_trip(&value);
        }
    }

    /// The point of the whole module: the API's shape can't tell these apart, and this
    /// one has to.
    #[test]
    fn integer_widths_are_not_confused() {
        let one = |v: Value| serde_json::to_string(&Stored::from(&v)).expect("serializes");
        assert_ne!(one(Value::U64(1)), one(Value::U128(1)));
        assert_ne!(one(Value::U128(1)), one(Value::U256(U256::from(1u8))));
        assert_ne!(one(Value::I64(1)), one(Value::U64(1)));
    }

    /// An unwrapped `Option` is indistinguishable from its contents in the API shape.
    #[test]
    fn an_option_keeps_its_wrapper() {
        let bare = serde_json::to_string(&Stored::from(&Value::U64(1))).expect("serializes");
        let wrapped =
            serde_json::to_string(&Stored::from(&Value::Option(Some(Box::new(Value::U64(1))))))
                .expect("serializes");
        assert_ne!(bare, wrapped);
    }

    #[test]
    fn nested_structures_survive() {
        round_trip(&Value::Struct(vec![
            (
                ident("inner"),
                Value::Vector(vec![Value::Option(Some(Box::new(Value::Address(
                    Address::special(3),
                ))))]),
            ),
            (ident("wide"), Value::U256(U256::MAX)),
        ]));
    }

    #[test]
    fn a_broken_value_is_an_error_not_a_panic() {
        assert!(Value::try_from(Stored::U64("not a number".to_owned())).is_err());
        assert!(Value::try_from(Stored::Bytes("abc".to_owned())).is_err());
        assert!(Value::try_from(Stored::Address("0xzz".to_owned())).is_err());
    }
}
