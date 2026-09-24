//! Handlers to rules.
//!
//! A handler is written event-first — "when a deposit arrives, here is everything that
//! changes" — and the engine wants the opposite: each state table carrying the rules
//! that fire on it. The scatter pass is the turn between the two.
//!
//! The grouping rule is the whole pass in one line: **every write is grouped by the row
//! it targets and the condition it sits under, and each group becomes one
//! [`Rule`]**. A row named once and assigned three times is one rule with three
//! columns; the same row assigned in both arms of an `if` is two rules with opposite
//! `when`s. That correspondence is what lets the DSL be a frontend and nothing more.

use std::collections::HashMap;

use nineveh_config::{
    Action, Column, ColumnType, Diagnostic, Diagnostics, Expr as CfgExpr, Named, Rule, Span,
    StateTable, TableKind, Trigger,
};
use nineveh_core::{I256, U256, Value};

use crate::ast::{AssignOp, BinOp, ColumnDecl, Expr, Literal, Program, RowRef, Stmt};
use crate::render::{Binding, Ctx, render, render_cond};

/// What the DSL file can refer to that it doesn't declare itself: the project's
/// sources, and the tables `nineveh.yaml` builds.
#[derive(Debug, Clone, Default)]
pub struct Context {
    pub sources: Vec<SourceInfo>,
    pub tables: Vec<TableInfo>,
}

#[derive(Debug, Clone)]
pub struct SourceInfo {
    pub name: String,
    /// Resources and table items have deletes; events don't.
    pub has_deletes: bool,
}

#[derive(Debug, Clone)]
pub struct TableInfo {
    pub name: String,
    /// How many columns identify a row, for checking `get(…)` arity.
    pub key_arity: usize,
    /// A `log` table, which a rule may not read.
    pub is_log: bool,
}

/// One table's declared shape, from `table({ key, columns })`.
struct Declared {
    name: Named,
    key: Vec<Named>,
    columns: Vec<Column>,
}

/// The writes that will become one rule.
struct Group {
    table: String,
    source: Named,
    deleted: bool,
    when: Option<String>,
    when_span: Option<Span>,
    key: Vec<(Named, String)>,
    key_span: Span,
    sets: Vec<(Named, String, Span)>,
    delete: Option<Span>,
}

pub(crate) fn scatter(program: &Program, ctx: &Context) -> Result<Vec<StateTable>, Diagnostics> {
    let mut errors = Vec::new();
    let declared = declare(program, &mut errors);

    // What a rule may read by key: every reduce table this file declares, plus the
    // `mirror` tables from the YAML. Logs are excluded, and named separately so the
    // error can say why (ADR 0019).
    let mut readable: HashMap<String, usize> = declared
        .iter()
        .map(|d| (d.name.name.clone(), d.key.len()))
        .collect();
    for t in &ctx.tables {
        if !t.is_log {
            readable.insert(t.name.clone(), t.key_arity);
        }
    }
    let logs: Vec<String> = ctx
        .tables
        .iter()
        .filter(|t| t.is_log)
        .map(|t| t.name.clone())
        .collect();

    let mut groups: Vec<Group> = Vec::new();
    for handler in &program.handlers {
        let Some(source) = ctx.sources.iter().find(|s| s.name == handler.source.text) else {
            errors.push(
                Diagnostic::new(
                    format!(
                        "nothing in nineveh.yaml is called `{}`",
                        handler.source.text
                    ),
                    Some(handler.source.span),
                )
                .did_you_mean(
                    &handler.source.text,
                    ctx.sources.iter().map(|s| s.name.as_str()),
                )
                .help_if_none("a handler fires on a source declared in `sources:`"),
            );
            continue;
        };
        if handler.deleted && !source.has_deletes {
            errors.push(
                Diagnostic::new(
                    format!("`{}` is an event source, so it has no deletes", source.name),
                    Some(handler.source.span),
                )
                .help("only `resource:` and `table:` sources delete"),
            );
            continue;
        }
        let mut walker = Walker {
            handler_source: &handler.source,
            deleted: handler.deleted,
            param: &handler.param.text,
            declared: &declared,
            readable: &readable,
            logs: &logs,
            scope: Vec::new(),
            groups: &mut groups,
            index: HashMap::new(),
            errors: &mut errors,
        };
        walker.block(&handler.body, &[]);
    }

    let tables = assemble(declared, groups, &mut errors);
    match Diagnostics::from_vec(errors) {
        Some(d) => Err(d),
        None => Ok(tables),
    }
}

