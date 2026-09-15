//! What a contract offers to follow, read from its modules' ABIs (ADR 0017).

use std::collections::{BTreeMap, BTreeSet};

use nineveh_config::Named;
use nineveh_core::{Address, StructName, TypeTag};
use nineveh_decode::{FieldAbi, ModuleAbi, StructAbi};
use serde::Serialize;

/// Everything at one address that a project can follow.
#[derive(Debug, Clone, Serialize)]
pub struct Catalog {
    /// The address, in the form Aptos writes in type names.
    pub address: String,
    /// The modules read, by name.
    pub modules: Vec<String>,
    pub items: Vec<Item>,
}

/// One thing to follow: an event type, a resource, or a table held in a resource.
#[derive(Debug, Clone, Serialize)]
pub struct Item {
    pub kind: ItemKind,
    /// What the config names: `0xabc::vault::Deposit`, or `0xabc::vault::Vault.positions`
    /// for a table.
    pub id: String,
    pub module: String,
    /// The struct's name, or `Struct.field` for a table.
    pub name: String,
    /// The source and table name scaffolding gives it: the struct in snake case, with
    /// the module in front when two modules define the same name.
    pub suggested_name: String,
    /// The fields a row gets: the struct's, or a table's `key` and `value` types.
    pub fields: Vec<ItemField>,
    /// Whether the struct is generic. Its sources match every instantiation.
    pub generic: bool,
    /// Why Nineveh can't follow it yet, if it can't.
    pub unsupported: Option<String>,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Serialize)]
#[serde(rename_all = "lowercase")]
pub enum ItemKind {
    Event,
    Resource,
    Table,
}

#[derive(Debug, Clone, Serialize)]
pub struct ItemField {
    pub name: String,
    #[serde(rename = "type")]
    pub ty: String,
}

/// The catalog of `address`, from the ABIs of the modules published there.
#[must_use]
pub fn catalog(address: Address, modules: &[ModuleAbi]) -> Catalog {
    let mut modules: Vec<&ModuleAbi> = modules.iter().filter(|m| m.address == address).collect();
    modules.sort_by(|a, b| a.name.as_str().cmp(b.name.as_str()));

    let structs: BTreeMap<StructName, &StructAbi> = modules
        .iter()
        .flat_map(|m| {
            m.structs
                .iter()
                .map(|s| (StructName::new(address, m.name.clone(), s.name.clone()), s))
        })
        .collect();
    // Old-style events: whatever a struct here keeps an `EventHandle` of.
    let handle_events: BTreeSet<StructName> = structs
        .values()
        .flat_map(|s| &s.fields)
        .filter_map(|f| TypeTag::parse_declared(&f.ty).ok())
        .filter_map(|ty| {
            let tag = ty.as_struct()?;
            if !tag.name.is(Address::ONE, "event", "EventHandle") {
                return None;
            }
            let event = tag.type_args.first()?.as_struct()?;
            (event.name.address == address).then(|| event.name.clone())
        })
        .collect();

    let mut items = Vec::new();
    for (name, s) in &structs {
        let generic = !s.generic_type_params.is_empty();
        let fields = s
            .fields
            .iter()
            .map(|f| ItemField {
                name: f.name.as_str().to_owned(),
                ty: f.ty.clone(),
            })
            .collect();
        let id = name.to_string();
        let module = name.module.as_str().to_owned();
        if s.is_event || handle_events.contains(name) {
            items.push(Item {
                kind: ItemKind::Event,
                id,
                module,
                name: s.name.as_str().to_owned(),
                suggested_name: snake_case(s.name.as_str()),
                fields,
                generic,
                unsupported: log_problem(s),
            });
            continue;
        }
        if !s.abilities.iter().any(|a| a == "key") {
            continue;
        }
        items.push(Item {
            kind: ItemKind::Resource,
            id: id.clone(),
            module: module.clone(),
            name: s.name.as_str().to_owned(),
            suggested_name: snake_case(s.name.as_str()),
            fields,
            generic,
            unsupported: mirror_problem(
                s,
                if generic {
                    &["address", "type"]
                } else {
                    &["address"]
                },
            ),
        });
        items.extend(
            s.fields
                .iter()
                .filter_map(|field| table_item(name, s, field, &structs)),
        );
    }
    disambiguate(&mut items);
    items.sort_by(|a, b| (a.kind, &a.module, &a.name).cmp(&(b.kind, &b.module, &b.name)));

    Catalog {
        address: address.to_standard_string(),
        modules: modules.iter().map(|m| m.name.as_str().to_owned()).collect(),
        items,
    }
}

/// The item for `field` of the resource `name`, if it holds a table.
fn table_item(
    name: &StructName,
    s: &StructAbi,
    field: &FieldAbi,
    structs: &BTreeMap<StructName, &StructAbi>,
) -> Option<Item> {
    let table = TypeTag::parse_declared(&field.ty)
        .ok()
        .and_then(|ty| table_of(&ty))?;
    let generic = !s.generic_type_params.is_empty();
    let unsupported = if let Some(reason) = table.unsupported {
        Some(reason.to_owned())
    } else if generic {
        Some(format!(
            "`{}` is generic: name its type arguments in the config to follow this table",
            s.name.as_str()
        ))
    } else {
        table
            .value
            .as_struct()
            .and_then(|tag| structs.get(&tag.name))
            .and_then(|value| mirror_problem(value, &["handle", "key"]))
    };
    Some(Item {
        kind: ItemKind::Table,
        id: format!("{name}.{}", field.name.as_str()),
        module: name.module.as_str().to_owned(),
        name: format!("{}.{}", s.name.as_str(), field.name.as_str()),
        suggested_name: format!(
            "{}_{}",
            snake_case(s.name.as_str()),
            snake_case(field.name.as_str())
        ),
        fields: vec![
            ItemField {
                name: "key".into(),
                ty: table.key.to_string(),
            },
            ItemField {
                name: "value".into(),
                ty: table.value.to_string(),
            },
        ],
        generic,
        unsupported,
    })
}

