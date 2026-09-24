//! Joining a `.nineveh.ts` file to the `nineveh.yaml` that names it.
//!
//! `nineveh.yaml` keeps what is configuration — the network, the sources, the `mirror`
//! and `log` tables, the API and the webhooks — and the DSL file keeps what is
//! behaviour. Neither is complete alone, so this is where they become one [`Config`],
//! identical in kind to one written entirely in YAML.

use nineveh_config::{Config, Diagnostic, Diagnostics, StateTable, TableKind};

use crate::scatter::{Context, SourceInfo, TableInfo};

impl Context {
    /// What a DSL file may refer to in the config that names it: its sources, and the
    /// tables the YAML builds.
    #[must_use]
    pub fn from_config(config: &Config) -> Self {
        Self {
            sources: config
                .sources
                .iter()
                .map(|s| SourceInfo {
                    name: s.name.as_str().to_owned(),
                    has_deletes: s.kind.has_deletes(),
                })
                .collect(),
            tables: config
                .state
                .iter()
                .map(|t| TableInfo {
                    name: t.name.as_str().to_owned(),
                    key_arity: match &t.kind {
                        TableKind::Reduce { key, .. } => Some(key.len()),
                        // A mirror's key columns come from its source's layout, which
                        // isn't known until the config is resolved against the lock.
                        TableKind::Mirror { .. } | TableKind::Log { .. } => None,
                    },
                    is_log: matches!(t.kind, TableKind::Log { .. }),
                })
                .collect(),
        }
    }
}

/// Compile `source` and add its tables to `config`.
///
/// The DSL's rules are numbered after the YAML's, so every rule in the project has a
/// distinct place in the order they apply to one record (ADR 0025). Within each
/// frontend the relative order is the one it chose: table by table for YAML, statement
/// by statement for the DSL.
///
/// # Errors
///
/// [`Diagnostics`] located in the DSL source, to render against that file's name.
pub fn merge(config: Config, source: &str) -> Result<Config, Diagnostics> {
    let mut tables = crate::compile(source, &Context::from_config(&config))?;

    let mut errors = Vec::new();
    // YAML rules keep 0..n; the DSL's continue from there.
    let offset = count_rules(&config.state);
    for table in &mut tables {
        if let TableKind::Reduce { rules, .. } = &mut table.kind {
            for rule in rules {
                rule.seq = rule.seq.saturating_add(offset);
            }
        }
        if let Some(existing) = config
            .state
            .iter()
            .find(|t| t.name.as_str() == table.name.as_str())
        {
            errors.push(
                Diagnostic::new(
                    format!("`{}` is already a table in nineveh.yaml", table.name),
                    table.name.span,
                )
                .help(match &existing.kind {
                    TableKind::Reduce { .. } => {
                        "a table is built one way: move its rules here, or keep them there"
                    }
                    TableKind::Mirror { .. } | TableKind::Log { .. } => {
                        "rename this table, or remove the one in nineveh.yaml"
                    }
                }),
            );
        }
    }

    // A source with no handler isn't an error — it may feed only a mirror or a log —
    // but a project whose DSL file does nothing almost certainly means to.
    if tables.is_empty() && config.state.is_empty() {
        errors.push(
            Diagnostic::new("this file declares no tables".to_owned(), None)
                .help("declare one with `table({ key: { … }, columns: { … } })`"),
        );
    }

    match Diagnostics::from_vec(errors) {
        Some(d) => Err(d),
        None => Ok(Config {
            state: config.state.into_iter().chain(tables).collect(),
            ..config
        }),
    }
}

/// How many rules the config already has, across every table.
fn count_rules(state: &[StateTable]) -> u32 {
    let total: usize = state
        .iter()
        .map(|t| match &t.kind {
            TableKind::Reduce { rules, .. } => rules.len(),
            TableKind::Mirror { .. } | TableKind::Log { .. } => 0,
        })
        .sum();
    u32::try_from(total).unwrap_or(u32::MAX)
}