/// Check each `table({ … })` declaration and turn it into columns.
fn declare(program: &Program, errors: &mut Vec<Diagnostic>) -> Vec<Declared> {
    let mut out: Vec<Declared> = Vec::new();
    for decl in &program.tables {
        if out.iter().any(|d| d.name.name == decl.name.text) {
            errors.push(
                Diagnostic::new(
                    format!("`{}` is declared twice", decl.name.text),
                    Some(decl.name.span),
                )
                .help("a table is declared once"),
            );
            continue;
        }
        if decl.key.is_empty() {
            errors.push(
                Diagnostic::new(
                    format!("table `{}` has an empty `key`", decl.name.text),
                    Some(decl.name.span),
                )
                .help("name at least one column that identifies a row"),
            );
        }
        let mut columns = Vec::new();
        for column in &decl.key {
            if column.nullable || column.default.is_some() {
                errors.push(
                    Diagnostic::new(
                        format!(
                            "key column `{}` can't be nullable or default",
                            column.name.text
                        ),
                        Some(column.name.span),
                    )
                    .help("a row is identified by its key, so every key column has a value"),
                );
            }
            columns.push(column_of(column, errors));
        }
        for column in &decl.columns {
            if decl.key.iter().any(|k| k.name.text == column.name.text) {
                errors.push(Diagnostic::new(
                    format!("`{}` is already a key column", column.name.text),
                    Some(column.name.span),
                ));
                continue;
            }
            columns.push(column_of(column, errors));
        }
        out.push(Declared {
            name: named(&decl.name.text, decl.name.span),
            key: decl
                .key
                .iter()
                .map(|c| named(&c.name.text, c.name.span))
                .collect(),
            columns,
        });
    }
    out
}

fn column_of(decl: &ColumnDecl, errors: &mut Vec<Diagnostic>) -> Column {
    let default = decl
        .default
        .as_ref()
        .and_then(|literal| match default_value(literal, decl.ty) {
            Ok(v) => Some(v),
            Err(reason) => {
                errors.push(Diagnostic::new(
                    format!("invalid default for this {} column: {reason}", decl.ty),
                    Some(decl.ty_span),
                ));
                None
            }
        });
    Column {
        name: named(&decl.name.text, decl.name.span),
        ty: decl.ty,
        nullable: decl.nullable,
        default,
    }
}

/// Walks one handler's body, collecting writes.
struct Walker<'a> {
    handler_source: &'a crate::ast::Name,
    deleted: bool,
    param: &'a str,
    declared: &'a [Declared],
    readable: &'a HashMap<String, usize>,
    logs: &'a [String],
    scope: Vec<(String, Binding)>,
    groups: &'a mut Vec<Group>,
    /// Group identity to its index in `groups`: the same row under the same condition
    /// is the same rule, however many statements write to it.
    index: HashMap<(String, String, String), usize>,
    errors: &'a mut Vec<Diagnostic>,
}

/// Whether a block runs off its end, or always stops first.
#[derive(PartialEq, Eq, Clone, Copy)]
enum Flow {
    Falls,
    Returns,
}

