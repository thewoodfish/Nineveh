//! `nineveh.lock`: the struct layouts a project decodes against, pinned (ADR 0010).

use std::collections::{BTreeMap, BTreeSet};

use nineveh_core::{Identifier, Network, StructName, TypeTag};
use serde::{Deserialize, Serialize};

use crate::layout::{Body, Builtin, Field, StructLayout, Variant, visit_struct_names};

/// The lock format this build reads and writes.
///
/// Format 2 added `resource` (ADR 0012).
pub const FORMAT: u32 = 2;

/// The contents of `nineveh.lock`: every struct layout a project can meet, keyed by
/// struct name.
///
/// `nineveh init` writes it from module ABIs; after that, decoding never touches the
/// network. A lock is always *closed*: every struct a layout refers to has a layout
/// of its own (or is `String` or `Option`, which the decoder handles natively), so
/// decoding can't run into a type it doesn't know.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Lockfile {
    network: Network,
    structs: BTreeMap<StructName, StructLayout>,
}

impl Lockfile {
    /// Build a lock from layouts, checking that it's well-formed and closed.
    ///
    /// # Errors
    ///
    /// If a layout is malformed or refers to a struct with no layout.
    pub fn new(
        network: Network,
        structs: BTreeMap<StructName, StructLayout>,
    ) -> Result<Self, LockError> {
        let lock = Self { network, structs };
        lock.validate()?;
        Ok(lock)
    }

    #[must_use]
    pub fn network(&self) -> Network {
        self.network
    }

    #[must_use]
    pub fn get(&self, name: &StructName) -> Option<&StructLayout> {
        self.structs.get(name)
    }

    pub fn structs(&self) -> impl Iterator<Item = (&StructName, &StructLayout)> {
        self.structs.iter()
    }

    /// Parse a lock from its JSON text.
    ///
    /// # Errors
    ///
    /// If the text isn't a lock this build understands, or the lock is malformed or
    /// not closed.
    pub fn from_json(text: &str) -> Result<Self, LockError> {
        let repr: LockRepr = serde_json::from_str(text).map_err(LockError::Json)?;
        if repr.format != FORMAT {
            return Err(LockError::UnsupportedFormat(repr.format));
        }
        let structs = repr
            .structs
            .into_iter()
            .map(|(name, s)| {
                let layout = s.into_layout().map_err(|reason| LockError::Invalid {
                    name: name.clone(),
                    reason,
                })?;
                Ok((name, layout))
            })
            .collect::<Result<_, LockError>>()?;
        Self::new(repr.network, structs)
    }

    /// The lock as pretty-printed JSON with a trailing newline.
    ///
    /// Output is deterministic (structs sorted by name, fields in declaration order),
    /// so regenerating an unchanged lock produces no diff.
    ///
    /// # Errors
    ///
    /// Only if serialization itself fails, which plain data like this never does.
    pub fn to_json(&self) -> Result<String, LockError> {
        let repr = LockRepr {
            format: FORMAT,
            network: self.network,
            structs: self
                .structs
                .iter()
                .map(|(name, layout)| (name.clone(), StructRepr::from_layout(layout)))
                .collect(),
        };
        let mut text = serde_json::to_string_pretty(&repr).map_err(LockError::Json)?;
        text.push('\n');
        Ok(text)
    }

    fn validate(&self) -> Result<(), LockError> {
        for (name, layout) in &self.structs {
            let invalid = |reason: String| LockError::Invalid {
                name: name.clone(),
                reason,
            };
            if Builtin::of(name).is_some() {
                return Err(invalid("builtin types are decoded natively".into()));
            }
            check_unique_names(layout).map_err(invalid)?;

            let mut problem = None;
            for ty in layout.field_types() {
                check_params(ty, layout.type_params).map_err(invalid)?;
                visit_struct_names(ty, &mut |tag| {
                    if problem.is_some() || Builtin::of(&tag.name).is_some() {
                        return;
                    }
                    problem = match self.structs.get(&tag.name) {
                        None => Some(format!("refers to `{}`, which has no layout", tag.name)),
                        Some(target) if usize::from(target.type_params) != tag.type_args.len() => {
                            Some(format!(
                                "gives `{}` {} type arguments, but it declares {}",
                                tag.name,
                                tag.type_args.len(),
                                target.type_params
                            ))
                        }
                        Some(_) => None,
                    };
                });
            }
            if let Some(reason) = problem {
                return Err(invalid(reason));
            }
        }
        Ok(())
    }
}