/// A table-like field's container, with its key and value types.
struct TableField {
    key: TypeTag,
    value: TypeTag,
    unsupported: Option<&'static str>,
}

fn table_of(ty: &TypeTag) -> Option<TableField> {
    let tag = ty.as_struct()?;
    let name = &tag.name;
    let unsupported = if name.is(Address::ONE, "table", "Table")
        || name.is(Address::ONE, "smart_table", "SmartTable")
    {
        None
    } else if name.is(Address::ONE, "big_ordered_map", "BigOrderedMap") {
        Some("BigOrderedMap fields aren't supported yet")
    } else if name.is(Address::ONE, "table_with_length", "TableWithLength") {
        Some("TableWithLength fields aren't supported yet")
    } else {
        return None;
    };
    let [key, value] = tag.type_args.as_slice() else {
        return None;
    };
    Some(TableField {
        key: key.clone(),
        value: value.clone(),
        unsupported,
    })
}

/// Why a `log` table can't be built from this event struct as it stands.
fn log_problem(s: &StructAbi) -> Option<String> {
    mirror_problem(s, &["version", "event_index"])
}

/// Why a table with these key columns plus one column per field of `s` can't be
/// built: a field that isn't a valid column name, or that collides with a key column.
/// An enum is stored in one `value` column, so its fields don't matter.
fn mirror_problem(s: &StructAbi, keys: &[&str]) -> Option<String> {
    if s.is_enum {
        return None;
    }
    s.fields.iter().find_map(|f| {
        let name = f.name.as_str();
        if keys.contains(&name) {
            Some(format!(
                "field `{name}` has the same name as a key column; build this table with \
                 `reduce` instead"
            ))
        } else if !Named::is_valid(name) {
            Some(format!(
                "field `{name}` isn't a valid column name; build this table with `reduce` \
                 instead"
            ))
        } else {
            None
        }
    })
}

/// Put the module in front of names two items share, and number what still clashes.
fn disambiguate(items: &mut [Item]) {
    let mut counts: BTreeMap<String, usize> = BTreeMap::new();
    for item in items.iter() {
        *counts.entry(item.suggested_name.clone()).or_default() += 1;
    }
    for item in items.iter_mut() {
        if counts.get(&item.suggested_name).is_some_and(|n| *n > 1) {
            item.suggested_name = format!("{}_{}", snake_case(&item.module), item.suggested_name);
        }
    }
    let mut taken = BTreeSet::new();
    for item in items.iter_mut() {
        let base = fit(&item.suggested_name, 0);
        let mut name = base.clone();
        let mut n = 1;
        while !taken.insert(name.clone()) {
            n += 1;
            name = fit(&base, n);
        }
        item.suggested_name = name;
    }
}

/// `name`, cut to fit a name's 63 bytes, with `_n` at the end when `n > 0`.
fn fit(name: &str, n: usize) -> String {
    const MAX: usize = 63;
    let suffix = if n > 0 {
        format!("_{n}")
    } else {
        String::new()
    };
    let keep = MAX.saturating_sub(suffix.len());
    let mut cut = name.get(..keep.min(name.len())).unwrap_or(name).to_owned();
    while cut.ends_with('_') {
        cut.pop();
    }
    cut + &suffix
}

/// `CreateContractEvent` → `create_contract_event`, `NFTMinted` → `nft_minted`. The
/// result is a valid name for any Move identifier that starts with a letter.
#[must_use]
pub fn snake_case(s: &str) -> String {
    let chars: Vec<char> = s.chars().collect();
    let mut out = String::with_capacity(s.len() + 4);
    for (i, &c) in chars.iter().enumerate() {
        if c.is_ascii_uppercase() {
            let prev = i.checked_sub(1).and_then(|p| chars.get(p));
            let next = chars.get(i + 1);
            let boundary = prev.is_some_and(|p| p.is_ascii_lowercase() || p.is_ascii_digit())
                || (prev.is_some_and(char::is_ascii_uppercase)
                    && next.is_some_and(char::is_ascii_lowercase));
            if boundary && !out.ends_with('_') {
                out.push('_');
            }
            out.push(c.to_ascii_lowercase());
        } else if c.is_ascii_alphanumeric() {
            out.push(c);
        } else if !out.is_empty() && !out.ends_with('_') {
            out.push('_');
        }
    }
    let trimmed = out.trim_matches('_');
    if trimmed.starts_with(|c: char| c.is_ascii_lowercase()) {
        trimmed.to_owned()
    } else {
        format!("s_{trimmed}")
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn snake_cases_move_names() {
        for (from, to) in [
            ("CreateContractEvent", "create_contract_event"),
            ("NFTMinted", "nft_minted"),
            ("V2Pool", "v2_pool"),
            ("Pool2", "pool2"),
            ("positions", "positions"),
            ("already_snake", "already_snake"),
            ("_private", "private"),
            ("__", "s_"),
        ] {
            assert_eq!(snake_case(from), to, "{from}");
        }
    }

    #[test]
    fn fits_names_in_63_bytes() {
        let long = "a".repeat(80);
        assert_eq!(fit(&long, 0).len(), 63);
        assert!(fit(&long, 12).ends_with("_12"));
        assert_eq!(fit(&long, 12).len(), 63);
        assert_eq!(fit("short", 2), "short_2");
    }
}
