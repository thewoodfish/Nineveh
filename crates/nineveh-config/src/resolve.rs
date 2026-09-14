//! Checking a [`Config`] against `nineveh.lock`: the types it names, the fields its
//! rules read, and the decode [`Selection`] it compiles to.

use nineveh_core::{Identifier, StructTag, TypeTag};
use nineveh_decode::{
    Body, Lockfile, Selection, SelectionError, SourceId, TableMatcher, TypeMatcher,
};

use crate::diagnostic::{Diagnostic, Diagnostics, Span};
use crate::model::{Column, Config, Expr, Named, Rule, SourceKind, TableKind};

/// A config resolved against its lock: ready to decode, fold and serve.
#[derive(Debug, Clone)]
pub struct Project {
    config: Config,
    selection: Selection,
    inputs: Vec<Input>,
    tables: Vec<ResolvedTable>,
}

impl Project {
    #[must_use]
    pub fn config(&self) -> &Config {
        &self.config
    }

    /// What to decode from the stream.
    #[must_use]
    pub fn selection(&self) -> &Selection {
        &self.selection
    }

    /// The id records of the named source carry.
    #[must_use]
    pub fn source_id(&self, name: &str) -> Option<SourceId> {
        let index = self
            .config
            .sources
            .iter()
            .position(|s| s.name.as_str() == name)?;
        Some(source_id(index))
    }

    /// What the source with this id matches, in `config().sources` order.
    #[must_use]
    pub fn input(&self, id: SourceId) -> Option<&Input> {
        self.inputs.get(usize::try_from(id.0).ok()?)
    }

    /// How each state table is built, in `config().state` order.
    #[must_use]
    pub fn tables(&self) -> &[ResolvedTable] {
        &self.tables
    }
}

fn source_id(index: usize) -> SourceId {
    // A config with more than 2^32 sources can't be written in a YAML file we'd parse.
    SourceId(u32::try_from(index).unwrap_or(u32::MAX))
}

/// What a source matches in the stream.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Input {
    Event(TypeMatcher),
    Resource(TypeMatcher),
    Table(TableMatcher),
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum ResolvedTable {
    Reduce { rules: Vec<ResolvedRule> },
    Mirror { source: SourceId },
    Log { source: SourceId },
}

/// A rule with its record's fields known.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ResolvedRule {
    pub source: SourceId,
    pub deleted: bool,
    /// The names a rule's expressions can read from the record, with their types.
    pub scope: Scope,
    /// Where each key column's value comes from, in key order.
    pub key: Vec<(String, KeySource)>,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum KeySource {
    /// The record field of the same name (the default).
    Field(Identifier),
    /// An explicit expression from the rule's `key:`.
    Expr(Expr),
}

/// The fields of a record, as seen by a rule's expressions.
///
/// - event: the event struct's fields;
/// - resource write: the resource's fields, plus `address`;
/// - resource delete: `address`;
/// - table write: `handle`, `key` and `value`; table delete: `handle` and `key`.
///
/// A struct field shadows the built-in name. For an enum record, only the fields every
/// variant declares with the same type are in scope, since the variant isn't known
/// until the record arrives. Fields whose type depends on type arguments the source
/// leaves open are omitted.
#[derive(Debug, Clone, PartialEq, Eq, Default)]
pub struct Scope {
    pub fields: Vec<(Identifier, TypeTag)>,
    /// The struct the fields come from, for messages.
    pub record: String,
    /// Set when the record is an enum, so messages can say why no fields are in scope.
    pub is_enum: bool,
}

impl Scope {
    #[must_use]
    pub fn get(&self, name: &str) -> Option<&TypeTag> {
        self.fields
            .iter()
            .find(|(n, _)| *n == *name)
            .map(|(_, t)| t)
    }

    fn push_builtin(&mut self, name: &'static str, ty: TypeTag) {
        if self.get(name).is_none() {
            self.fields.push((Identifier::from_static(name), ty));
        }
    }
}

