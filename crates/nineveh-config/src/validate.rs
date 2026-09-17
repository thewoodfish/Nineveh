//! From YAML text to a validated [`Config`], collecting every problem.

use nineveh_core::{I256, Identifier, Network, StructTag, Value, parse_u256};
use serde_saphyr::{Location, Spanned};

use crate::diagnostic::{Diagnostic, Diagnostics, Span};
use crate::model::{
    Action, Api, Change, Column, ColumnType, Config, Expr, Named, Rule, Source, SourceKind,
    StartVersion, StateTable, Subscription, TableKind, Trigger, Webhook,
};
use crate::raw::{
    Entries, ExprText, Literal, RawColumn, RawConfig, RawRule, RawSource, RawStart, RawTable,
};

/// Parse and validate `nineveh.yaml`.
///
/// This checks everything that doesn't need `nineveh.lock`: YAML syntax and shape,
/// names, references between sources, tables and rules, column types and defaults.
/// Resolve the result against the lock with [`Config::resolve`].
///
/// # Errors
///
/// Every problem found, each located in `source`. A YAML syntax or shape error stops
/// parsing, so it's reported alone; everything else is reported together.
pub fn parse(source: &str) -> Result<Config, Diagnostics> {
    let mut options = serde_saphyr::Options::default();
    // `yes`/`no`/`on`/`off` as booleans cause more surprises than they save typing.
    options.strict_booleans = true;
    let raw: RawConfig = serde_saphyr::from_str_with_options(source, options)
        .map_err(|e| Diagnostics::single(yaml_diagnostic(&e)))?;
    let mut v = Validator::default();
    let config = v.config(raw);
    match Diagnostics::from_vec(v.diagnostics) {
        Some(diagnostics) => Err(diagnostics),
        None => Ok(config),
    }
}

/// Turn a YAML error into our diagnostic shape, dropping the parser's own location
/// suffix and snippet (we render those ourselves).
fn yaml_diagnostic(e: &serde_saphyr::Error) -> Diagnostic {
    let mut plain = serde_saphyr::RenderOptions::default();
    plain.snippets = serde_saphyr::SnippetMode::Off;
    let mut message = e.render_with_options(plain);
    let location = e.location();
    if let Some(loc) = &location {
        let suffix = format!(" at line {}, column {}", loc.line(), loc.column());
        if let Some(len) = message.strip_suffix(&suffix).map(str::len) {
            message.truncate(len);
        }
    }
    if let Some(key) = message
        .strip_prefix("duplicate mapping key: ")
        .and_then(|rest| rest.split(", set DuplicateKeyPolicy").next())
    {
        message = format!("`{key}` is defined twice");
    }
    let at = location.as_ref().and_then(Span::from_location);
    if message.starts_with("unknown field `realtime`") {
        return Diagnostic::new("`realtime` is now `webhooks`", at).help(
            "webhooks are named, so several changes can share one endpoint and its secret: \
             `webhooks: { my_backend: { url: ..., on: [balances.changed] } }`",
        );
    }
    Diagnostic::new(message, at)
}

fn span(location: &Location) -> Option<Span> {
    Span::from_location(location)
}

fn span_of<T>(spanned: &Spanned<T>) -> Option<Span> {
    span(&spanned.referenced)
}

#[derive(Default)]
struct Validator {
    diagnostics: Vec<Diagnostic>,
}

/// The facts about sources that table and rule checks need.
struct SourceInfo<'a> {
    name: &'a str,
    kind: Option<&'static str>,
}

impl Validator {
    fn push(&mut self, diagnostic: Diagnostic) {
        self.diagnostics.push(diagnostic);
    }

