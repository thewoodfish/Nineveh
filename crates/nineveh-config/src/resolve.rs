//! Checking a [`Config`] against `nineveh.lock`: the types it names, the fields its
//! rules read, and the decode [`Selection`] it compiles to.

use nineveh_core::{Identifier, StructTag, TypeTag};
use nineveh_decode::{
    Body, Container, Lockfile, Selection, SelectionError, SourceId, TableMatcher, TypeMatcher,
};
use nineveh_expr::{
    Cell, ColumnVar, Compiled, Env, IntType, Structs, TableColumn, TableVar, Type, compile,
};

use crate::diagnostic::{Diagnostic, Diagnostics, Span};
use crate::model::{
    Action, Column, ColumnType, Config, Expr, Named, Rule, SourceKind, StateTable, TableKind,
};
use crate::schema::{Projection, SchemaColumn, TableSchema, table_schema};

/// A config resolved against its lock: ready to decode, fold and serve.
#[derive(Debug, Clone)]
pub struct Project {
    config: Config,
    selection: Selection,
    inputs: Vec<Input>,
    tables: Vec<ResolvedTable>,
    schemas: Vec<TableSchema>,
    watchers: Vec<Watcher>,
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

    /// Each state table's stored columns, in `config().state` order.
    #[must_use]
    pub fn schemas(&self) -> &[TableSchema] {
        &self.schemas
    }

    /// The hidden sources that watch table sources' parents, to learn their handles.
    #[must_use]
    pub fn watchers(&self) -> &[Watcher] {
        &self.watchers
    }

    /// The watcher with this id, if it is one.
    #[must_use]
    pub fn watcher(&self, id: SourceId) -> Option<&Watcher> {
        self.watchers.iter().find(|w| w.id == id)
    }
}

/// A table source whose parent needs a watcher: its id, parent, field and container.
type PendingWatcher = (SourceId, StructTag, Identifier, Container);

/// A hidden source that decodes a table source's parent struct wherever the stream
/// shows it (as a resource, or as a table value) so the engine can learn which table
/// handles belong to the source (ADR 0012).
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Watcher {
    /// The id the watcher's records carry. Watcher ids follow the config's sources.
    pub id: SourceId,
    /// The table source whose handles this watcher learns.
    pub table_source: SourceId,
    pub parent: StructTag,
    /// The parent field holding the table.
    pub field: Identifier,
    pub container: Container,
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

/// A rule with every expression typechecked and compiled.
///
/// Expressions read the row as one value per column, in the table's `columns` order,
/// and the record as one value per field, in `scope.fields` order.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ResolvedRule {
    pub source: SourceId,
    pub deleted: bool,
    /// The names a rule's expressions can read from the record, with their types.
    pub scope: Scope,
    /// Each key column's value, in key order. A column the rule doesn't map reads the
    /// record field of the same name, compiled as `<source>.<field>`.
    pub key: Vec<(String, CompiledExpr)>,
    pub when: Option<CompiledExpr>,
    pub action: ResolvedAction,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum ResolvedAction {
    Set(Vec<(String, CompiledExpr)>),
    Delete,
}

/// A compiled expression and the config text it came from, so a runtime error can be
/// reported at its place in `nineveh.yaml`.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct CompiledExpr {
    pub source: Expr,
    pub compiled: Compiled,
}

impl CompiledExpr {
    /// The YAML span of a span within the expression's text.
    ///
    /// Exact when the expression is written plain or quoted without escapes; otherwise
    /// the whole expression.
    #[must_use]
    pub fn yaml_span(&self, inner: nineveh_expr::Span) -> Option<Span> {
        yaml_span(&self.source, inner)
    }
}