impl Walker<'_> {
    fn block(&mut self, stmts: &[Stmt], path: &[String]) -> Flow {
        let depth = self.scope.len();
        let mut path = path.to_vec();
        let mut flow = Flow::Falls;
        for stmt in stmts {
            if flow == Flow::Returns {
                self.errors.push(
                    Diagnostic::new("this can never run".to_owned(), Some(stmt_span(stmt)))
                        .help("an earlier `return` always stops the handler first"),
                );
                break;
            }
            match stmt {
                // Anything after this in the block is reported unreachable above.
                Stmt::Return { .. } => flow = Flow::Returns,
                Stmt::Let { name, value } => {
                    self.declare_binding(&name.text, name.span, Binding::Value(value.clone()));
                }
                Stmt::Row { name, table, keys } => {
                    self.check_table(table);
                    // The keys are rendered where the row is written, so a binding
                    // that's never written costs nothing and reports nothing.
                    self.declare_binding(
                        &name.text,
                        name.span,
                        Binding::Row {
                            table: table.text.clone(),
                            keys: keys.clone(),
                        },
                    );
                }
                Stmt::Assign {
                    row,
                    column,
                    op,
                    value,
                } => self.assign(row, column, *op, value, &path),
                Stmt::Delete { row, span } => self.delete(row, *span, &path),
                Stmt::If {
                    cond,
                    then,
                    otherwise,
                } => {
                    let Some((yes, no)) = self.conditions(cond, &path) else {
                        continue;
                    };
                    let then_flow = self.block(then, &yes);
                    let else_flow = if otherwise.is_empty() {
                        Flow::Falls
                    } else {
                        self.block(otherwise, &no)
                    };
                    // A branch that always returns narrows what follows it.
                    match (then_flow, else_flow) {
                        (Flow::Returns, Flow::Returns) => flow = Flow::Returns,
                        (Flow::Returns, Flow::Falls) => path = no,
                        (Flow::Falls, Flow::Returns) => path = yes,
                        (Flow::Falls, Flow::Falls) => {}
                    }
                }
            }
        }
        self.scope.truncate(depth);
        flow
    }

    /// The path condition with `cond` true, and with it false.
    fn conditions(&mut self, cond: &Expr, path: &[String]) -> Option<(Vec<String>, Vec<String>)> {
        let ctx = self.ctx(None);
        let yes = render_cond(cond, &ctx, false);
        let no = render_cond(cond, &ctx, true);
        match (yes, no) {
            (Ok(yes), Ok(no)) => {
                let mut a = path.to_vec();
                a.push(yes);
                let mut b = path.to_vec();
                b.push(no);
                Some((a, b))
            }
            (Err(e), _) | (_, Err(e)) => {
                self.errors.push(e);
                None
            }
        }
    }

    fn ctx<'c>(&'c self, target: Option<&'c str>) -> Ctx<'c> {
        Ctx {
            param: self.param,
            source: &self.handler_source.text,
            scope: &self.scope,
            readable: self.readable,
            logs: self.logs,
            target,
        }
    }

    fn declare_binding(&mut self, name: &str, span: Span, binding: Binding) {
        if self.scope.iter().any(|(n, _)| n == name) {
            self.errors.push(
                Diagnostic::new(format!("`{name}` is already defined here"), Some(span))
                    .help("each name is defined once in a handler"),
            );
            return;
        }
        self.scope.push((name.to_owned(), binding));
    }

    fn check_table(&mut self, table: &crate::ast::Name) -> bool {
        if self.declared.iter().any(|d| d.name.name == table.text) {
            return true;
        }
        let diagnostic =
            if self.readable.contains_key(&table.text) || self.logs.contains(&table.text) {
                Diagnostic::new(
                    format!("`{}` isn't written by a rule", table.text),
                    Some(table.span),
                )
                .help("its source writes it; only a `table({ … })` here takes writes")
            } else {
                Diagnostic::new(
                    format!("nothing here is called `{}`", table.text),
                    Some(table.span),
                )
                .did_you_mean(
                    &table.text,
                    self.declared.iter().map(|d| d.name.name.as_str()),
                )
            };
        self.errors.push(diagnostic);
        false
    }

    /// Resolve a row reference to its table and rendered key expressions.
    fn row(&mut self, row: &RowRef) -> Option<(String, Vec<String>, Span, Option<String>)> {
        let (table, keys, span, target) = match row {
            RowRef::Bound(name) => {
                let Some((_, Binding::Row { table, keys })) =
                    self.scope.iter().rev().find(|(n, _)| *n == name.text)
                else {
                    self.errors.push(
                        Diagnostic::new(format!("`{}` isn't a row", name.text), Some(name.span))
                            .help("name a row first: `const b = <table>.row(<key>)`"),
                    );
                    return None;
                };
                (
                    table.clone(),
                    keys.clone(),
                    name.span,
                    Some(name.text.clone()),
                )
            }
            RowRef::Inline { table, keys, span } => {
                if !self.check_table(table) {
                    return None;
                }
                (table.text.clone(), keys.clone(), *span, None)
            }
        };
        let ctx = self.ctx(target.as_deref());
        let mut rendered = Vec::new();
        for key in &keys {
            match render(key, &ctx) {
                Ok(text) => rendered.push(text),
                Err(e) => {
                    self.errors.push(e);
                    return None;
                }
            }
        }
        Some((table, rendered, span, target))
    }

    fn assign(
        &mut self,
        row: &RowRef,
        column: &crate::ast::Name,
        op: AssignOp,
        value: &Expr,
        path: &[String],
    ) {
        let Some((table, keys, span, target)) = self.row(row) else {
            return;
        };
        let Some(decl) = self.declared.iter().find(|d| d.name.name == table) else {
            return;
        };
        let Some(col) = decl.columns.iter().find(|c| c.name.name == column.text) else {
            self.errors.push(
                Diagnostic::new(
                    format!("`{table}` has no column `{}`", column.text),
                    Some(column.span),
                )
                .did_you_mean(
                    &column.text,
                    decl.columns.iter().map(|c| c.name.name.as_str()),
                ),
            );
            return;
        };
        if decl.key.iter().any(|k| k.name == col.name.name) {
            self.errors.push(
                Diagnostic::new(
                    format!(
                        "`{}` identifies the row, so a rule can't set it",
                        column.text
                    ),
                    Some(column.span),
                )
                .help(format!("pass it to `{table}.row(…)` instead")),
            );
            return;
        }
        let ctx = self.ctx(target.as_deref());
        // `+=` and `-=` read the row's current value, which is what a fold is.
        let rendered = match op {
            AssignOp::Set => render(value, &ctx),
            AssignOp::Add | AssignOp::Sub => {
                let sym = if matches!(op, AssignOp::Add) {
                    "+"
                } else {
                    "-"
                };
                crate::render::render_at(value, &ctx, BinOp::Add.power() + 1)
                    .map(|v| format!("row.{} {sym} {v}", column.text))
            }
        };
        let text = match rendered {
            Ok(text) => text,
            Err(e) => {
                self.errors.push(e);
                return;
            }
        };
        let g = self.group(&table, &keys, span, path);
        if self.groups[g]
            .sets
            .iter()
            .any(|(name, _, _)| name.name == column.text)
        {
            self.errors.push(
                Diagnostic::new(
                    format!("`{}` is set twice for the same row", column.text),
                    Some(column.span),
                )
                .help("one rule sets each column once; combine them into one expression"),
            );
            return;
        }
        self.groups[g]
            .sets
            .push((named(&column.text, column.span), text, column.span));
    }

    fn delete(&mut self, row: &RowRef, span: Span, path: &[String]) {
        let Some((table, keys, key_span, _)) = self.row(row) else {
            return;
        };
        if self.declared.iter().all(|d| d.name.name != table) {
            return;
        }
        let g = self.group(&table, &keys, key_span, path);
        self.groups[g].delete = Some(span);
    }

    /// The group every write to this row under this condition belongs to.
    fn group(&mut self, table: &str, keys: &[String], key_span: Span, path: &[String]) -> usize {
        let when = (!path.is_empty()).then(|| path.join(" && "));
        let id = (
            table.to_owned(),
            keys.join(","),
            when.clone().unwrap_or_default(),
        );
        if let Some(&i) = self.index.get(&id) {
            return i;
        }
        // Only a declared table reaches here; `check_table` rejected the rest.
        let declared_key = self
            .declared
            .iter()
            .find(|d| d.name.name == table)
            .map_or(&[][..], |d| d.key.as_slice());
        if keys.len() != declared_key.len() {
            self.errors.push(Diagnostic::new(
                format!(
                    "`{table}` is keyed by {} column{}, but {} {} given",
                    declared_key.len(),
                    if declared_key.len() == 1 { "" } else { "s" },
                    keys.len(),
                    if keys.len() == 1 { "was" } else { "were" }
                ),
                Some(key_span),
            ));
        }
        let key: Vec<(Named, String)> = declared_key
            .iter()
            .cloned()
            .zip(keys.iter().cloned())
            .collect();
        self.groups.push(Group {
            table: table.to_owned(),
            source: named(&self.handler_source.text, self.handler_source.span),
            deleted: self.deleted,
            when,
            when_span: Some(key_span),
            key,
            key_span,
            sets: Vec::new(),
            delete: None,
        });
        let i = self.groups.len() - 1;
        self.index.insert(id, i);
        i
    }
}