    fn config(&mut self, raw: RawConfig) -> Config {
        let name = self.name(&raw.name.value, span_of(&raw.name), "project");

        let network = raw.network.value.parse().unwrap_or_else(|_| {
            self.push(
                Diagnostic::new(
                    format!("unknown network `{}`", raw.network.value),
                    span_of(&raw.network),
                )
                .help("use one of mainnet, testnet, devnet")
                .did_you_mean(&raw.network.value, Network::ALL.map(Network::as_str)),
            );
            Network::Testnet
        });

        let start_version = match raw.start_version.map(|s| s.value) {
            None | Some(RawStart::Auto) => StartVersion::Auto,
            Some(RawStart::Version(v)) => StartVersion::Version(v),
        };

        let sources = self.sources(&raw.sources);
        let infos: Vec<SourceInfo<'_>> = sources
            .iter()
            .map(|s| SourceInfo {
                name: s.name.as_str(),
                kind: Some(s.kind.keyword()),
            })
            .collect();
        // Sources whose type failed to parse still count as defined, so references to
        // them don't pile on a second error.
        let mut all_infos = infos;
        for (key, _) in &raw.sources.value.0 {
            if !all_infos.iter().any(|i| i.name == key.value) {
                all_infos.push(SourceInfo {
                    name: &key.value,
                    kind: None,
                });
            }
        }

        let state = self.tables(&raw.state, &all_infos);
        let webhooks = raw
            .webhooks
            .0
            .iter()
            .filter_map(|(name, hook)| self.webhook(name, hook, &raw.state.value))
            .collect();
        let api = raw.api.map_or_else(Api::default, |a| Api {
            rest: a.rest.unwrap_or(true),
            graphql: a.graphql.unwrap_or(true),
        });

        Config {
            name,
            network,
            network_span: span_of(&raw.network),
            start_version,
            sources,
            state,
            api,
            webhooks,
        }
    }

    fn name(&mut self, name: &str, span: Option<Span>, what: &str) -> Named {
        if !Named::is_valid(name) {
            let help = if name.starts_with('_') {
                "names starting with `_` are reserved for Nineveh's own columns"
            } else {
                "use lower snake case: a-z, 0-9 and _, starting with a letter, at most 63 \
                 characters"
            };
            self.push(Diagnostic::new(format!("invalid {what} name `{name}`"), span).help(help));
        }
        Named {
            name: name.to_owned(),
            span,
        }
    }

    // --- sources -----------------------------------------------------------------

    fn sources(&mut self, raw: &Spanned<Entries<Spanned<RawSource>>>) -> Vec<Source> {
        if raw.value.0.is_empty() {
            self.push(
                Diagnostic::new("a project needs at least one source", span_of(raw))
                    .help("add one, like `deposits: { event: 0xabc::vault::DepositEvent }`"),
            );
        }
        raw.value
            .0
            .iter()
            .filter_map(|(key, source)| {
                let name = self.name(&key.value, span_of(key), "source");
                let (text, ty_span) = match &source.value {
                    RawSource::Event(t) | RawSource::Resource(t) | RawSource::Table(t) => {
                        (&t.value, span_of(t))
                    }
                };
                let kind = match &source.value {
                    RawSource::Event(_) => self.struct_type(text, ty_span).map(SourceKind::Event),
                    RawSource::Resource(_) => {
                        self.struct_type(text, ty_span).map(SourceKind::Resource)
                    }
                    RawSource::Table(_) => self.table_field(text, ty_span),
                }?;
                Some(Source {
                    name,
                    kind,
                    type_span: ty_span,
                })
            })
            .collect()
    }

    fn struct_type(&mut self, text: &str, span: Option<Span>) -> Option<StructTag> {
        text.trim()
            .parse::<StructTag>()
            .map_err(|e| {
                self.push(
                    Diagnostic::new(
                        format!("`{text}` isn't a Move struct type: {}", e.reason),
                        span,
                    )
                    .help("write it as `address::module::Struct`, like `0x1::coin::CoinStore`"),
                );
            })
            .ok()
    }

