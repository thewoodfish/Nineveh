//! Building a [`Lockfile`] from module ABIs.
//!
//! `nineveh init` fetches ABIs from a fullnode and hands them to a [`LockBuilder`].
//! The builder is pure: it reports the modules it still needs
//! ([`BuildError::MissingModules`]), the caller fetches them, and the build is retried
//! until the lock is closed.

use std::collections::{BTreeMap, BTreeSet, HashMap, VecDeque};
use std::fmt;

use nineveh_core::{Address, Identifier, Network, StructName, TypeTag};
use serde::Deserialize;

use crate::layout::{Body, Builtin, Field, StructLayout, Variant, visit_struct_names};
use crate::lock::{LockError, Lockfile};

/// A module's ABI, as the fullnode REST API returns it in the `abi` field of
/// `GET /v1/accounts/{address}/module/{name}`.
///
/// Only struct declarations are read. Unknown fields are ignored, so ABIs from newer
/// fullnodes still load.
#[derive(Debug, Clone, Deserialize)]
pub struct ModuleAbi {
    pub address: Address,
    pub name: Identifier,
    pub structs: Vec<StructAbi>,
}

#[derive(Debug, Clone, Deserialize)]
pub struct StructAbi {
    pub name: Identifier,
    #[serde(default)]
    pub is_native: bool,
    #[serde(default)]
    pub is_event: bool,
    #[serde(default)]
    pub is_enum: bool,
    #[serde(default)]
    pub abilities: Vec<String>,
    pub generic_type_params: Vec<serde::de::IgnoredAny>,
    pub fields: Vec<FieldAbi>,
    #[serde(default)]
    pub variants: Vec<VariantAbi>,
}

#[derive(Debug, Clone, Deserialize)]
pub struct FieldAbi {
    pub name: Identifier,
    #[serde(rename = "type")]
    pub ty: String,
}

#[derive(Debug, Clone, Deserialize)]
pub struct VariantAbi {
    pub name: Identifier,
    pub fields: Vec<FieldAbi>,
}

/// A module's address and name: `0x1::coin`.
#[derive(Debug, Clone, PartialEq, Eq, PartialOrd, Ord, Hash)]
pub struct ModuleId {
    pub address: Address,
    pub name: Identifier,
}

impl fmt::Display for ModuleId {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(f, "{}::{}", self.address.to_standard_string(), self.name)
    }
}

/// Collects module ABIs and builds the closed lock for a set of root structs.
#[derive(Debug)]
pub struct LockBuilder {
    network: Network,
    modules: HashMap<ModuleId, ModuleAbi>,
    groups: HashMap<StructName, StructName>,
}

impl LockBuilder {
    #[must_use]
    pub fn new(network: Network) -> Self {
        Self {
            network,
            modules: HashMap::new(),
            groups: HashMap::new(),
        }
    }

    /// Add a module's ABI, replacing any earlier copy of the same module.
    pub fn add_module(&mut self, abi: ModuleAbi) {
        let id = ModuleId {
            address: abi.address,
            name: abi.name.clone(),
        };
        self.modules.insert(id, abi);
    }

    /// Record that `member` belongs to the resource group `group`.
    ///
    /// REST ABIs don't carry `#[resource_group_member]`; it lives in the module's
    /// metadata, which the caller reads.
    pub fn set_group(&mut self, member: StructName, group: StructName) {
        self.groups.insert(member, group);
    }

    /// Build the lock holding `roots` and every struct they reach.
    ///
    /// # Errors
    ///
    /// [`BuildError::MissingModules`] lists every module still needed; add them and
    /// build again. Other errors mean the ABIs can't produce a valid lock.
    pub fn build<'a>(
        &self,
        roots: impl IntoIterator<Item = &'a StructName>,
    ) -> Result<Lockfile, BuildError> {
        let mut queue: VecDeque<StructName> = roots.into_iter().cloned().collect();
        let mut seen = BTreeSet::new();
        let mut structs = BTreeMap::new();
        let mut missing = BTreeSet::new();

        while let Some(name) = queue.pop_front() {
            if Builtin::of(&name).is_some() || !seen.insert(name.clone()) {
                continue;
            }
            let module_id = ModuleId {
                address: name.address,
                name: name.module.clone(),
            };
            let Some(module) = self.modules.get(&module_id) else {
                missing.insert(module_id);
                continue;
            };
            let abi = module
                .structs
                .iter()
                .find(|s| s.name == name.name)
                .ok_or_else(|| BuildError::NoSuchStruct(name.clone()))?;
            let layout = self.layout(&name, abi)?;
            for ty in layout.field_types() {
                visit_struct_names(ty, &mut |tag| queue.push_back(tag.name.clone()));
            }
            structs.insert(name, layout);
        }

        if !missing.is_empty() {
            return Err(BuildError::MissingModules(missing.into_iter().collect()));
        }
        Lockfile::new(self.network, structs).map_err(BuildError::Lock)
    }

    fn layout(&self, name: &StructName, abi: &StructAbi) -> Result<StructLayout, BuildError> {
        if abi.is_native {
            return Err(BuildError::Native(name.clone()));
        }
        let fields = |fields: &[FieldAbi]| -> Result<Vec<Field>, BuildError> {
            fields
                .iter()
                .map(|f| {
                    let ty = TypeTag::parse_declared(&f.ty).map_err(|e| BuildError::BadType {
                        name: name.clone(),
                        field: f.name.clone(),
                        reason: e.to_string(),
                    })?;
                    Ok(Field {
                        name: f.name.clone(),
                        ty,
                    })
                })
                .collect()
        };
        let body = if abi.is_enum {
            Body::Enum(
                abi.variants
                    .iter()
                    .map(|v| {
                        Ok(Variant {
                            name: v.name.clone(),
                            fields: fields(&v.fields)?,
                        })
                    })
                    .collect::<Result<_, BuildError>>()?,
            )
        } else {
            Body::Struct(fields(&abi.fields)?)
        };
        let type_params = u16::try_from(abi.generic_type_params.len())
            .map_err(|_| BuildError::TooManyParams(name.clone()))?;
        Ok(StructLayout {
            type_params,
            body,
            is_event: abi.is_event,
            is_resource: abi.abilities.iter().any(|a| a == "key"),
            group: self.groups.get(name).cloned(),
        })
    }
}