fn yaml_span(expr: &Expr, inner: nineveh_expr::Span) -> Option<Span> {
    let outer = expr.span?;
    let text_len = expr.text.len();
    let start = if outer.len == text_len {
        outer.offset
    } else if outer.len == text_len + 2 {
        outer.offset + 1
    } else {
        return Some(outer);
    };
    Some(Span::new(
        start + inner.start.min(text_len),
        inner.end.saturating_sub(inner.start).max(1),
    ))
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

        let (mut selection, inputs, pending_watchers) = self.sources(lock, &mut diagnostics);

        // Each table source's parent is watched wherever it can appear: as a resource
        // (if it is one) and as a table value.
        let mut watchers = Vec::new();
        for (table_source, parent, field, container) in pending_watchers {
            let id = source_id(self.sources.len() + watchers.len());
            if lock.get(&parent.name).is_some_and(|l| l.is_resource)
                && let Err(e) = selection.add_resource(lock, id, TypeMatcher::Exact(parent.clone()))
            {
                diagnostics.push(selection_diagnostic(&e, None));
            }
            selection.add_table_values(id, TypeTag::Struct(Box::new(parent.clone())));
            watchers.push(Watcher {
                id,
                table_source,
                parent,
                field,
                container,
            });
        }

        // Tables and rules can't be checked against sources that didn't resolve.
        if let Some(diagnostics) = Diagnostics::from_vec(std::mem::take(&mut diagnostics)) {
            return Err(diagnostics);
        }

        // Every table's schema first: a rule can read any of them, so they all have to
        // be known before the first rule is compiled.
        let mut schemas = Vec::new();
        for table in &self.state {
            let (input, source_span) = match &table.kind {
                TableKind::Mirror { source } | TableKind::Log { source } => {
                    let id = self.id_of(source);
                    (
                        inputs.get(usize::try_from(id.0).unwrap_or(usize::MAX)),
                        source.span,
                    )
                }
                TableKind::Reduce { .. } => (None, None),
            };
            schemas.push(table_schema(
                lock,
                table,
                input,
                source_span,
                &mut diagnostics,
            ));
        }
        let readable = table_vars(&self.state, &schemas);

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
                        .map(|rule| {
                            self.rule(
                                lock,
                                &inputs,
                                &readable,
                                rule,
                                key,
                                columns,
                                &mut diagnostics,
                            )
                        })
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
                schemas,
                watchers,
            }),
        }
    }

    /// Resolve every source into the decode selection, collecting the table sources
    /// whose parents need watching.
    fn sources(
        &self,
        lock: &Lockfile,
        diagnostics: &mut Vec<Diagnostic>,
    ) -> (Selection, Vec<Input>, Vec<PendingWatcher>) {
        let mut selection = Selection::new();
        let mut inputs = Vec::new();
        let mut pending_watchers = Vec::new();
        for (index, source) in self.sources.iter().enumerate() {
            let id = source_id(index);
            let at = source.type_span;
            let input = match &source.kind {
                SourceKind::Event(tag) => matcher(lock, tag).and_then(|m| {
                    selection.add_event(lock, id, m.clone())?;
                    Ok(Input::Event(m))
                }),
                SourceKind::Resource(tag) => matcher(lock, tag).and_then(|m| {
                    let is_group_member = lock.get(&tag.name).is_some_and(|l| l.group.is_some());
                    if is_group_member && matches!(m, TypeMatcher::AnyInstance(_)) {
                        diagnostics.push(
                            Diagnostic::new(
                                format!(
                                    "`{}` is a generic resource-group member; name its type \
                                     arguments",
                                    tag.name
                                ),
                                at,
                            )
                            .help(
                                "a group delete removes every member at an address, and which \
                                 instantiations exist there isn't tracked",
                            ),
                        );
                    }
                    selection.add_resource(lock, id, m.clone())?;
                    Ok(Input::Resource(m))
                }),
                SourceKind::Table { parent, field } => {
                    TableMatcher::for_field(lock, parent, field.as_str()).map(|m| {
                        if m.container == Container::BigOrderedMap {
                            diagnostics.push(
                                Diagnostic::new("BigOrderedMap sources aren't supported yet", at)
                                    .help(
                                        "small maps keep their entries inside the parent struct, \
                                     which the engine doesn't read yet; use a `resource` source \
                                     on the parent meanwhile",
                                    ),
                            );
                        }
                        pending_watchers.push((id, parent.clone(), field.clone(), m.container));
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
        (selection, inputs, pending_watchers)
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

    #[allow(clippy::too_many_arguments)]
    fn rule(
        &self,
        lock: &Lockfile,
        inputs: &[Input],
        tables: &[TableVar],
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

        let vars = column_vars(key, columns);
        let structs = LockStructs(lock);
        let row_env = Env {
            columns: &vars,
            record: &scope.fields,
            source: rule.on.source.as_str(),
            tables,
            structs: &structs,
        };
        // A key picks the row, so it can't read the row it's picking; it can still
        // read other tables.
        let key_env = Env {
            columns: &[],
            ..row_env
        };
        let compile_expr = |expr: &Expr,
                            env: &Env<'_>,
                            target: &Type,
                            diagnostics: &mut Vec<Diagnostic>|
         -> Option<CompiledExpr> {
            match compile(&expr.text, env, target) {
                Ok(compiled) => Some(CompiledExpr {
                    source: expr.clone(),
                    compiled,
                }),
                Err(e) => {
                    let d = Diagnostic::new(e.message, yaml_span(expr, e.span).or(expr.span));
                    diagnostics.push(match e.help {
                        Some(help) => d.help(help),
                        None => d,
                    });
                    None
                }
            }
        };

        let mut bindings = Vec::new();
        for column_name in key {
            let Some(column) = columns.iter().find(|c| c.name.name == column_name.name) else {
                continue;
            };
            let target = column_type(column);
            if let Some((_, expr)) = rule.key.iter().find(|(n, _)| n.name == column_name.name) {
                if let Some(c) = compile_expr(expr, &key_env, &target, diagnostics) {
                    bindings.push((column_name.name.clone(), c));
                }
                continue;
            }
            match scope.get(&column_name.name) {
                Some(ty) if column.ty.accepts(ty, false) => {
                    let implicit = Expr {
                        text: format!("{}.{}", rule.on.source, column_name),
                        span: rule.on.span,
                    };
                    if let Some(c) = compile_expr(&implicit, &key_env, &target, diagnostics) {
                        bindings.push((column_name.name.clone(), c));
                    }
                }
                found => diagnostics.push(implicit_key_error(&scope, column, found, rule)),
            }
        }

        let when = rule
            .when
            .as_ref()
            .and_then(|w| compile_expr(w, &row_env, &Type::Bool, diagnostics));
        let action = match &rule.action {
            Action::Delete => ResolvedAction::Delete,
            Action::Set(assignments) => ResolvedAction::Set(
                assignments
                    .iter()
                    .filter_map(|(name, expr)| {
                        let column = columns.iter().find(|c| c.name.name == name.name)?;
                        let compiled =
                            compile_expr(expr, &row_env, &column_type(column), diagnostics)?;
                        Some((name.name.clone(), compiled))
                    })
                    .collect(),
            ),
        };

        ResolvedRule {
            source,
            deleted: rule.on.deleted,
            scope,
            key: bindings,
            when,
            action,
        }
    }
}

/// Why a key column without a `key:` mapping can't take the same-named record field.
fn implicit_key_error(
    scope: &Scope,
    column: &Column,
    found: Option<&TypeTag>,
    rule: &Rule,
) -> Diagnostic {
    let name = &column.name;
    let example = format!("map it under the rule's `key`, like `key: {{ {name}: \"...\" }}`");
    if let Some(ty) = found {
        return Diagnostic::new(
            format!(
                "`{}.{name}` is a `{ty}`, but key column `{name}` is `{}`",
                scope.record, column.ty
            ),
            rule.on.span,
        )
        .help(example);
    }
    let why = if scope.is_enum {
        format!(
            "`{}` is an enum with no field `{name}` in every variant",
            scope.record
        )
    } else {
        format!("`{}` has no field `{name}`", scope.record)
    };
    Diagnostic::new(
        format!("this rule doesn't say where key column `{name}` comes from, and {why}"),
        rule.on.span,
    )
    .did_you_mean(name.as_str(), scope.fields.iter().map(|(n, _)| n.as_str()))
    .help_if_none(example)
}

/// The project's tables as a rule's expressions see them: every column of every
/// table, and where its value sits in the stored row (ADR 0019). A `log` table is
/// listed but not readable, so naming one says why rather than "unknown table".
fn table_vars(state: &[StateTable], schemas: &[TableSchema]) -> Vec<TableVar> {
    let column = |c: &SchemaColumn| TableColumn {
        name: c.name.clone(),
        ty: lookup_type(c),
        cell: match &c.from {
            Projection::Row(i) => Cell::At(*i),
            Projection::Field(i, field) => Cell::Field(*i, field.clone()),
            Projection::VariantField { index, field, .. } => Cell::Field(*index, field.clone()),
            Projection::Variant(i) => Cell::Variant(*i),
        },
    };
    state
        .iter()
        .zip(schemas)
        .enumerate()
        .map(|(i, (table, schema))| TableVar {
            name: table.name.name.clone(),
            index: u32::try_from(i).unwrap_or(u32::MAX),
            key: schema
                .key
                .iter()
                .filter_map(|&k| schema.columns.get(k))
                .map(column)
                .collect(),
            columns: schema.columns.iter().map(column).collect(),
            readable: !matches!(table.kind, TableKind::Log { .. }),
        })
        .collect()
}

/// A stored column's type as a lookup reads it, before the `Option` a lookup adds:
/// the column's own type, with a nullable column's `Option` already folded in.
fn lookup_type(column: &SchemaColumn) -> Type {
    column_type(&Column {
        name: Named {
            name: column.name.clone(),
            span: None,
        },
        ty: column.ty,
        nullable: false,
        default: None,
    })
}

/// The row as expressions see it: every column, readable if a new row has a value
/// for it (key columns, defaults, nullables).
fn column_vars(key: &[Named], columns: &[Column]) -> Vec<ColumnVar> {
    columns
        .iter()
        .map(|c| ColumnVar {
            name: c.name.name.clone(),
            ty: column_type(c),
            readable: key.iter().any(|k| k.name == c.name.name)
                || c.default.is_some()
                || c.nullable,
        })
        .collect()
}

/// A column's expression type: `Option<T>` when nullable.
fn column_type(column: &Column) -> Type {
    let int = |t| Type::Int(t);
    let base = match column.ty {
        ColumnType::Bool => Type::Bool,
        ColumnType::U8 => int(IntType::U8),
        ColumnType::U16 => int(IntType::U16),
        ColumnType::U32 => int(IntType::U32),
        ColumnType::U64 => int(IntType::U64),
        ColumnType::U128 => int(IntType::U128),
        ColumnType::U256 => int(IntType::U256),
        ColumnType::I8 => int(IntType::I8),
        ColumnType::I16 => int(IntType::I16),
        ColumnType::I32 => int(IntType::I32),
        ColumnType::I64 => int(IntType::I64),
        ColumnType::I128 => int(IntType::I128),
        ColumnType::I256 => int(IntType::I256),
        ColumnType::Address => Type::Address,
        ColumnType::String => Type::String,
        ColumnType::Bytes => Type::Bytes,
        ColumnType::Json => return Type::Json,
    };
    if column.nullable {
        Type::Option(Box::new(base))
    } else {
        base
    }
}

/// Struct fields for `.field` access in expressions: the fields every value of the
/// type has, with its type arguments substituted.
struct LockStructs<'a>(&'a Lockfile);

impl Structs for LockStructs<'_> {
    fn fields(&self, tag: &StructTag) -> Option<Vec<(Identifier, TypeTag)>> {
        let layout = self.0.get(&tag.name)?;
        Some(
            layout
                .common_fields()
                .into_iter()
                .filter_map(|f| {
                    let ty = f.ty.substitute(&tag.type_args).ok()?;
                    Some((f.name.clone(), ty))
                })
                .collect(),
        )
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

/// What a rule on `input` can read: the record's fields, and the built-ins for its
/// kind. With `deleted`, the scope of a `<source>.deleted` rule, which has only what
/// identifies the row.
#[must_use]
pub fn record_scope(lock: &Lockfile, input: &Input, deleted: bool) -> Scope {
    scope(lock, input, deleted)
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