impl Config {
    /// Resolve against `lock`: check every source type, table field and implicit key
    /// mapping, and compile the decode selection.
    ///
    /// # Errors
    ///
    /// Every problem found, located in the config source.
    pub fn resolve(self, lock: &Lockfile) -> Result<Project, Diagnostics> {
        let mut diagnostics = Vec::new();

        if lock.network() != self.network {
            diagnostics.push(
                Diagnostic::new(
                    format!(
                        "this project is for {}, but nineveh.lock was built for {}",
                        self.network,
                        lock.network()
                    ),
                    self.network_span,
                )
                .help("run `nineveh init` to rebuild the lock"),
            );
        }

        let mut selection = Selection::new();
        let mut inputs = Vec::new();
        for (index, source) in self.sources.iter().enumerate() {
            let id = source_id(index);
            let at = source.type_span;
            let input = match &source.kind {
                SourceKind::Event(tag) => matcher(lock, tag).and_then(|m| {
                    selection.add_event(lock, id, m.clone())?;
                    Ok(Input::Event(m))
                }),
                SourceKind::Resource(tag) => matcher(lock, tag).and_then(|m| {
                    selection.add_resource(lock, id, m.clone())?;
                    Ok(Input::Resource(m))
                }),
                SourceKind::Table { parent, field } => {
                    TableMatcher::for_field(lock, parent, field.as_str()).map(|m| {
                        if let Some(other) = colliding_table(lock, parent, field, &m) {
                            diagnostics.push(
                                Diagnostic::new(
                                    format!(
                                        "table items can't be told apart: `{other}` holds a table \
                                         with the same key and value types"
                                    ),
                                    at,
                                )
                                .help(
                                    "items are matched by type, so both tables' items would land \
                                     in this source; attributing items by table handle isn't \
                                     supported yet",
                                ),
                            );
                        }
                        selection.add_table(id, &m);
                        Input::Table(m)
                    })
                }
            };
            match input {
                Ok(input) => inputs.push(input),
                Err(e) => diagnostics.push(selection_diagnostic(&e, at)),
            }
        }
        // Tables and rules can't be checked against sources that didn't resolve.
        if let Some(diagnostics) = Diagnostics::from_vec(std::mem::take(&mut diagnostics)) {
            return Err(diagnostics);
        }

        let mut tables = Vec::new();
        for table in &self.state {
            let resolved = match &table.kind {
                TableKind::Mirror { source } => ResolvedTable::Mirror {
                    source: self.id_of(source),
                },
                TableKind::Log { source } => ResolvedTable::Log {
                    source: self.id_of(source),
                },
                TableKind::Reduce {
                    key,
                    columns,
                    rules,
                } => ResolvedTable::Reduce {
                    rules: rules
                        .iter()
                        .map(|rule| self.rule(lock, &inputs, rule, key, columns, &mut diagnostics))
                        .collect(),
                },
            };
            tables.push(resolved);
        }

        match Diagnostics::from_vec(diagnostics) {
            Some(diagnostics) => Err(diagnostics),
            None => Ok(Project {
                config: self,
                selection,
                inputs,
                tables,
            }),
        }
    }

    fn id_of(&self, source: &Named) -> SourceId {
        // Validation guarantees every reference names a source.
        source_id(
            self.sources
                .iter()
                .position(|s| s.name.name == source.name)
                .unwrap_or(usize::MAX),
        )
    }

    fn rule(
        &self,
        lock: &Lockfile,
        inputs: &[Input],
        rule: &Rule,
        key: &[Named],
        columns: &[Column],
        diagnostics: &mut Vec<Diagnostic>,
    ) -> ResolvedRule {
        let source = self.id_of(&rule.on.source);
        let scope = inputs
            .get(usize::try_from(source.0).unwrap_or(usize::MAX))
            .map(|input| scope(lock, input, rule.on.deleted))
            .unwrap_or_default();

        let mut bindings = Vec::new();
        for column_name in key {
            if let Some((_, expr)) = rule.key.iter().find(|(n, _)| n.name == column_name.name) {
                bindings.push((column_name.name.clone(), KeySource::Expr(expr.clone())));
                continue;
            }
            let Some(column) = columns.iter().find(|c| c.name.name == column_name.name) else {
                continue;
            };
            match scope.get(&column_name.name) {
                Some(ty) if column.ty.accepts(ty, false) => {
                    // The scope only holds valid identifiers.
                    if let Ok(field) = column_name.name.parse() {
                        bindings.push((column_name.name.clone(), KeySource::Field(field)));
                    }
                }
                Some(ty) => diagnostics.push(
                    Diagnostic::new(
                        format!(
                            "`{}.{}` is a `{ty}`, but key column `{}` is `{}`",
                            scope.record, column_name, column_name, column.ty
                        ),
                        rule.on.span,
                    )
                    .help(format!(
                        "convert it under the rule's `key`, like `key: {{ {column_name}: \"...\" }}`"
                    )),
                ),
                None => {
                    let why = if scope.is_enum {
                        format!(
                            "`{}` is an enum with no field `{column_name}` in every variant",
                            scope.record
                        )
                    } else {
                        format!("`{}` has no field `{column_name}`", scope.record)
                    };
                    diagnostics.push(
                        Diagnostic::new(
                            format!(
                                "this rule doesn't say where key column `{column_name}` comes \
                                 from, and {why}"
                            ),
                            rule.on.span,
                        )
                        .did_you_mean(
                            &column_name.name,
                            scope.fields.iter().map(|(n, _)| n.as_str()),
                        )
                        .help_if_none(format!(
                            "map it under the rule's `key`, like `key: {{ {column_name}: \"...\" }}`"
                        )),
                    );
                }
            }
        }

        ResolvedRule {
            source,
            deleted: rule.on.deleted,
            scope,
            key: bindings,
        }
    }
}

