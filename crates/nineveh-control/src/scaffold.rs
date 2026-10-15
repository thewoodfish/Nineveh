//! A config from picks in a contract's catalog: a `log` table per event, a `mirror`
//! table per resource or table (ADR 0017).
//!
//! [`scaffold`] writes the whole `nineveh.yaml` for a project that doesn't exist yet.
//! [`add_sources`] puts more picks into one that does, by splicing entries into the
//! text rather than re-emitting it. That is the point of it: the config is the
//! author's file, with their comments and their own reduce tables in it, and a round
//! trip through a YAML writer would hand it back reformatted — which is not what
//! "add a source" should mean.

use std::collections::BTreeSet;
use std::fmt::Write as _;

use nineveh_config::{Config, Named, SourceKind};
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
    #[error("`{id}` is already followed, by the source `{existing}`")]
    AlreadyFollowed { id: String, existing: String },
    #[error(
        "this config's `{0}:` isn't a block of entries, so Nineveh can't add one to it \
         without rewriting the file: add the source to `nineveh.yaml` by hand"
    )]
    Unextendable(String),
}

impl ScaffoldError {
    /// Scaffolding reads only its inputs, so trying again can't help.
    #[must_use]
    pub fn is_retryable(&self) -> bool {
        false
    }
}

/// `event`, `resource` or `table`: the key a source is written with.
fn keyword(kind: ItemKind) -> &'static str {
    match kind {
        ItemKind::Event => "event",
        ItemKind::Resource => "resource",
        ItemKind::Table => "table",
    }
}

/// `log` or `mirror`: the raw table a source of this kind brings with it.
fn how(kind: ItemKind) -> &'static str {
    match kind {
        ItemKind::Event => "log",
        ItemKind::Resource | ItemKind::Table => "mirror",
    }
}

/// The catalog [`Item::id`] a source follows, for comparing one against the other.
///
/// Type arguments are dropped, because a catalog pick has none: a generic struct is
/// listed once and its source matches every instantiation. So a project following
/// `Pool<AptosCoin>` already follows what the pick `…::Pool` would add.
fn followed(kind: &SourceKind) -> String {
    match kind {
        SourceKind::Event(tag) | SourceKind::Resource(tag) => tag.name.to_string(),
        SourceKind::Table { parent, field } => format!("{}.{}", parent.name, field.as_str()),
    }
}

/// The `picks` as catalog items, in the order they were asked for, each one once.
///
/// # Errors
///
/// If nothing is picked, or a pick isn't in a catalog or can't be followed.
fn resolve<'a>(picks: &[String], catalogs: &'a [Catalog]) -> Result<Vec<&'a Item>, ScaffoldError> {
    let mut seen = BTreeSet::new();
    let mut picked: Vec<&Item> = Vec::new();
    for id in picks {
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
    Ok(picked)
}

/// A name for each item, unique among themselves and against everything in `taken`,
/// which it adds them to.
///
/// Suggested names are unique within a catalog, so a clash is either two contracts
/// defining the same struct or a name the project already uses.
fn named(picked: &[&Item], taken: &mut BTreeSet<String>) -> Vec<String> {
    picked
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
        .collect()
}

/// How a block writes its entries, so a new one is written the way its neighbours are.
///
/// Both are the same mapping to YAML, and the difference is purely how it looks in the
/// file — which is the reason to carry it: an entry added underneath three one-liners
/// shouldn't be the one that isn't.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum Style {
    /// The key on the line below the name, as [`scaffold`] writes a new config.
    Block,
    /// `name: { event: "…" }`, as `examples/03-market` writes one.
    Flow,
}

/// One entry of a `sources:` or `state:` block, as a line or two ending in a newline.
fn entry(yaml: &mut String, style: Style, indent: &str, name: &str, key: &str, value: &str) {
    let _ = match style {
        Style::Flow => writeln!(yaml, "{indent}{name}: {{ {key}: {value} }}"),
        Style::Block => writeln!(yaml, "{indent}{name}:\n{indent}{indent}{key}: {value}"),
    };
}