    fn table_field(&mut self, text: &str, span: Option<Span>) -> Option<SourceKind> {
        let bad = |v: &mut Self, reason: &str| {
            v.push(
                Diagnostic::new(format!("`{text}` isn't a table field: {reason}"), span).help(
                    "name the struct field that holds the table, like \
                     `0xabc::vault::Vault.positions`",
                ),
            );
        };
        let Some((parent, field)) = text.trim().rsplit_once('.') else {
            bad(self, "expected `address::module::Struct.field`");
            return None;
        };
        let Ok(field) = field.parse::<Identifier>() else {
            bad(self, &format!("`{field}` isn't a field name"));
            return None;
        };
        match parent.parse::<StructTag>() {
            Ok(parent) => Some(SourceKind::Table { parent, field }),
            Err(e) => {
                bad(self, e.reason);
                None
            }
        }
    }

    // --- state tables ------------------------------------------------------------

    fn tables(
        &mut self,
        raw: &Spanned<Entries<Spanned<RawTable>>>,
        sources: &[SourceInfo<'_>],
    ) -> Vec<StateTable> {
        if raw.value.0.is_empty() {
            self.push(
                Diagnostic::new("a project needs at least one state table", span_of(raw))
                    .help("add one, like `vaults: { mirror: vaults }`"),
            );
        }
        raw.value
            .0
            .iter()
            .filter_map(|(key, table)| {
                let name = self.name(&key.value, span_of(key), "state table");
                let kind = self.table_kind(&name, &table.value, span_of(table), sources)?;
                Some(StateTable { name, kind })
            })
            .collect()
    }

    fn table_kind(
        &mut self,
        table: &Named,
        raw: &RawTable,
        span: Option<Span>,
        sources: &[SourceInfo<'_>],
    ) -> Option<TableKind> {
        let kinds: Vec<&str> = [
            raw.reduce.as_ref().map(|_| "reduce"),
            raw.mirror.as_ref().map(|_| "mirror"),
            raw.log.as_ref().map(|_| "log"),
        ]
        .into_iter()
        .flatten()
        .collect();
        match kinds.as_slice() {
            [] => {
                self.push(
                    Diagnostic::new(
                        format!("state table `{table}` doesn't say how it's built"),
                        table.span.or(span),
                    )
                    .help("give it `reduce` (with `key` and `columns`), `mirror` or `log`"),
                );
                return None;
            }
            [_] => {}
            [a, b, ..] => {
                self.push(
                    Diagnostic::new(
                        format!("state table `{table}` has both `{a}` and `{b}`"),
                        table.span.or(span),
                    )
                    .help("a table is built one way: `reduce`, `mirror` or `log`"),
                );
                return None;
            }
        }

        if let Some(source) = raw.mirror.as_ref().or(raw.log.as_ref()) {
            let (keyword, allowed): (&str, &[&str]) = if raw.mirror.is_some() {
                ("mirror", &["resource", "table"])
            } else {
                ("log", &["event"])
            };
            for (present, field) in [
                (raw.key.as_ref().map(span_of), "key"),
                (raw.columns.as_ref().map(span_of), "columns"),
            ] {
                if let Some(field_span) = present {
                    self.push(
                        Diagnostic::new(
                            format!("`{field}` only applies to `reduce` tables"),
                            field_span,
                        )
                        .help(format!(
                            "a `{keyword}` table's columns come from its source's layout"
                        )),
                    );
                }
            }
            let source =
                self.source_ref(&source.value, span_of(source), sources, allowed, keyword)?;
            return Some(if keyword == "mirror" {
                TableKind::Mirror { source }
            } else {
                TableKind::Log { source }
            });
        }

        let reduce = raw.reduce.as_ref()?;
        self.reduce_table(table, raw, reduce, span, sources)
    }

    fn source_ref(
        &mut self,
        name: &str,
        span: Option<Span>,
        sources: &[SourceInfo<'_>],
        allowed: &[&str],
        role: &str,
    ) -> Option<Named> {
        let Some(info) = sources.iter().find(|s| s.name == name) else {
            self.push(
                Diagnostic::new(format!("unknown source `{name}`"), span)
                    .did_you_mean(name, sources.iter().map(|s| s.name)),
            );
            return None;
        };
        if let Some(kind) = info.kind
            && !allowed.contains(&kind)
        {
            self.push(Diagnostic::new(
                format!(
                    "`{role}` needs {} source, but `{name}` is {} source",
                    with_article(&allowed.join(" or ")),
                    with_article(kind)
                ),
                span,
            ));
            return None;
        }
        Some(Named {
            name: name.to_owned(),
            span,
        })
    }

    fn reduce_table(
        &mut self,
        table: &Named,
        raw: &RawTable,
        reduce: &Spanned<Vec<Spanned<RawRule>>>,
        span: Option<Span>,
        sources: &[SourceInfo<'_>],
    ) -> Option<TableKind> {
        let columns = match &raw.columns {
            Some(columns) if !columns.value.0.is_empty() => self.columns(columns),
            other => {
                self.push(
                    Diagnostic::new(
                        format!("reduce table `{table}` needs `columns`"),
                        other.as_ref().map_or(table.span.or(span), span_of),
                    )
                    .help("list its columns and types, like `balance: u128`"),
                );
                return None;
            }
        };
        let key = self.key(table, raw.key.as_ref(), &columns, span)?;
        if reduce.value.is_empty() {
            self.push(
                Diagnostic::new(
                    format!("reduce table `{table}` has no rules"),
                    span_of(reduce),
                )
                .help(
                    "add a rule, like `- { on: deposits, set: { balance: \"balance + amount\" } }`",
                ),
            );
        }
        let rules = reduce
            .value
            .iter()
            .filter_map(|rule| self.rule(rule, &key, &columns, sources))
            .collect();
        Some(TableKind::Reduce {
            key,
            columns,
            rules,
        })
    }

    fn columns(&mut self, raw: &Spanned<Entries<Spanned<RawColumn>>>) -> Vec<Column> {
        raw.value
            .0
            .iter()
            .filter_map(|(key, column)| {
                let name = self.name(&key.value, span_of(key), "column");
                let ty_span = span_of(&column.value.ty).or_else(|| span_of(column));
                let ty = self.column_type(&column.value.ty.value, ty_span)?;
                let default = column.value.default.as_ref().and_then(|d| {
                    self.column_default(&d.value, span_of(d), ty, column.value.nullable)
                });
                Some(Column {
                    name,
                    ty,
                    nullable: column.value.nullable,
                    default,
                })
            })
            .collect()
    }

    fn column_type(&mut self, text: &str, span: Option<Span>) -> Option<ColumnType> {
        if let Some(ty) = ColumnType::parse(text) {
            return Some(ty);
        }
        let hint = match text {
            "vector<u8>" => Some("byte vectors are `bytes`"),
            _ if text.contains("::string::String") || text == "String" => {
                Some("strings are `string`")
            }
            _ if text.contains("::object::Object") => Some("objects are stored as `address`"),
            _ if text.contains("::option::Option") => {
                Some("use the inner type with `nullable: true`")
            }
            _ if text.contains("::") || text.starts_with("vector<") => {
                Some("structured values are `json`")
            }
            _ => None,
        };
        let d = Diagnostic::new(format!("unknown column type `{text}`"), span);
        self.push(match hint {
            Some(hint) => d.help(hint),
            None => d
                .did_you_mean(text, ColumnType::ALL.map(ColumnType::as_str))
                .help_if_none(format!(
                    "use one of {}",
                    ColumnType::ALL.map(ColumnType::as_str).join(", ")
                )),
        });
        None
    }

    fn column_default(
        &mut self,
        literal: &Literal,
        span: Option<Span>,
        ty: ColumnType,
        nullable: bool,
    ) -> Option<Value> {
        let value = match (literal, ty) {
            (Literal::Null, _) if nullable => return None,
            (Literal::Null, _) => Err("only a nullable column can default to null".to_owned()),
            (_, ColumnType::Json) => Err("json columns can't have a default".to_owned()),
            (Literal::Bool(b), ColumnType::Bool) => Ok(Value::Bool(*b)),
            (Literal::Str(s), ColumnType::String) => Ok(Value::String(s.clone())),
            (Literal::Str(s), ColumnType::Address) => s
                .parse()
                .map(Value::Address)
                .map_err(|e: nineveh_core::InvalidAddress| e.to_string()),
            (Literal::Int(_) | Literal::BigUint(_), ColumnType::Address) => {
                Err("write addresses as quoted strings, like \"0x1\"".to_owned())
            }
            (Literal::Str(s), ColumnType::Bytes) => {
                hex_bytes(s).map(Value::Bytes).ok_or_else(|| {
                    "bytes are `0x` followed by an even number of hex digits".to_owned()
                })
            }
            (literal, ty) => integer(literal, ty),
        };
        match value {
            Ok(v) => Some(v),
            Err(reason) => {
                self.push(Diagnostic::new(
                    format!("invalid default for this {ty} column: {reason}"),
                    span,
                ));
                None
            }
        }
    }

    fn key(
        &mut self,
        table: &Named,
        raw: Option<&Spanned<Vec<Spanned<String>>>>,
        columns: &[Column],
        span: Option<Span>,
    ) -> Option<Vec<Named>> {
        let Some(raw) = raw.filter(|k| !k.value.is_empty()) else {
            self.push(
                Diagnostic::new(
                    format!("reduce table `{table}` needs a `key`"),
                    raw.map_or(table.span.or(span), span_of),
                )
                .help("name the columns that identify a row, like `key: [user]`"),
            );
            return None;
        };
        let mut key: Vec<Named> = Vec::new();
        for name in &raw.value {
            let at = span_of(name);
            let Some(column) = columns.iter().find(|c| c.name.as_str() == name.value) else {
                self.push(
                    Diagnostic::new(
                        format!("key column `{}` isn't in `columns`", name.value),
                        at,
                    )
                    .did_you_mean(&name.value, columns.iter().map(|c| c.name.as_str())),
                );
                continue;
            };
            if key.iter().any(|k| k.name == name.value) {
                self.push(Diagnostic::new(
                    format!("`{}` is listed in the key twice", name.value),
                    at,
                ));
                continue;
            }
            if !column.ty.is_keyable() {
                self.push(Diagnostic::new(
                    format!(
                        "`{}` is a json column, which can't be part of a key",
                        name.value
                    ),
                    at,
                ));
            }
            if column.nullable {
                self.push(Diagnostic::new(
                    format!("key column `{}` can't be nullable", name.value),
                    at,
                ));
            }
            if column.default.is_some() {
                self.push(
                    Diagnostic::new(
                        format!("key column `{}` can't have a default", name.value),
                        at,
                    )
                    .help("each rule's `key` gives key columns their values"),
                );
            }
            key.push(Named {
                name: name.value.clone(),
                span: at,
            });
        }
        Some(key)
    }

    fn rule(
        &mut self,
        raw: &Spanned<RawRule>,
        key: &[Named],
        columns: &[Column],
        sources: &[SourceInfo<'_>],
    ) -> Option<Rule> {
        let rule_span = span_of(raw);
        let raw = &raw.value;
        let on = self.trigger(&raw.on, sources)?;

        let action = match (&raw.set, &raw.delete) {
            (Some(set), None) => Action::Set(self.assignments(set, key, columns)),
            (None, Some(delete)) if delete.value => Action::Delete,
            (None, Some(delete)) => {
                self.push(Diagnostic::new(
                    "`delete: false` does nothing; remove it and give the rule a `set`",
                    span_of(delete),
                ));
                return None;
            }
            (Some(set), Some(_)) => {
                self.push(Diagnostic::new(
                    "a rule either sets columns or deletes the row, not both",
                    span_of(set),
                ));
                return None;
            }
            (None, None) => {
                self.push(
                    Diagnostic::new("this rule doesn't do anything", on.span.or(rule_span))
                        .help("give it `set: { column: \"expression\" }` or `delete: true`"),
                );
                return None;
            }
        };

        // Any rule may be the one that creates a row, so every set rule must give
        // every column a value: explicitly, by default, or null.
        if let Action::Set(assignments) = &action {
            for column in columns {
                let is_key = key.iter().any(|k| k.name == column.name.name);
                let assigned = assignments.iter().any(|(n, _)| n.name == column.name.name);
                if !is_key && !assigned && column.default.is_none() && !column.nullable {
                    self.push(
                        Diagnostic::new(
                            format!(
                                "this rule can create a row but doesn't set `{}`, which has \
                                 no default",
                                column.name
                            ),
                            raw.set.as_ref().and_then(span_of).or(rule_span),
                        )
                        .help(format!(
                            "set `{}` here, give it a `default`, or make it `nullable: true`",
                            column.name
                        )),
                    );
                }
            }
        }

        let key_exprs = raw
            .key
            .as_ref()
            .map(|k| {
                k.value
                    .0
                    .iter()
                    .filter_map(|(name, expr)| {
                        if !key.iter().any(|k| k.name == name.value) {
                            self.push(
                                Diagnostic::new(
                                    format!("`{}` isn't a key column", name.value),
                                    span_of(name),
                                )
                                .did_you_mean(&name.value, key.iter().map(Named::as_str)),
                            );
                            return None;
                        }
                        Some((
                            Named {
                                name: name.value.clone(),
                                span: span_of(name),
                            },
                            self.expr(expr)?,
                        ))
                    })
                    .collect()
            })
            .unwrap_or_default();

        let when = raw.when.as_ref().and_then(|w| self.expr(w));
        Some(Rule {
            on,
            when,
            key: key_exprs,
            action,
        })
    }

    fn trigger(&mut self, raw: &Spanned<String>, sources: &[SourceInfo<'_>]) -> Option<Trigger> {
        let at = span_of(raw);
        let (name, deleted) = match raw.value.split_once('.') {
            None => (raw.value.as_str(), false),
            Some((name, "deleted")) => (name, true),
            Some((_, other)) => {
                self.push(
                    Diagnostic::new(format!("unknown trigger `.{other}`"), at).help(
                        "use `<source>` for events and writes, or `<source>.deleted` for deletes",
                    ),
                );
                return None;
            }
        };
        let Some(info) = sources.iter().find(|s| s.name == name) else {
            self.push(
                Diagnostic::new(format!("unknown source `{name}`"), at)
                    .did_you_mean(name, sources.iter().map(|s| s.name)),
            );
            return None;
        };
        if deleted && info.kind == Some("event") {
            self.push(Diagnostic::new(
                format!("`{name}` is an event source, and events are never deleted"),
                at,
            ));
            return None;
        }
        Some(Trigger {
            source: Named {
                name: name.to_owned(),
                span: at,
            },
            deleted,
            span: at,
        })
    }

    fn assignments(
        &mut self,
        set: &Spanned<Entries<Spanned<ExprText>>>,
        key: &[Named],
        columns: &[Column],
    ) -> Vec<(Named, Expr)> {
        if set.value.0.is_empty() {
            self.push(Diagnostic::new("`set` is empty", span_of(set)));
        }
        set.value
            .0
            .iter()
            .filter_map(|(name, expr)| {
                let at = span_of(name);
                if key.iter().any(|k| k.name == name.value) {
                    self.push(
                        Diagnostic::new(format!("`{}` is part of the key", name.value), at)
                            .help("key columns get their values from the rule's `key`"),
                    );
                    return None;
                }
                if !columns.iter().any(|c| c.name.as_str() == name.value) {
                    self.push(
                        Diagnostic::new(format!("unknown column `{}`", name.value), at)
                            .did_you_mean(&name.value, columns.iter().map(|c| c.name.as_str())),
                    );
                    return None;
                }
                Some((
                    Named {
                        name: name.value.clone(),
                        span: at,
                    },
                    self.expr(expr)?,
                ))
            })
            .collect()
    }

    fn expr(&mut self, raw: &Spanned<ExprText>) -> Option<Expr> {
        if raw.value.0.trim().is_empty() {
            self.push(Diagnostic::new("empty expression", span_of(raw)));
            return None;
        }
        Some(Expr {
            text: raw.value.0.clone(),
            span: span_of(raw),
        })
    }

    // --- webhooks ----------------------------------------------------------------

    /// One named endpoint: where deliveries go, and which changes it asks for
    /// (ADR 0020).
    fn webhook(
        &mut self,
        name: &Spanned<String>,
        raw: &Spanned<crate::raw::RawWebhook>,
        raw_tables: &Entries<Spanned<RawTable>>,
    ) -> Option<Webhook> {
        let at = span_of(name);
        let named = self.name(&name.value, at, "webhook");
        let url = &raw.value.url;
        if let Err(reason) = check_webhook(&url.value) {
            self.push(Diagnostic::new(
                format!("invalid webhook URL `{}`: {reason}", url.value),
                span_of(url),
            ));
            return None;
        }
        if raw.value.on.is_empty() {
            self.push(
                Diagnostic::new(format!("webhook `{named}` asks for nothing"), at)
                    .help("list the changes it wants, like `on: [balances.changed]`"),
            );
            return None;
        }
        let on: Vec<Subscription> = raw
            .value
            .on
            .iter()
            .filter_map(|entry| self.subscription(entry, raw_tables))
            .collect();
        // An endpoint with a change nobody can deliver isn't saved at all, so the
        // config never half-describes where state goes.
        (on.len() == raw.value.on.len()).then(|| Webhook {
            name: named,
            url: url.value.clone(),
            on,
            rows: raw.value.rows.unwrap_or(true),
        })
    }

    /// `<table>.changed`, `.inserted`, `.updated` or `.deleted`.
    fn subscription(
        &mut self,
        on: &Spanned<String>,
        raw_tables: &Entries<Spanned<RawTable>>,
    ) -> Option<Subscription> {
        let at = span_of(on);
        let Some((table, change)) = on.value.split_once('.') else {
            self.push(
                Diagnostic::new(format!("`{}` isn't a change feed", on.value), at)
                    .help("write `<table>.changed`, `.inserted`, `.updated` or `.deleted`"),
            );
            return None;
        };
        let names = || raw_tables.0.iter().map(|(k, _)| k.value.as_str());
        if !names().any(|n| n == table) {
            self.push(
                Diagnostic::new(format!("unknown state table `{table}`"), at)
                    .did_you_mean(table, names()),
            );
            return None;
        }
        let Some(change) = Change::ALL.into_iter().find(|c| c.as_str() == change) else {
            self.push(
                Diagnostic::new(format!("unknown change `.{change}`"), at)
                    .did_you_mean(change, Change::ALL.map(Change::as_str))
                    .help_if_none("use `.changed`, `.inserted`, `.updated` or `.deleted`"),
            );
            return None;
        };
        Some(Subscription {
            table: Named {
                name: table.to_owned(),
                span: at,
            },
            change,
        })
    }
}

fn integer(literal: &Literal, ty: ColumnType) -> Result<Value, String> {
    let out_of_range = || format!("out of range for {ty}");
    let not_int = || "expected an integer".to_owned();
    // Wide integers may be written as decimal strings, since YAML numbers stop at 64
    // bits in many tools.
    let as_i128 = |literal: &Literal| -> Result<Option<i128>, String> {
        match literal {
            Literal::Int(i) => Ok(Some(*i)),
            Literal::BigUint(_) => Ok(None),
            Literal::Str(s) => {
                let digits = s.strip_prefix('-').unwrap_or(s);
                if digits.is_empty() || !digits.bytes().all(|b| b.is_ascii_digit()) {
                    return Err(not_int());
                }
                Ok(s.parse::<i128>().ok())
            }
            _ => Err(not_int()),
        }
    };
    macro_rules! small {
        ($variant:ident, $t:ty) => {{
            let i = as_i128(literal)?.ok_or_else(out_of_range)?;
            <$t>::try_from(i)
                .map(Value::$variant)
                .map_err(|_| out_of_range())
        }};
    }
    match ty {
        ColumnType::U8 => small!(U8, u8),
        ColumnType::U16 => small!(U16, u16),
        ColumnType::U32 => small!(U32, u32),
        ColumnType::U64 => small!(U64, u64),
        ColumnType::I8 => small!(I8, i8),
        ColumnType::I16 => small!(I16, i16),
        ColumnType::I32 => small!(I32, i32),
        ColumnType::I64 => small!(I64, i64),
        ColumnType::I128 => small!(I128, i128),
        ColumnType::U128 => match literal {
            Literal::BigUint(u) => Ok(Value::U128(*u)),
            Literal::Int(i) => u128::try_from(*i)
                .map(Value::U128)
                .map_err(|_| out_of_range()),
            Literal::Str(s) if is_decimal(s) => {
                s.parse().map(Value::U128).map_err(|_| out_of_range())
            }
            _ => Err(not_int()),
        },
        ColumnType::U256 => match literal {
            Literal::BigUint(u) => Ok(Value::U256(nineveh_core::U256::from(*u))),
            Literal::Int(i) => u128::try_from(*i)
                .map(|u| Value::U256(nineveh_core::U256::from(u)))
                .map_err(|_| out_of_range()),
            Literal::Str(s) => parse_u256(s).map(Value::U256).map_err(|_| not_int()),
            _ => Err(not_int()),
        },
        ColumnType::I256 => match literal {
            Literal::Int(i) => i
                .to_string()
                .parse::<I256>()
                .map(Value::I256)
                .map_err(|_| out_of_range()),
            Literal::BigUint(u) => u
                .to_string()
                .parse::<I256>()
                .map(Value::I256)
                .map_err(|_| out_of_range()),
            Literal::Str(s) => s.parse::<I256>().map(Value::I256).map_err(|_| not_int()),
            _ => Err(not_int()),
        },
        ColumnType::Bool => Err("expected true or false".to_owned()),
        ColumnType::String => Err("expected a string".to_owned()),
        ColumnType::Bytes => Err("expected `0x` followed by hex digits".to_owned()),
        ColumnType::Address | ColumnType::Json => Err("unsupported default".to_owned()),
    }
}

/// "an event", "a resource or table".
fn with_article(noun: &str) -> String {
    let article = if noun.starts_with(['a', 'e', 'i', 'o', 'u']) {
        "an"
    } else {
        "a"
    };
    format!("{article} {noun}")
}

fn is_decimal(s: &str) -> bool {
    !s.is_empty() && s.bytes().all(|b| b.is_ascii_digit())
}

fn hex_bytes(s: &str) -> Option<Vec<u8>> {
    let digits = s.strip_prefix("0x")?.as_bytes();
    if digits.len() % 2 != 0 {
        return None;
    }
    let nibble = |c: u8| {
        char::from(c)
            .to_digit(16)
            .and_then(|d| u8::try_from(d).ok())
    };
    digits
        .as_chunks::<2>()
        .0
        .iter()
        .map(|&[high, low]| Some(nibble(high)? << 4 | nibble(low)?))
        .collect()
}

/// Webhooks must be HTTPS, except to the local machine during development.
fn check_webhook(url: &str) -> Result<(), &'static str> {
    let rest = if let Some(rest) = url.strip_prefix("https://") {
        rest
    } else if let Some(rest) = url.strip_prefix("http://") {
        let host = rest.split(['/', ':', '?', '#']).next().unwrap_or_default();
        if !matches!(host, "localhost" | "127.0.0.1" | "[::1]") {
            return Err("use https (plain http is only allowed for localhost)");
        }
        rest
    } else {
        return Err("expected an https:// URL");
    };
    let host = rest.split(['/', '?', '#']).next().unwrap_or_default();
    if host.is_empty() {
        return Err("the URL has no host");
    }
    if host.contains('@') {
        return Err("don't put credentials in the URL; webhooks are signed instead");
    }
    if url.chars().any(char::is_whitespace) {
        return Err("the URL contains whitespace");
    }
    Ok(())
}
