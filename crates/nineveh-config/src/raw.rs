//! The YAML schema, as serde types that keep every value's location.
//!
//! These types only capture shape. Meaning (names, references, types) is checked in
//! `validate`, which collects every problem instead of stopping at the first.

use std::fmt;
use std::marker::PhantomData;

use serde::Deserialize;
use serde::de::{self, MapAccess, SeqAccess, Visitor};
use serde_saphyr::Spanned;

#[derive(Debug, Deserialize)]
#[serde(deny_unknown_fields)]
pub(crate) struct RawConfig {
    pub(crate) name: Spanned<String>,
    pub(crate) network: Spanned<String>,
    #[serde(default)]
    pub(crate) start_version: Option<Spanned<RawStart>>,
    pub(crate) sources: Spanned<Entries<Spanned<RawSource>>>,
    pub(crate) state: Spanned<Entries<Spanned<RawTable>>>,
    #[serde(default)]
    pub(crate) api: Option<RawApi>,
    #[serde(default)]
    pub(crate) webhooks: Entries<Spanned<RawWebhook>>,
}

#[derive(Debug)]
pub(crate) enum RawStart {
    Auto,
    Version(u64),
}

#[derive(Debug, Deserialize)]
#[serde(deny_unknown_fields, rename_all = "snake_case")]
pub(crate) enum RawSource {
    Event(Spanned<String>),
    Resource(Spanned<String>),
    Table(Spanned<String>),
}

#[derive(Debug, Deserialize)]
#[serde(deny_unknown_fields)]
pub(crate) struct RawTable {
    #[serde(default)]
    pub(crate) key: Option<Spanned<Vec<Spanned<String>>>>,
    #[serde(default)]
    pub(crate) columns: Option<Spanned<Entries<Spanned<RawColumn>>>>,
    #[serde(default)]
    pub(crate) reduce: Option<Spanned<Vec<Spanned<RawRule>>>>,
    #[serde(default)]
    pub(crate) mirror: Option<Spanned<String>>,
    #[serde(default)]
    pub(crate) log: Option<Spanned<String>>,
}

/// A column: `balance: u128` or `balance: { type: u128, default: 0 }`.
#[derive(Debug)]
pub(crate) struct RawColumn {
    pub(crate) ty: Spanned<String>,
    pub(crate) default: Option<Spanned<Literal>>,
    pub(crate) nullable: bool,
}

#[derive(Debug, Deserialize)]
#[serde(deny_unknown_fields)]
struct RawColumnLong {
    #[serde(rename = "type")]
    ty: Spanned<String>,
    #[serde(default)]
    default: Option<Spanned<Literal>>,
    #[serde(default)]
    nullable: bool,
}

#[derive(Debug, Deserialize)]
#[serde(deny_unknown_fields)]
pub(crate) struct RawRule {
    pub(crate) on: Spanned<String>,
    #[serde(default)]
    pub(crate) when: Option<Spanned<ExprText>>,
    #[serde(default)]
    pub(crate) key: Option<Spanned<Entries<Spanned<ExprText>>>>,
    #[serde(default)]
    pub(crate) set: Option<Spanned<Entries<Spanned<ExprText>>>>,
    #[serde(default)]
    pub(crate) delete: Option<Spanned<bool>>,
}

#[derive(Debug, Default, Deserialize)]
#[serde(deny_unknown_fields)]
pub(crate) struct RawApi {
    #[serde(default)]
    pub(crate) rest: Option<bool>,
    #[serde(default)]
    pub(crate) graphql: Option<bool>,
}

#[derive(Debug, Deserialize)]
#[serde(deny_unknown_fields)]
pub(crate) struct RawWebhook {
    pub(crate) url: Spanned<String>,
    pub(crate) on: Vec<Spanned<String>>,
    /// Whether deliveries carry the changed row, not only its key. Default: they do.
    #[serde(default)]
    pub(crate) rows: Option<bool>,
}

/// A YAML mapping kept in document order, with each key's location.
#[derive(Debug)]
pub(crate) struct Entries<V>(pub(crate) Vec<(Spanned<String>, V)>);

impl<V> Default for Entries<V> {
    fn default() -> Self {
        Self(Vec::new())
    }
}

impl<'de, V: Deserialize<'de>> Deserialize<'de> for Entries<V> {
    fn deserialize<D: de::Deserializer<'de>>(d: D) -> Result<Self, D::Error> {
        struct EntriesVisitor<V>(PhantomData<V>);

        impl<'de, V: Deserialize<'de>> Visitor<'de> for EntriesVisitor<V> {
            type Value = Entries<V>;

            fn expecting(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
                f.write_str("a mapping")
            }

            fn visit_map<A: MapAccess<'de>>(self, mut map: A) -> Result<Self::Value, A::Error> {
                let mut entries = Vec::new();
                while let Some(entry) = map.next_entry()? {
                    entries.push(entry);
                }
                Ok(Entries(entries))
            }
        }

        d.deserialize_map(EntriesVisitor(PhantomData))
    }
}

impl<'de> Deserialize<'de> for RawStart {
    fn deserialize<D: de::Deserializer<'de>>(d: D) -> Result<Self, D::Error> {
        struct StartVisitor;

        impl Visitor<'_> for StartVisitor {
            type Value = RawStart;

            fn expecting(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
                f.write_str("`auto` or a transaction version")
            }

            fn visit_u64<E: de::Error>(self, v: u64) -> Result<RawStart, E> {
                Ok(RawStart::Version(v))
            }

            fn visit_i64<E: de::Error>(self, v: i64) -> Result<RawStart, E> {
                u64::try_from(v)
                    .map(RawStart::Version)
                    .map_err(|_| E::custom("start_version can't be negative"))
            }