/// The source entry for `item`: `event`, `resource` or `table`, and the type it names.
fn source_entry(yaml: &mut String, style: Style, indent: &str, name: &str, item: &Item) {
    // Quoted: generic arguments hold commas, which YAML's flow syntax would split.
    let value = format!("\"{}\"", item.id);
    entry(yaml, style, indent, name, keyword(item.kind), &value);
}

/// The raw table a source brings with it: a `log` of an event, a `mirror` of a value.
fn table_entry(yaml: &mut String, style: Style, indent: &str, name: &str, item: &Item) {
    entry(yaml, style, indent, name, how(item.kind), name);
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
    let picked = resolve(&draft.picks, catalogs)?;
    let names = named(&picked, &mut BTreeSet::new());

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
        source_entry(&mut yaml, Style::Block, "  ", name, item);
    }
    yaml.push_str("\nstate:\n");
    for (item, name) in picked.iter().zip(&names) {
        table_entry(&mut yaml, Style::Block, "  ", name, item);
    }
    Ok(yaml)
}

/// Where a top-level block's entries end, and how they're written.
#[derive(Debug, PartialEq, Eq)]
struct Block {
    /// Byte offset to insert at: just past the block's last entry line.
    at: usize,
    /// The indentation its entries are written with, so a new one lines up with them.
    indent: String,
    style: Style,
}

/// The top-level block `key` in `text`: where to add an entry to it.
///
/// Line-based on purpose. The alternative is to parse, insert and re-emit, which
/// reformats the whole file and drops every comment in it — so this finds the place
/// instead and leaves the rest of the bytes alone. It's safe to be this crude because
/// the text has already parsed as a config by the time it gets here: the only question
/// left is which lines the block occupies.
///
/// `Ok(None)` means there is no such key. `state:` is optional, and a project whose
/// tables all come from a `reducers:` file (ADR 0025) has none.
///
/// # Errors
///
/// If the key is written as a flow mapping (`sources: {a: {event: …}}`), which has no
/// line to put a line after.
fn block(text: &str, key: &str) -> Result<Option<Block>, ScaffoldError> {
    let mut at = 0;
    let mut found = false;
    let mut end = 0;
    let mut indent: Option<String> = None;
    let mut entries = 0usize;
    let mut flow = 0usize;
    for line in text.split_inclusive('\n') {
        let next = at + line.len();
        let content = line.trim_end_matches(['\n', '\r']);
        if !found {
            if let Some(rest) = content
                .strip_prefix(key)
                .and_then(|rest| rest.strip_prefix(':'))
            {
                let rest = rest.trim();
                if !rest.is_empty() && !rest.starts_with('#') {
                    return Err(ScaffoldError::Unextendable(key.to_owned()));
                }
                found = true;
                end = next;
            }
            at = next;
            continue;
        }
        // The block runs to the next line that starts in column 0: the next top-level
        // key, or the end of the document.
        let blank = content.trim().is_empty();
        if !blank && !content.starts_with([' ', '\t']) {
            break;
        }
        if !blank {
            end = next;
            let trimmed = content.trim_start();
            let width = content.len() - trimmed.len();
            if !trimmed.starts_with('#') {
                if indent.is_none() {
                    indent = Some(content[..width].to_owned());
                }
                // Only the block's own entries vote on the style. A nested line is
                // deeper, and belongs to the entry above it.
                if indent.as_ref().is_some_and(|i| i.len() == width) {
                    entries += 1;
                    let one_liner = trimmed.split_once(':').is_some_and(|(_, rest)| {
                        rest.trim_start().starts_with('{') && trimmed.ends_with('}')
                    });
                    flow += usize::from(one_liner);
                }
            }
        }
        at = next;
    }
    Ok(found.then(|| Block {
        at: end,
        indent: indent.unwrap_or_else(|| "  ".to_owned()),
        // Unanimous, or not at all: a block that mixes the two gets the plain form,
        // which is the one a reader of `docs/config.md` has seen.
        style: if entries > 0 && flow == entries {
            Style::Flow
        } else {
            Style::Block
        },
    }))
}