fn check_unique_names(layout: &StructLayout) -> Result<(), String> {
    fn unique<'a>(names: impl Iterator<Item = &'a Identifier>, what: &str) -> Result<(), String> {
        let mut seen = BTreeSet::new();
        for name in names {
            if !seen.insert(name) {
                return Err(format!("declares {what} `{name}` twice"));
            }
        }
        Ok(())
    }
    match &layout.body {
        Body::Struct(fields) => unique(fields.iter().map(|f| &f.name), "field"),
        Body::Enum(variants) => {
            unique(variants.iter().map(|v| &v.name), "variant")?;
            for variant in variants {
                unique(variant.fields.iter().map(|f| &f.name), "field")?;
            }
            Ok(())
        }
    }
}

fn check_params(ty: &TypeTag, arity: u16) -> Result<(), String> {
    match ty {
        TypeTag::Param(index) if *index >= arity => Err(format!(
            "a field refers to type parameter T{index}, but the struct declares {arity}"
        )),
        TypeTag::Vector(inner) => check_params(inner, arity),
        TypeTag::Struct(tag) => tag
            .type_args
            .iter()
            .try_for_each(|a| check_params(a, arity)),
        _ => Ok(()),
    }
}

/// Why a lock couldn't be read.
#[derive(Debug, thiserror::Error)]
#[non_exhaustive]
pub enum LockError {
    #[error("nineveh.lock isn't valid JSON for this lock format")]
    Json(#[source] serde_json::Error),

    #[error(
        "nineveh.lock uses format {0}, but this build reads format {FORMAT}; \
         regenerate it with `nineveh init`"
    )]
    UnsupportedFormat(u32),

    #[error("nineveh.lock: layout of `{name}` {reason}")]
    Invalid { name: StructName, reason: String },
}

// --- JSON representation ---------------------------------------------------------

#[derive(Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
struct LockRepr {
    format: u32,
    network: Network,
    structs: BTreeMap<StructName, StructRepr>,
}

#[derive(Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
struct StructRepr {
    #[serde(default, skip_serializing_if = "is_zero")]
    type_params: u16,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    fields: Option<Vec<FieldRepr>>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    variants: Option<Vec<VariantRepr>>,
    #[serde(default, skip_serializing_if = "is_false")]
    event: bool,
    #[serde(default, skip_serializing_if = "is_false")]
    resource: bool,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    group: Option<StructName>,
}

#[derive(Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
struct FieldRepr {
    name: Identifier,
    #[serde(rename = "type", with = "declared_type")]
    ty: TypeTag,
}

#[derive(Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
struct VariantRepr {
    name: Identifier,
    fields: Vec<FieldRepr>,
}

impl StructRepr {
    fn from_layout(layout: &StructLayout) -> Self {
        let fields_repr = |fields: &[Field]| {
            fields
                .iter()
                .map(|f| FieldRepr {
                    name: f.name.clone(),
                    ty: f.ty.clone(),
                })
                .collect()
        };
        let (fields, variants) = match &layout.body {
            Body::Struct(fields) => (Some(fields_repr(fields)), None),
            Body::Enum(variants) => (
                None,
                Some(
                    variants
                        .iter()
                        .map(|v| VariantRepr {
                            name: v.name.clone(),
                            fields: fields_repr(&v.fields),
                        })
                        .collect(),
                ),
            ),
        };
        Self {
            type_params: layout.type_params,
            fields,
            variants,
            event: layout.is_event,
            resource: layout.is_resource,
            group: layout.group.clone(),
        }
    }