/// Hand the collected groups back to their tables, in the order they were written.
fn assemble(
    declared: Vec<Declared>,
    groups: Vec<Group>,
    errors: &mut Vec<Diagnostic>,
) -> Vec<StateTable> {
    let mut by_table: HashMap<String, Vec<Rule>> = HashMap::new();
    for group in groups {
        if let (Some(delete), false) = (group.delete, group.sets.is_empty()) {
            errors.push(
                Diagnostic::new(
                    "this row is both written and deleted".to_owned(),
                    Some(delete),
                )
                .help("a rule either sets columns or deletes the row"),
            );
            continue;
        }
        let action = if group.delete.is_some() {
            Action::Delete
        } else if group.sets.is_empty() {
            continue;
        } else {
            Action::Set(
                group
                    .sets
                    .into_iter()
                    .map(|(name, text, span)| (name, expr_text(text, span)))
                    .collect(),
            )
        };
        by_table.entry(group.table).or_default().push(Rule {
            on: Trigger {
                source: group.source,
                deleted: group.deleted,
                span: None,
            },
            when: group
                .when
                .map(|text| expr_text(text, group.when_span.unwrap_or(group.key_span))),
            key: group
                .key
                .into_iter()
                .map(|(name, text)| (name, expr_text(text, group.key_span)))
                .collect(),
            action,
        });
    }
    declared
        .into_iter()
        .map(|d| {
            let rules = by_table.remove(&d.name.name).unwrap_or_default();
            StateTable {
                name: d.name,
                kind: TableKind::Reduce {
                    key: d.key,
                    columns: d.columns,
                    rules,
                },
            }
        })
        .collect()
}