/// `Exact` when type arguments are given or the struct isn't generic; `AnyInstance`
/// for a generic struct named without arguments.
fn matcher(lock: &Lockfile, tag: &StructTag) -> Result<TypeMatcher, SelectionError> {
    let layout = lock
        .get(&tag.name)
        .ok_or_else(|| SelectionError::NoLayout(tag.name.clone()))?;
    Ok(if tag.type_args.is_empty() && layout.type_params > 0 {
        TypeMatcher::AnyInstance(tag.name.clone())
    } else {
        TypeMatcher::Exact(tag.clone())
    })
}

fn scope(lock: &Lockfile, input: &Input, deleted: bool) -> Scope {
    match input {
        Input::Event(m) => struct_scope(lock, m),
        Input::Resource(m) => {
            let mut scope = if deleted {
                Scope {
                    record: matcher_name(m),
                    ..Scope::default()
                }
            } else {
                struct_scope(lock, m)
            };
            scope.push_builtin("address", TypeTag::Address);
            scope
        }
        Input::Table(t) => {
            let mut scope = Scope {
                record: "table item".to_owned(),
                ..Scope::default()
            };
            scope.push_builtin("handle", TypeTag::Address);
            scope.push_builtin("key", t.key.clone());
            if !deleted {
                scope.push_builtin("value", t.value.clone());
            }
            scope
        }
    }
}

fn matcher_name(m: &TypeMatcher) -> String {
    match m {
        TypeMatcher::Exact(tag) => tag.to_string(),
        TypeMatcher::AnyInstance(name) => name.to_string(),
    }
}

fn struct_scope(lock: &Lockfile, m: &TypeMatcher) -> Scope {
    let (name, args) = match m {
        TypeMatcher::Exact(tag) => (&tag.name, Some(tag.type_args.as_slice())),
        TypeMatcher::AnyInstance(name) => (name, None),
    };
    let mut scope = Scope {
        record: name.name.to_string(),
        ..Scope::default()
    };
    let Some(layout) = lock.get(name) else {
        return scope;
    };
    scope.is_enum = matches!(layout.body, Body::Enum(_));
    for field in layout.common_fields() {
        let ty = match args {
            Some(args) => field.ty.substitute(args).ok(),
            None => Some(field.ty.clone()),
        };
        if let Some(ty) = ty.filter(TypeTag::is_concrete) {
            scope.fields.push((field.name.clone(), ty));
        }
    }
    scope
}

/// Another field in the lock whose table items have the same stream types as `m`'s.
///
/// Only non-generic parents are compared; a generic parent's item types depend on its
/// instantiation.
fn colliding_table(
    lock: &Lockfile,
    parent: &StructTag,
    field: &Identifier,
    m: &TableMatcher,
) -> Option<String> {
    let items = m.item_types();
    for (name, layout) in lock.structs() {
        if layout.type_params > 0 {
            continue;
        }
        let tag = StructTag {
            name: name.clone(),
            type_args: Vec::new(),
        };
        let names: Vec<&Identifier> = match &layout.body {
            Body::Struct(fields) => fields.iter().map(|f| &f.name).collect(),
            Body::Enum(variants) => {
                let mut names: Vec<&Identifier> = variants
                    .iter()
                    .flat_map(|v| v.fields.iter().map(|f| &f.name))
                    .collect();
                names.sort();
                names.dedup();
                names
            }
        };
        for other in names {
            if tag == *parent && other == field {
                continue;
            }
            if let Ok(candidate) = TableMatcher::for_field(lock, &tag, other.as_str())
                && candidate.item_types() == items
            {
                return Some(format!("{name}.{other}"));
            }
        }
    }
    None
}

fn selection_diagnostic(e: &SelectionError, at: Option<Span>) -> Diagnostic {
    let d = Diagnostic::new(e.to_string(), at);
    match e {
        SelectionError::NoLayout(_) => d.help("run `nineveh init` to refresh nineveh.lock"),
        SelectionError::Generic { .. } => d.help(
            "table items are matched by type, so the table's key and value types must be \
             concrete",
        ),
        _ => d,
    }
}
