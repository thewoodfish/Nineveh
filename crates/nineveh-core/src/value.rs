use serde::ser::{SerializeMap, SerializeSeq};

use crate::{Address, I256, Identifier, U256};

/// A decoded Move value, exact to 256 bits.
///
/// `nineveh-decode` produces these from the stream's JSON, guided by a pinned layout,
/// so every value has already been checked against its Move type. The framework's
/// `String` and `Option<T>` get their own variants because they map to their own
/// column types (ADR 0008). Every other struct, including `Object<T>`, stays a
/// [`Value::Struct`].
///
/// Values are totally ordered and hashable so they can key state rows. Integers of
/// the same type order numerically; the order between different variants is fixed but
/// otherwise meaningless.
#[derive(Debug, Clone, PartialEq, Eq, PartialOrd, Ord, Hash)]
pub enum Value {
    Bool(bool),
    U8(u8),
    U16(u16),
    U32(u32),
    U64(u64),
    U128(u128),
    U256(U256),
    I8(i8),
    I16(i16),
    I32(i32),
    I64(i64),
    I128(i128),
    I256(I256),
    Address(Address),
    /// `0x1::string::String`.
    String(String),
    /// `vector<u8>`.
    Bytes(Vec<u8>),
    Vector(Vec<Value>),
    /// `0x1::option::Option<T>`.
    Option(Option<Box<Value>>),
    /// A struct's fields, in declaration order.
    Struct(Fields),
    /// A Move 2 enum value: the variant's name and its fields.
    Variant {
        name: Identifier,
        fields: Fields,
    },
}

/// A struct's or variant's fields, in declaration order.
pub type Fields = Vec<(Identifier, Value)>;

impl Value {
    /// The named field of a struct or enum variant.
    #[must_use]
    pub fn field(&self, name: &str) -> Option<&Value> {
        match self {
            Self::Struct(fields) | Self::Variant { fields, .. } => {
                fields.iter().find(|(n, _)| n == name).map(|(_, v)| v)
            }
            _ => None,
        }
    }
}

/// Serializes in the API's JSON shape (ADR 0008): integers wider than 32 bits as
/// decimal strings, addresses at full width, bytes as `0x` hex, `Option` as the value
/// or `null`, structs as objects in field order, and enum values as objects with a
/// `__variant__` key, like the stream.
impl serde::Serialize for Value {
    fn serialize<S: serde::Serializer>(&self, s: S) -> Result<S::Ok, S::Error> {
        match self {
            Self::Bool(b) => s.serialize_bool(*b),
            Self::U8(n) => s.serialize_u8(*n),
            Self::U16(n) => s.serialize_u16(*n),
            Self::U32(n) => s.serialize_u32(*n),
            Self::I8(n) => s.serialize_i8(*n),
            Self::I16(n) => s.serialize_i16(*n),
            Self::I32(n) => s.serialize_i32(*n),
            Self::U64(n) => s.collect_str(n),
            Self::U128(n) => s.collect_str(n),
            Self::U256(n) => s.collect_str(n),
            Self::I64(n) => s.collect_str(n),
            Self::I128(n) => s.collect_str(n),
            Self::I256(n) => s.collect_str(n),
            Self::Address(a) => s.collect_str(a),
            Self::String(text) => s.serialize_str(text),
            Self::Bytes(bytes) => s.collect_str(&HexBytes(bytes)),
            Self::Vector(items) => {
                let mut seq = s.serialize_seq(Some(items.len()))?;
                for item in items {
                    seq.serialize_element(item)?;
                }
                seq.end()
            }
            Self::Option(None) => s.serialize_none(),
            Self::Option(Some(v)) => s.serialize_some(v),
            Self::Struct(fields) => {
                let mut map = s.serialize_map(Some(fields.len()))?;
                for (name, value) in fields {
                    map.serialize_entry(name.as_str(), value)?;
                }
                map.end()
            }
            Self::Variant { name, fields } => {
                let mut map = s.serialize_map(Some(fields.len() + 1))?;
                map.serialize_entry("__variant__", name.as_str())?;
                for (field, value) in fields {
                    map.serialize_entry(field.as_str(), value)?;
                }
                map.end()
            }
        }
    }
}

struct HexBytes<'a>(&'a [u8]);

impl std::fmt::Display for HexBytes<'_> {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.write_str("0x")?;
        for byte in self.0 {
            write!(f, "{byte:02x}")?;
        }
        Ok(())
    }
}