            fn visit_str<E: de::Error>(self, v: &str) -> Result<RawStart, E> {
                match v {
                    "auto" => Ok(RawStart::Auto),
                    _ => Err(E::invalid_value(de::Unexpected::Str(v), &self)),
                }
            }
        }

        d.deserialize_any(StartVisitor)
    }
}

impl<'de> Deserialize<'de> for RawColumn {
    fn deserialize<D: de::Deserializer<'de>>(d: D) -> Result<Self, D::Error> {
        struct ColumnVisitor;

        impl<'de> Visitor<'de> for ColumnVisitor {
            type Value = RawColumn;

            fn expecting(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
                f.write_str("a column type, or a mapping with `type`, `default` and `nullable`")
            }

            fn visit_str<E: de::Error>(self, v: &str) -> Result<RawColumn, E> {
                // The shorthand's location is the enclosing `Spanned<RawColumn>`'s.
                Ok(RawColumn {
                    ty: Spanned::new(
                        v.to_owned(),
                        serde_saphyr::Location::UNKNOWN,
                        serde_saphyr::Location::UNKNOWN,
                    ),
                    default: None,
                    nullable: false,
                })
            }

            fn visit_map<A: MapAccess<'de>>(self, map: A) -> Result<RawColumn, A::Error> {
                let long = RawColumnLong::deserialize(de::value::MapAccessDeserializer::new(map))?;
                Ok(RawColumn {
                    ty: long.ty,
                    default: long.default,
                    nullable: long.nullable,
                })
            }
        }

        d.deserialize_any(ColumnVisitor)
    }
}

/// A scalar default value, before it's checked against its column's type.
#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) enum Literal {
    Null,
    Bool(bool),
    Int(i128),
    /// Larger than `i128`: kept as decimal text for `u128` and `u256` columns.
    BigUint(u128),
    Str(String),
}

impl<'de> Deserialize<'de> for Literal {
    fn deserialize<D: de::Deserializer<'de>>(d: D) -> Result<Self, D::Error> {
        struct LiteralVisitor;

        impl Visitor<'_> for LiteralVisitor {
            type Value = Literal;

            fn expecting(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
                f.write_str("a boolean, an integer, a string or null")
            }

            fn visit_unit<E: de::Error>(self) -> Result<Literal, E> {
                Ok(Literal::Null)
            }

            fn visit_none<E: de::Error>(self) -> Result<Literal, E> {
                Ok(Literal::Null)
            }

            fn visit_bool<E: de::Error>(self, v: bool) -> Result<Literal, E> {
                Ok(Literal::Bool(v))
            }

            fn visit_i64<E: de::Error>(self, v: i64) -> Result<Literal, E> {
                Ok(Literal::Int(i128::from(v)))
            }

            fn visit_u64<E: de::Error>(self, v: u64) -> Result<Literal, E> {
                Ok(Literal::Int(i128::from(v)))
            }

            fn visit_i128<E: de::Error>(self, v: i128) -> Result<Literal, E> {
                Ok(Literal::Int(v))
            }

            fn visit_u128<E: de::Error>(self, v: u128) -> Result<Literal, E> {
                Ok(i128::try_from(v).map_or(Literal::BigUint(v), Literal::Int))
            }

            fn visit_f64<E: de::Error>(self, _: f64) -> Result<Literal, E> {
                Err(E::custom(
                    "floating-point numbers aren't supported; Nineveh's numbers are exact integers",
                ))
            }

            fn visit_str<E: de::Error>(self, v: &str) -> Result<Literal, E> {
                Ok(Literal::Str(v.to_owned()))
            }
        }

        d.deserialize_any(LiteralVisitor)
    }
}

/// An expression's source text. YAML turns unquoted `1` and `true` into a number and a
/// boolean, so those are accepted and turned back into their text.
#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) struct ExprText(pub(crate) String);

impl<'de> Deserialize<'de> for ExprText {
    fn deserialize<D: de::Deserializer<'de>>(d: D) -> Result<Self, D::Error> {
        struct ExprVisitor;

        impl<'de> Visitor<'de> for ExprVisitor {
            type Value = ExprText;

            fn expecting(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
                f.write_str("an expression, like \"balance + amount\"")
            }

            fn visit_bool<E: de::Error>(self, v: bool) -> Result<ExprText, E> {
                Ok(ExprText(v.to_string()))
            }

            fn visit_i64<E: de::Error>(self, v: i64) -> Result<ExprText, E> {
                Ok(ExprText(v.to_string()))
            }

            fn visit_u64<E: de::Error>(self, v: u64) -> Result<ExprText, E> {
                Ok(ExprText(v.to_string()))
            }

            fn visit_i128<E: de::Error>(self, v: i128) -> Result<ExprText, E> {
                Ok(ExprText(v.to_string()))
            }

            fn visit_u128<E: de::Error>(self, v: u128) -> Result<ExprText, E> {
                Ok(ExprText(v.to_string()))
            }

            fn visit_f64<E: de::Error>(self, _: f64) -> Result<ExprText, E> {
                Err(E::custom(
                    "floating-point numbers aren't supported; Nineveh's numbers are exact integers",
                ))
            }

            fn visit_str<E: de::Error>(self, v: &str) -> Result<ExprText, E> {
                Ok(ExprText(v.to_owned()))
            }

            fn visit_seq<A: SeqAccess<'de>>(self, _: A) -> Result<ExprText, A::Error> {
                Err(de::Error::custom(
                    "expected an expression; quote it if it starts with `[`",
                ))
            }

            fn visit_map<A: MapAccess<'de>>(self, _: A) -> Result<ExprText, A::Error> {
                Err(de::Error::custom(
                    "expected an expression; quote it if it starts with `{`",
                ))
            }
        }

        d.deserialize_any(ExprVisitor)
    }
}
