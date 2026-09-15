//! A config from picks in a contract's catalog: a `log` table per event, a `mirror`
//! table per resource or table (ADR 0017).

use std::collections::BTreeSet;
use std::fmt::Write as _;

use nineveh_config::Named;
use nineveh_core::Network;
use serde::Deserialize;

use crate::catalog::{Catalog, Item, ItemKind};

/// What to scaffold: a project's name and network, where it starts, and the catalog
/// items it follows.
#[derive(Debug, Clone, Deserialize)]
pub struct Draft {
    pub name: String,
    pub network: Network,
    #[serde(default)]
    pub start: Start,
    /// Catalog [`Item::id`]s.
    pub picks: Vec<String>,
}

/// Where a scaffolded project starts.
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq, Deserialize)]
#[serde(rename_all = "lowercase", tag = "at", content = "version")]
pub enum Start {
    /// `start_version: auto`: the contract's whole history.
    #[default]
    Auto,
    /// A version: the chain's current one, for a live backend without a backfill.
    Version(u64),
}

#[derive(Debug, Clone, PartialEq, Eq, thiserror::Error)]
#[non_exhaustive]
pub enum ScaffoldError {
    #[error(
        "`{0}` isn't a valid project name: use a–z, 0–9 and `_`, starting with a letter, \
         at most 63 characters"
    )]
    Name(String),
    #[error("pick at least one thing to follow")]
    NoPicks,
    #[error("`{0}` isn't in the contract's catalog")]
    Unknown(String),
    #[error("`{id}` can't be followed yet: {reason}")]
    Unsupported { id: String, reason: String },
}

impl ScaffoldError {
    /// Scaffolding reads only its inputs, so trying again can't help.
    #[must_use]
    pub fn is_retryable(&self) -> bool {
        false
    }
}

/// The `nineveh.yaml` for `draft`, with its picks looked up in `catalogs`.
///
/// # Errors
///
/// If the name isn't valid, nothing is picked, or a pick isn't in a catalog or can't be
/// followed.
pub fn scaffold(draft: &Draft, catalogs: &[Catalog]) -> Result<String, ScaffoldError> {
    if !Named::is_valid(&draft.name) {
        return Err(ScaffoldError::Name(draft.name.clone()));
    }
    let mut seen = BTreeSet::new();
    let mut picked: Vec<&Item> = Vec::new();
    for id in &draft.picks {
        if !seen.insert(id.as_str()) {
            continue;
        }
        let item = catalogs
            .iter()
            .flat_map(|c| &c.items)
            .find(|item| item.id == *id)
            .ok_or_else(|| ScaffoldError::Unknown(id.clone()))?;
        if let Some(reason) = &item.unsupported {
            return Err(ScaffoldError::Unsupported {
                id: id.clone(),
                reason: reason.clone(),
            });
        }
        picked.push(item);
    }
    if picked.is_empty() {
        return Err(ScaffoldError::NoPicks);
    }
    // Suggested names are unique within a catalog; number any clash across two.
    let mut taken = BTreeSet::new();
    let names: Vec<String> = picked
        .iter()
        .map(|item| {
            let mut name = item.suggested_name.clone();
            let mut n = 1;
            while !taken.insert(name.clone()) {
                n += 1;
                name = format!("{}_{n}", item.suggested_name);
            }
            name
        })
        .collect();

    let mut yaml = String::new();
    let addresses: BTreeSet<&str> = catalogs
        .iter()
        .filter(|c| c.items.iter().any(|i| picked.iter().any(|p| p.id == i.id)))
        .map(|c| c.address.as_str())
        .collect();
    let _ = writeln!(
        yaml,
        "# Scaffolded by Nineveh from {}. Edit freely: see docs/config.md.",
        addresses.into_iter().collect::<Vec<_>>().join(", ")
    );
    let _ = writeln!(yaml, "name: {}", draft.name);
    let _ = writeln!(yaml, "network: {}", draft.network);
    match draft.start {
        Start::Auto => yaml.push_str("start_version: auto\n"),
        Start::Version(v) => {
            let _ = writeln!(yaml, "start_version: {v}");
        }
    }
    yaml.push_str("\nsources:\n");
    for (item, name) in picked.iter().zip(&names) {
        let keyword = match item.kind {
            ItemKind::Event => "event",
            ItemKind::Resource => "resource",
            ItemKind::Table => "table",
        };
        // Quoted: generic arguments hold commas, which YAML's flow syntax would split.
        let _ = writeln!(yaml, "  {name}:\n    {keyword}: \"{}\"", item.id);
    }
    yaml.push_str("\nstate:\n");
    for (item, name) in picked.iter().zip(&names) {
        let how = match item.kind {
            ItemKind::Event => "log",
            ItemKind::Resource | ItemKind::Table => "mirror",
        };
        let _ = writeln!(yaml, "  {name}:\n    {how}: {name}");
    }
    Ok(yaml)
}