/// Why a lock couldn't be built from the ABIs at hand.
#[derive(Debug, thiserror::Error)]
#[non_exhaustive]
pub enum BuildError {
    #[error("fetch these modules and build again: {}", list(.0))]
    MissingModules(Vec<ModuleId>),

    #[error("`{0}` isn't declared in its module's ABI")]
    NoSuchStruct(StructName),

    #[error("`{0}` is a native struct, which has no field layout to decode against")]
    Native(StructName),

    #[error("`{name}` field `{field}` has a type Nineveh can't parse: {reason}")]
    BadType {
        name: StructName,
        field: Identifier,
        reason: String,
    },

    #[error("`{0}` declares more than 65535 type parameters")]
    TooManyParams(StructName),

    #[error(transparent)]
    Lock(LockError),
}

fn list(ids: &[ModuleId]) -> String {
    ids.iter()
        .map(ToString::to_string)
        .collect::<Vec<_>>()
        .join(", ")
}

#[cfg(test)]
mod tests {
    use super::*;

    fn module(json: &str) -> ModuleAbi {
        serde_json::from_str(json).unwrap()
    }

    fn name(s: &str) -> StructName {
        s.parse().unwrap()
    }

    const COIN: &str = r#"{
        "address": "0x1", "name": "coin", "friends": [], "exposed_functions": [],
        "structs": [
            {"name": "Coin", "is_native": false, "is_event": false, "abilities": ["store"],
             "generic_type_params": [{"constraints": []}],
             "fields": [{"name": "value", "type": "u64"}]},
            {"name": "CoinStore", "is_native": false, "is_event": false, "abilities": ["key"],
             "generic_type_params": [{"constraints": []}],
             "fields": [{"name": "coin", "type": "0x1::coin::Coin<T0>"},
                        {"name": "frozen", "type": "bool"},
                        {"name": "memo", "type": "0x1::option::Option<0x1::string::String>"}]}
        ]}"#;

    #[test]
    fn builds_the_closure_and_reports_missing_modules_all_at_once() {
        let mut builder = LockBuilder::new(Network::Testnet);
        builder.add_module(module(COIN));
        let root = name("0x1::coin::CoinStore");

        let lock = builder.build([&root]).unwrap();
        let names: Vec<String> = lock.structs().map(|(n, _)| n.to_string()).collect();
        // Builtins (Option, String) are decoded natively and stay out of the lock.
        assert_eq!(names, ["0x1::coin::Coin", "0x1::coin::CoinStore"]);

        let other = name("0xabc::vault::Vault");
        let also = name("0x1::account::Account");
        let err = builder.build([&root, &other, &also]).unwrap_err();
        assert_eq!(
            err.to_string(),
            "fetch these modules and build again: 0x1::account, \
             0x0000000000000000000000000000000000000000000000000000000000000abc::vault"
        );
    }

    #[test]
    fn missing_structs_and_natives_are_errors() {
        let mut builder = LockBuilder::new(Network::Testnet);
        builder.add_module(module(COIN));
        assert!(matches!(
            builder.build([&name("0x1::coin::Nope")]),
            Err(BuildError::NoSuchStruct(_))
        ));

        builder.add_module(module(&COIN.replace(
            r#""is_native": false, "is_event": false, "abilities": ["store"]"#,
            r#""is_native": true, "is_event": false, "abilities": ["store"]"#,
        )));
        assert!(matches!(
            builder.build([&name("0x1::coin::Coin")]),
            Err(BuildError::Native(_))
        ));
    }
}