    fn into_layout(self) -> Result<StructLayout, String> {
        let fields = |repr: Vec<FieldRepr>| {
            repr.into_iter()
                .map(|f| Field {
                    name: f.name,
                    ty: f.ty,
                })
                .collect()
        };
        let body = match (self.fields, self.variants) {
            (Some(f), None) => Body::Struct(fields(f)),
            (None, Some(v)) => Body::Enum(
                v.into_iter()
                    .map(|v| Variant {
                        name: v.name,
                        fields: fields(v.fields),
                    })
                    .collect(),
            ),
            _ => return Err("must have exactly one of `fields` and `variants`".into()),
        };
        Ok(StructLayout {
            type_params: self.type_params,
            body,
            is_event: self.event,
            is_resource: self.resource,
            group: self.group,
        })
    }
}

#[allow(
    clippy::trivially_copy_pass_by_ref,
    reason = "serde's skip_serializing_if signature"
)]
fn is_zero(n: &u16) -> bool {
    *n == 0
}

#[allow(
    clippy::trivially_copy_pass_by_ref,
    reason = "serde's skip_serializing_if signature"
)]
fn is_false(b: &bool) -> bool {
    !*b
}

/// Field types may name the struct's generic parameters, so they're parsed with
/// [`TypeTag::parse_declared`].
mod declared_type {
    use nineveh_core::TypeTag;
    use serde::{Deserialize, Deserializer, Serializer};

    pub(super) fn serialize<S: Serializer>(ty: &TypeTag, s: S) -> Result<S::Ok, S::Error> {
        s.collect_str(ty)
    }

    pub(super) fn deserialize<'de, D: Deserializer<'de>>(d: D) -> Result<TypeTag, D::Error> {
        let s = <std::borrow::Cow<'de, str>>::deserialize(d)?;
        TypeTag::parse_declared(&s).map_err(serde::de::Error::custom)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    const LOCK: &str = r#"{
  "format": 2,
  "network": "testnet",
  "structs": {
    "0x1::coin::Coin": {
      "type_params": 1,
      "fields": [
        {
          "name": "value",
          "type": "u64"
        }
      ]
    },
    "0x1::coin::CoinStore": {
      "type_params": 1,
      "fields": [
        {
          "name": "coin",
          "type": "0x1::coin::Coin<T0>"
        },
        {
          "name": "frozen",
          "type": "bool"
        }
      ],
      "resource": true
    },
    "0x0000000000000000000000000000000000000000000000000000000000000abc::vault::Position": {
      "variants": [
        {
          "name": "V1",
          "fields": [
            {
              "name": "memo",
              "type": "0x1::option::Option<0x1::string::String>"
            }
          ]
        }
      ],
      "group": "0x1::object::ObjectGroup"
    }
  }
}
"#;

    #[test]
    fn round_trips_byte_for_byte() {
        let lock = Lockfile::from_json(LOCK).unwrap();
        assert_eq!(lock.to_json().unwrap(), LOCK);
    }

    #[test]
    fn rejects_an_open_lock() {
        let open = LOCK.replace("\"0x1::coin::Coin\": {", "\"0x1::coin::Other\": {");
        let err = Lockfile::from_json(&open).unwrap_err().to_string();
        assert!(
            err.contains("refers to `0x1::coin::Coin`, which has no layout"),
            "{err}"
        );
    }

    #[test]
    fn rejects_out_of_range_params_and_wrong_arity() {
        let param = LOCK.replace("\"0x1::coin::Coin<T0>\"", "\"0x1::coin::Coin<T1>\"");
        assert!(
            Lockfile::from_json(&param)
                .unwrap_err()
                .to_string()
                .contains("T1")
        );

        let arity = LOCK.replace("\"0x1::coin::Coin<T0>\"", "\"0x1::coin::Coin<T0, u8>\"");
        assert!(
            Lockfile::from_json(&arity)
                .unwrap_err()
                .to_string()
                .contains("2 type arguments")
        );
    }

    #[test]
    fn rejects_unknown_formats_and_fields() {
        let future = LOCK.replace("\"format\": 2", "\"format\": 3");
        assert!(matches!(
            Lockfile::from_json(&future),
            Err(LockError::UnsupportedFormat(3))
        ));
        let extra = LOCK.replace("\"type_params\": 1,", "\"type_params\": 1, \"x\": 0,");
        assert!(matches!(
            Lockfile::from_json(&extra),
            Err(LockError::Json(_))
        ));
    }
}