fn expr_text(text: String, span: Span) -> CfgExpr {
    CfgExpr {
        text,
        span: Some(span),
    }
}

fn named(name: &str, span: Span) -> Named {
    Named {
        name: name.to_owned(),
        span: Some(span),
    }
}

fn stmt_span(stmt: &Stmt) -> Span {
    match stmt {
        Stmt::Let { name, .. } | Stmt::Row { name, .. } => name.span,
        Stmt::Assign { row, .. } => row.span(),
        Stmt::Delete { span, .. } | Stmt::Return { span } => *span,
        Stmt::If { cond, .. } => cond.span,
    }
}

/// A column default, converted against the column's type.
fn default_value(literal: &Literal, ty: ColumnType) -> Result<Value, String> {
    let digits = |negative: bool, digits: &str| -> String {
        if negative {
            format!("-{digits}")
        } else {
            digits.to_owned()
        }
    };
    match (literal, ty) {
        (_, ColumnType::Json) => Err("json columns can't have a default".to_owned()),
        (Literal::Bool(b), ColumnType::Bool) => Ok(Value::Bool(*b)),
        (Literal::Str(s), ColumnType::String) => Ok(Value::String(s.clone())),
        (Literal::Str(s), ColumnType::Address) => s
            .parse()
            .map(Value::Address)
            .map_err(|e: nineveh_core::InvalidAddress| e.to_string()),
        (Literal::Str(s), ColumnType::Bytes) => hex_bytes(s)
            .map(Value::Bytes)
            .ok_or_else(|| "bytes are `0x` followed by an even number of hex digits".to_owned()),
        (
            Literal::Int {
                digits: d,
                negative,
            },
            ty,
        ) => integer(&digits(*negative, d), ty),
        (Literal::Bool(_) | Literal::Str(_), _) => Err("expected a number".to_owned()),
    }
}

fn integer(text: &str, ty: ColumnType) -> Result<Value, String> {
    let text = text.replace('_', "");
    let range = || "out of range for this column".to_owned();
    match ty {
        ColumnType::U8 => text.parse().map(Value::U8).map_err(|_| range()),
        ColumnType::U16 => text.parse().map(Value::U16).map_err(|_| range()),
        ColumnType::U32 => text.parse().map(Value::U32).map_err(|_| range()),
        ColumnType::U64 => text.parse().map(Value::U64).map_err(|_| range()),
        ColumnType::U128 => text.parse().map(Value::U128).map_err(|_| range()),
        ColumnType::U256 => text.parse::<U256>().map(Value::U256).map_err(|_| range()),
        ColumnType::I8 => text.parse().map(Value::I8).map_err(|_| range()),
        ColumnType::I16 => text.parse().map(Value::I16).map_err(|_| range()),
        ColumnType::I32 => text.parse().map(Value::I32).map_err(|_| range()),
        ColumnType::I64 => text.parse().map(Value::I64).map_err(|_| range()),
        ColumnType::I128 => text.parse().map(Value::I128).map_err(|_| range()),
        ColumnType::I256 => text.parse::<I256>().map(Value::I256).map_err(|_| range()),
        ColumnType::Bool => Err("expected true or false".to_owned()),
        ColumnType::String => Err("expected a string".to_owned()),
        ColumnType::Bytes => Err("expected `0x` followed by hex digits".to_owned()),
        ColumnType::Address => Err("write addresses as quoted strings, like \"0x1\"".to_owned()),
        ColumnType::Json => Err("json columns can't have a default".to_owned()),
    }
}

fn hex_bytes(s: &str) -> Option<Vec<u8>> {
    let hex = s.strip_prefix("0x")?;
    if hex.len() % 2 != 0 {
        return None;
    }
    (0..hex.len() / 2)
        .map(|i| u8::from_str_radix(&hex[i * 2..i * 2 + 2], 16).ok())
        .collect()
}