/// `text` with every pick in `picks` added to it: a source each, and the raw table
/// each source brings.
///
/// `config` is `text` parsed — composed with the project's reducers, so a name a
/// `reducers:` file declares counts as taken too (ADR 0025). The new names are made
/// unique against it, which is what lets a second contract's `Deposit` be added beside
/// the first as `deposit_2` instead of as a config that no longer parses.
///
/// # Errors
///
/// If nothing is picked, a pick isn't in a catalog, can't be followed or is already
/// followed, or the config's blocks aren't written in a form entries can be added to.
pub fn add_sources(
    text: &str,
    config: &Config,
    picks: &[String],
    catalogs: &[Catalog],
) -> Result<String, ScaffoldError> {
    let picked = resolve(picks, catalogs)?;
    for item in &picked {
        let already = config.sources.iter().find(|source| {
            source.kind.keyword() == keyword(item.kind) && followed(&source.kind) == item.id
        });
        if let Some(source) = already {
            return Err(ScaffoldError::AlreadyFollowed {
                id: item.id.clone(),
                existing: source.name.name.clone(),
            });
        }
    }
    let mut taken: BTreeSet<String> = config
        .sources
        .iter()
        .map(|s| s.name.name.clone())
        .chain(config.state.iter().map(|t| t.name.name.clone()))
        .collect();
    let names = named(&picked, &mut taken);

    let sources = block(text, "sources")?.ok_or_else(|| {
        // Unreachable through the control plane: `sources:` is required, so a config
        // without one didn't parse and never got this far.
        ScaffoldError::Unextendable("sources".to_owned())
    })?;
    let mut source_lines = String::new();
    for (item, name) in picked.iter().zip(&names) {
        source_entry(
            &mut source_lines,
            sources.style,
            &sources.indent,
            name,
            item,
        );
    }

    // No `state:` at all — every table comes from a reducers file — so the block the
    // raw tables go in is one this has to open.
    let state = block(text, "state")?;
    let mut table_lines = String::new();
    if state.is_none() {
        table_lines.push_str("\nstate:\n");
    }
    // A `state:` block this is opening has no neighbours to copy, so it takes the
    // style of the `sources:` block it was added beside.
    let indent = state
        .as_ref()
        .map_or(sources.indent.as_str(), |b| b.indent.as_str());
    let style = state.as_ref().map_or(sources.style, |b| b.style);
    for (item, name) in picked.iter().zip(&names) {
        table_entry(&mut table_lines, style, indent, name, item);
    }
    let state_at = state.map_or_else(|| text.len(), |b| b.at);

    // Built in one pass, front to back, so neither insertion has to account for what
    // the other did to the offsets. `state:` before `sources:` is unusual but
    // perfectly good YAML, so neither order is assumed; the sort is stable, which is
    // what keeps the sources first when a config with no `state:` block puts both
    // insertions at the end of the file.
    let mut inserts = [(sources.at, source_lines), (state_at, table_lines)];
    inserts.sort_by_key(|(at, _)| *at);
    let mut out = String::with_capacity(text.len() + 256);
    let mut cut = 0;
    for (at, entries) in &inserts {
        out.push_str(&text[cut..*at]);
        // A file whose last line has no newline: one is owed before anything is added
        // under it, or the entry lands on the end of that line.
        if !out.is_empty() && !out.ends_with('\n') {
            out.push('\n');
        }
        out.push_str(entries);
        cut = *at;
    }
    out.push_str(&text[cut..]);
    Ok(out)
}
