//! Typechecking: an untyped tree plus an environment gives a typed [`Compiled`]
//! expression, with every name resolved to an input slot.

use nineveh_core::{Identifier, StructTag, TypeTag, Value};

use crate::error::{ExprError, Span};
use crate::eval::{Compiled, Node};
use crate::num::Int;
use crate::syntax::{self, BinOp, Expr, ExprKind, UnOp};
use crate::types::{IntType, Type, mentions_object};

/// Looks up struct fields for `.field` access. `nineveh-config` implements this over
/// `nineveh.lock`, returning the fields every value of the type has (for an enum, the
/// ones common to all variants) with type arguments substituted.
pub trait Structs {
    fn fields(&self, tag: &StructTag) -> Option<Vec<(Identifier, TypeTag)>>;
}

/// A row column an expression may read.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ColumnVar {
    pub name: String,
    /// `Option<T>` for nullable columns.
    pub ty: Type,
    /// Whether a new row has a value for it: key columns, columns with a default, and
    /// nullable columns do. Reading any other column could find no value, so it's a
    /// type error.
    pub readable: bool,
}

/// Another state table an expression may read a row of: `holders[owner].balance`.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct TableVar {
    pub name: String,
    /// The table's index in the project's state, passed to [`crate::Tables::row`].
    pub index: u32,
    /// The key columns, in key order.
    pub key: Vec<TableColumn>,
    /// Every column, key columns included.
    pub columns: Vec<TableColumn>,
    /// Whether the table keeps the rows a lookup reads. `log` tables don't: they're
    /// append-only history, not state.
    pub readable: bool,
}

/// A column of another table, and where its value sits in that table's stored row.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct TableColumn {
    pub name: String,
    /// The column's type. Reading it gives an `Option` of this, since the row may not
    /// be there.
    pub ty: Type,
    pub cell: Cell,
}

/// Where a column's value sits in a table's stored row.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Cell {
    /// The row's value at this index.
    At(usize),
    /// A field of the struct or enum value at this index.
    Field(usize, Identifier),
    /// The variant name of the enum value at this index.
    Variant(usize),
}

/// Everything an expression can refer to.
#[derive(Clone, Copy)]
pub struct Env<'a> {
    /// The row's columns, in the order values are supplied at evaluation.
    pub columns: &'a [ColumnVar],
    /// The record's fields, in the order values are supplied at evaluation.
    pub record: &'a [(Identifier, TypeTag)],
    /// The record's source name, usable as a qualifier: `deposits.amount`.
    pub source: &'a str,
    /// The project's other state tables, readable a row at a time.
    pub tables: &'a [TableVar],
    pub structs: &'a dyn Structs,
}

impl std::fmt::Debug for Env<'_> {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("Env")
            .field("columns", &self.columns)
            .field("record", &self.record)
            .field("source", &self.source)
            .field("tables", &self.tables)
            .finish_non_exhaustive()
    }
}

/// Parse and typecheck `text` as a value of type `target`.
///
/// `target` is the column type (`Option<T>` for a nullable column), or `bool` for a
/// `when` condition. A `T` is accepted where `Option<T>` is expected, and anything
/// where `json` is.
///
/// # Errors
///
/// If `text` doesn't parse, refers to something `env` doesn't have, or doesn't have
/// type `target`. The error's span is within `text`.
pub fn compile(text: &str, env: &Env<'_>, target: &Type) -> Result<Compiled, ExprError> {
    let expr = syntax::parse(text)?;
    let checker = Checker { env };
    let typed = checker.check(&expr, Some(target))?;
    Ok(Compiled::new(typed.node, typed.ty))
}

struct Typed {
    node: Node,
    ty: Type,
}

struct Checker<'a, 'e> {
    env: &'a Env<'e>,
}

impl Checker<'_, '_> {
    /// Infer `e`'s type with `expected` as a hint, then coerce it to `expected`.
    fn check(&self, e: &Expr, expected: Option<&Type>) -> Result<Typed, ExprError> {
        let typed = self.infer(e, expected)?;
        let Some(expected) = expected else {
            return Ok(typed);
        };
        if typed.ty == *expected {
            return Ok(typed);
        }
        match expected {
            Type::Json => Ok(Typed {
                node: typed.node,
                ty: Type::Json,
            }),
            Type::Option(inner) if typed.ty == **inner => Ok(Typed {
                node: Node::Some(Box::new(typed.node)),
                ty: expected.clone(),
            }),
            _ => Err(mismatch(expected, &typed.ty, e.span)),
        }
    }

    fn infer(&self, e: &Expr, expected: Option<&Type>) -> Result<Typed, ExprError> {
        match &e.kind {
            ExprKind::Int {
                magnitude,
                negative,
                suffix,
            } => {
                let ty = suffix
                    .or_else(|| expected.and_then(literal_int_hint))
                    .unwrap_or(IntType::U64);
                let int = Int::from_u256(*magnitude);
                let int = if *negative { int.neg() } else { int };
                let value = int.to_value(ty).map_err(|_| {
                    ExprError::new(format!("this number doesn't fit in {ty}"), e.span)
                })?;
                Ok(Typed {
                    node: Node::Const(value),
                    ty: Type::Int(ty),
                })
            }
            ExprKind::Bool(b) => Ok(constant(Value::Bool(*b), Type::Bool)),
            ExprKind::Str(s) => Ok(constant(Value::String(s.clone()), Type::String)),
            ExprKind::Address(a) => Ok(constant(Value::Address(*a), Type::Address)),
            ExprKind::Null => match expected {
                Some(ty @ Type::Option(_)) => Ok(constant(Value::Option(None), ty.clone())),
                _ => Err(
                    ExprError::new("`null` needs a nullable context", e.span).help(
                        "`null` can be assigned to a nullable column or compared with an option",
                    ),
                ),
            },
            ExprKind::Name(name) => self.name(name, e.span),
            ExprKind::Field(base, field, field_span) => self.field(base, field, *field_span),
            ExprKind::Index(base, _, _) => {
                let table = match &base.kind {
                    ExprKind::Name(name) => self.table(name).map(|t| t.name.as_str()),
                    _ => None,
                };
                Err(match table {
                    Some(name) => ExprError::new(
                        format!("a row of `{name}` isn't a value on its own"),
                        e.span,
                    )
                    .help(format!("read one of its columns, like `{name}[...].x`")),
                    None => ExprError::new("only a state table can be indexed", e.span),
                })
            }
            ExprKind::Unary(op, operand) => self.unary(*op, operand, e.span, expected),
            ExprKind::Binary(op, lhs, rhs) => self.binary(*op, lhs, rhs, e.span, expected),
            ExprKind::If(cond, then, otherwise) => {
                let cond = self.check(cond, Some(&Type::Bool))?;
                // Type the branch that isn't a bare literal first, so `if c then 0 else x`
                // takes x's type.
                let (then, otherwise) = if is_polymorphic(then) && !is_polymorphic(otherwise) {
                    let otherwise = self.check(otherwise, expected)?;
                    let then = self.check(then, Some(&otherwise.ty))?;
                    (then, otherwise)
                } else {
                    let then = self.check(then, expected)?;
                    let otherwise = self.check(otherwise, Some(&then.ty))?;
                    (then, otherwise)
                };
                Ok(Typed {
                    ty: then.ty,
                    node: Node::If(
                        Box::new(cond.node),
                        Box::new(then.node),
                        Box::new(otherwise.node),
                    ),
                })
            }
            ExprKind::Call(name, name_span, args) => {
                self.call(name, *name_span, args, e.span, expected)
            }
        }
    }

    /// A bare name: a column or a record field, never both.
    fn name(&self, name: &str, span: Span) -> Result<Typed, ExprError> {
        let column = self.env.columns.iter().position(|c| c.name == name);
        let field = self.env.record.iter().position(|(n, _)| n.as_str() == name);
        match (column, field) {
            (Some(_), Some(_)) => Err(ExprError::new(
                format!("`{name}` is both a column and a field of the record"),
                span,
            )
            .help(format!(
                "write `row.{name}` for the row's value or `{}.{name}` for the record's",
                self.env.source
            ))),
            (Some(i), None) => self.column(i, span),
            (None, Some(i)) => self.record_field(i, span),
            (None, None) => {
                let e = ExprError::new(format!("unknown name `{name}`"), span);
                if let Some(table) = self.table(name) {
                    let names: Vec<&str> = table.key.iter().map(|k| k.name.as_str()).collect();
                    return Err(e.help(format!(
                        "`{name}` is a table; read a row of it, like `{name}[{}].x`",
                        names.join(", ")
                    )));
                }
                let candidates = self
                    .env
                    .columns
                    .iter()
                    .map(|c| c.name.as_str())
                    .chain(self.env.record.iter().map(|(n, _)| n.as_str()));
                Err(match closest(name, candidates) {
                    Some(best) => e.help(format!("did you mean `{best}`?")),
                    None if matches!(name, "row" | "tx") || name == self.env.source => e.help(
                        format!("`{name}` is a qualifier; read a field of it, like `{name}.x`"),
                    ),
                    None => e,
                })
            }
        }
    }

    fn column(&self, index: usize, span: Span) -> Result<Typed, ExprError> {
        let column = &self.env.columns[index];
        if !column.readable {
            return Err(ExprError::new(
                format!(
                    "`{}` has no default, so a new row has no value to read",
                    column.name
                ),
                span,
            )
            .help(format!(
                "give `{}` a `default`, or make it `nullable: true`",
                column.name
            )));
        }
        Ok(Typed {
            node: Node::Column(index),
            ty: column.ty.clone(),
        })
    }

    fn record_field(&self, index: usize, span: Span) -> Result<Typed, ExprError> {
        let (name, tag) = &self.env.record[index];
        let ty = readable_type(tag, name.as_str(), span)?;
        Ok(Typed {
            node: Node::Record {
                index,
                flatten: mentions_object(tag).then(|| tag.clone()),
            },
            ty,
        })
    }

    fn table(&self, name: &str) -> Option<&TableVar> {
        self.env.tables.iter().find(|t| t.name == name)
    }

    /// `holders[owner].balance`: a column of another table's row, `null` when that
    /// table has no such row.
    fn lookup(
        &self,
        table: &TableVar,
        keys: &[Expr],
        key_span: Span,
        column: &str,
        column_span: Span,
    ) -> Result<Typed, ExprError> {
        if !table.readable {
            return Err(ExprError::new(
                format!(
                    "`{}` is a log table, so it has no rows to look up",
                    table.name
                ),
                key_span,
            )
            .help("logs are append-only history; look up a state or mirror table"));
        }
        if keys.len() != table.key.len() {
            let names: Vec<&str> = table.key.iter().map(|k| k.name.as_str()).collect();
            return Err(ExprError::new(
                format!(
                    "`{}` is keyed by {} column{}, got {}",
                    table.name,
                    table.key.len(),
                    if table.key.len() == 1 { "" } else { "s" },
                    keys.len()
                ),
                key_span,
            )
            .help(format!("write `{}[{}]`", table.name, names.join(", "))));
        }
        let Some(found) = table.columns.iter().find(|c| c.name == column) else {
            let e = ExprError::new(
                format!("`{}` has no column `{column}`", table.name),
                column_span,
            );
            let names = table.columns.iter().map(|c| c.name.as_str());
            return Err(
                match closest(column, table.columns.iter().map(|c| c.name.as_str())) {
                    Some(best) => e.help(format!("did you mean `{best}`?")),
                    None => e.help(format!(
                        "its columns are: {}",
                        names.collect::<Vec<_>>().join(", ")
                    )),
                },
            );
        };
        let mut key = Vec::with_capacity(keys.len());
        for (expr, column) in keys.iter().zip(&table.key) {
            key.push(self.check(expr, Some(&column.ty))?.node);
        }
        Ok(Typed {
            node: Node::Lookup {
                table: table.index,
                key,
                cell: found.cell.clone(),
                address: found.ty == Type::Address,
            },
            ty: Type::Option(Box::new(found.ty.clone())),
        })
    }

    fn field(&self, base: &Expr, field: &str, field_span: Span) -> Result<Typed, ExprError> {
        // A column of another table's row: `holders[owner].balance`.
        if let ExprKind::Index(indexed, keys, key_span) = &base.kind
            && let ExprKind::Name(name) = &indexed.kind
        {
            let table = self.table(name).ok_or_else(|| {
                let e = ExprError::new(format!("unknown table `{name}`"), indexed.span);
                match closest(name, self.env.tables.iter().map(|t| t.name.as_str())) {
                    Some(best) => e.help(format!("did you mean `{best}`?")),
                    None => e,
                }
            })?;
            return self.lookup(table, keys, *key_span, field, field_span);
        }

        // Qualified names: `row.x`, `tx.version`, `<source>.x`.
        if let ExprKind::Name(qualifier) = &base.kind {
            let span = base.span.to(field_span);
            if qualifier == "row" {
                let i = self
                    .env
                    .columns
                    .iter()
                    .position(|c| c.name == field)
                    .ok_or_else(|| {
                        ExprError::new(format!("the row has no column `{field}`"), field_span)
                    })?;
                return self.column(i, span);
            }
            if qualifier == "tx" {
                return match field {
                    "version" => Ok(Typed {
                        node: Node::TxVersion,
                        ty: Type::Int(IntType::U64),
                    }),
                    "timestamp" => Ok(Typed {
                        node: Node::TxTimestamp,
                        ty: Type::Int(IntType::U64),
                    }),
                    _ => Err(
                        ExprError::new(format!("`tx` has no field `{field}`"), field_span)
                            .help("`tx.version` and `tx.timestamp` (microseconds) are available"),
                    ),
                };
            }
            if qualifier == self.env.source {
                let i = self
                    .env
                    .record
                    .iter()
                    .position(|(n, _)| n.as_str() == field)
                    .ok_or_else(|| {
                        ExprError::new(
                            format!("`{}` records have no field `{field}`", self.env.source),
                            field_span,
                        )
                    })?;
                return self.record_field(i, span);
            }
        }

        let base_typed = self.infer(base, None)?;
        let Type::Struct(tag) = &base_typed.ty else {
            return Err(ExprError::new(
                format!("a {} has no fields", base_typed.ty),
                field_span,
            ));
        };
        let fields = self.env.structs.fields(tag).unwrap_or_default();
        let Some((name, field_tag)) = fields.iter().find(|(n, _)| n.as_str() == field) else {
            return Err(ExprError::new(
                format!("`{}` has no field `{field}` every value has", tag.name.name),
                field_span,
            ));
        };
        let ty = readable_type(field_tag, field, field_span)?;
        Ok(Typed {
            node: Node::Field {
                base: Box::new(base_typed.node),
                name: name.clone(),
                flatten: mentions_object(field_tag).then(|| field_tag.clone()),
            },
            ty,
        })
    }

    fn unary(
        &self,
        op: UnOp,
        operand: &Expr,
        span: Span,
        expected: Option<&Type>,
    ) -> Result<Typed, ExprError> {
        match op {
            UnOp::Not => {
                let operand = self.check(operand, Some(&Type::Bool))?;
                Ok(Typed {
                    node: Node::Not(Box::new(operand.node)),
                    ty: Type::Bool,
                })
            }
            UnOp::Neg => {
                let operand = self.infer(operand, expected)?;
                let ty = int_operand(&operand.ty, "-", span)?;
                if !ty.is_signed() {
                    return Err(
                        ExprError::new(format!("can't negate an unsigned {ty}"), span)
                            .help("subtract from a signed value, or cast with `i128(x)`"),
                    );
                }
                Ok(Typed {
                    node: Node::Neg(Box::new(operand.node), ty, span),
                    ty: Type::Int(ty),
                })
            }
        }
    }

    fn binary(
        &self,
        op: BinOp,
        lhs: &Expr,
        rhs: &Expr,
        span: Span,
        expected: Option<&Type>,
    ) -> Result<Typed, ExprError> {
        match op {
            BinOp::And | BinOp::Or => {
                let lhs = self.check(lhs, Some(&Type::Bool))?;
                let rhs = self.check(rhs, Some(&Type::Bool))?;
                let node = if op == BinOp::And {
                    Node::And(Box::new(lhs.node), Box::new(rhs.node))
                } else {
                    Node::Or(Box::new(lhs.node), Box::new(rhs.node))
                };
                Ok(Typed {
                    node,
                    ty: Type::Bool,
                })
            }
            _ => {
                // Arithmetic operands take the result's type; comparison operands don't.
                let hint = if op.is_comparison() {
                    None
                } else {
                    expected.filter(|t| matches!(t, Type::Int(_)))
                };
                let needs_int = (!matches!(op, BinOp::Eq | BinOp::Ne)).then_some(op.symbol());
                let (lhs, rhs) = self.operands(lhs, rhs, hint, needs_int)?;
                if op.is_comparison() {
                    comparison(op, lhs, rhs, span)
                } else {
                    let ty = int_operand(&lhs.ty, op.symbol(), span)?;
                    Ok(Typed {
                        node: Node::Arith(op, ty, Box::new(lhs.node), Box::new(rhs.node), span),
                        ty: Type::Int(ty),
                    })
                }
            }
        }
    }

    /// Type two operands that must agree: the one that isn't a bare literal goes first
    /// and fixes the other's type. With `needs_int` (the operator's symbol), the first
    /// operand must be an integer, so `owner + 1` says why rather than blaming `1`.
    fn operands(
        &self,
        lhs: &Expr,
        rhs: &Expr,
        hint: Option<&Type>,
        needs_int: Option<&str>,
    ) -> Result<(Typed, Typed), ExprError> {
        let lhs_first = !is_polymorphic(lhs) || is_polymorphic(rhs);
        let (first, second) = if lhs_first { (lhs, rhs) } else { (rhs, lhs) };
        let first_typed = self.check(first, hint)?;
        if let Some(op) = needs_int {
            int_operand(&first_typed.ty, op, first.span)?;
        }
        // `x == null` compares an option with nothing.
        let second_typed = self.check(second, Some(&first_typed.ty))?;
        Ok(if lhs_first {
            (first_typed, second_typed)
        } else {
            (second_typed, first_typed)
        })
    }

    fn call(
        &self,
        name: &str,
        name_span: Span,
        args: &[Expr],
        span: Span,
        expected: Option<&Type>,
    ) -> Result<Typed, ExprError> {
        let arity = |n: usize| -> Result<(), ExprError> {
            if args.len() == n {
                Ok(())
            } else {
                Err(ExprError::new(
                    format!(
                        "`{name}` takes {n} argument{}, got {}",
                        if n == 1 { "" } else { "s" },
                        args.len()
                    ),
                    span,
                ))
            }
        };

        if let Some(target) = IntType::parse(name) {
            arity(1)?;
            let arg = self.infer(&args[0], None)?;
            int_operand(&arg.ty, name, args[0].span)?;
            return Ok(Typed {
                node: Node::Cast(target, Box::new(arg.node), span),
                ty: Type::Int(target),
            });
        }

        match name {
            "min" | "max" => {
                arity(2)?;
                let hint = expected.filter(|t| matches!(t, Type::Int(_)));
                let (a, b) = self.operands(&args[0], &args[1], hint, Some(name))?;
                let ty = int_operand(&a.ty, name, span)?;
                Ok(Typed {
                    node: Node::MinMax(name == "max", Box::new(a.node), Box::new(b.node)),
                    ty: Type::Int(ty),
                })
            }
            "abs" => {
                arity(1)?;
                let arg = self.infer(&args[0], expected)?;
                let ty = int_operand(&arg.ty, name, span)?;
                if !ty.is_signed() {
                    return Err(ExprError::new(
                        format!("`abs` of an unsigned {ty} does nothing"),
                        span,
                    ));
                }
                Ok(Typed {
                    node: Node::Abs(Box::new(arg.node), ty, span),
                    ty: Type::Int(ty),
                })
            }
            "is_some" | "is_none" => {
                arity(1)?;
                let arg = self.infer(&args[0], None)?;
                if !matches!(arg.ty, Type::Option(_)) {
                    return Err(ExprError::new(
                        format!("`{name}` needs an option, found {}", arg.ty),
                        args[0].span,
                    ));
                }
                Ok(Typed {
                    node: Node::IsSome(name == "is_none", Box::new(arg.node)),
                    ty: Type::Bool,
                })
            }
            "unwrap_or" => {
                arity(2)?;
                let option = self.infer(&args[0], None)?;
                let Type::Option(inner) = &option.ty else {
                    return Err(ExprError::new(
                        format!("`unwrap_or` needs an option, found {}", option.ty),
                        args[0].span,
                    ));
                };
                let default = self.check(&args[1], Some(inner))?;
                Ok(Typed {
                    ty: (**inner).clone(),
                    node: Node::UnwrapOr(Box::new(option.node), Box::new(default.node)),
                })
            }
            _ => Err(unknown_function(name, name_span)),
        }
    }
}

fn unknown_function(name: &str, span: Span) -> ExprError {
    const KNOWN: [&str; 18] = [
        "min",
        "max",
        "abs",
        "is_some",
        "is_none",
        "unwrap_or",
        "u8",
        "u16",
        "u32",
        "u64",
        "u128",
        "u256",
        "i8",
        "i16",
        "i32",
        "i64",
        "i128",
        "i256",
    ];
    let e = ExprError::new(format!("unknown function `{name}`"), span);
    match closest(name, KNOWN) {
        Some(best) => e.help(format!("did you mean `{best}`?")),
        None => e.help(format!("available: {}", KNOWN.join(", "))),
    }
}

fn comparison(op: BinOp, lhs: Typed, rhs: Typed, span: Span) -> Result<Typed, ExprError> {
    let node = match op {
        BinOp::Eq | BinOp::Ne => {
            if !lhs.ty.has_equality() {
                return Err(ExprError::new(
                    format!("{} values can't be compared", lhs.ty),
                    span,
                ));
            }
            Node::Eq(op == BinOp::Ne, Box::new(lhs.node), Box::new(rhs.node))
        }
        _ => {
            int_operand(&lhs.ty, op.symbol(), span)?;
            Node::Ord(op, Box::new(lhs.node), Box::new(rhs.node))
        }
    };
    Ok(Typed {
        node,
        ty: Type::Bool,
    })
}

fn constant(value: Value, ty: Type) -> Typed {
    Typed {
        node: Node::Const(value),
        ty,
    }
}

/// The integer type an unsuffixed literal takes from its expected type.
fn literal_int_hint(expected: &Type) -> Option<IntType> {
    match expected {
        Type::Int(t) => Some(*t),
        Type::Option(inner) => literal_int_hint(inner),
        _ => None,
    }
}

/// Whether `e`'s type comes from context: an unsuffixed number, or arithmetic,
/// negation or `if` over only such numbers.
fn is_polymorphic(e: &Expr) -> bool {
    match &e.kind {
        ExprKind::Int { suffix, .. } => suffix.is_none(),
        ExprKind::Null => true,
        ExprKind::Unary(UnOp::Neg, inner) => is_polymorphic(inner),
        ExprKind::Binary(op, a, b)
            if !op.is_comparison() && !matches!(op, BinOp::And | BinOp::Or) =>
        {
            is_polymorphic(a) && is_polymorphic(b)
        }
        ExprKind::If(_, a, b) => is_polymorphic(a) && is_polymorphic(b),
        _ => false,
    }
}

fn int_operand(ty: &Type, op: &str, span: Span) -> Result<IntType, ExprError> {
    ty.as_int()
        .ok_or_else(|| ExprError::new(format!("`{op}` needs integers, found {ty}"), span))
}

fn readable_type(tag: &TypeTag, name: &str, span: Span) -> Result<Type, ExprError> {
    Type::from_move(tag).ok_or_else(|| {
        ExprError::new(
            format!("`{name}` has type `{tag}`, which expressions can't read"),
            span,
        )
    })
}

fn mismatch(expected: &Type, found: &Type, span: Span) -> ExprError {
    let e = ExprError::new(format!("expected {expected}, found {found}"), span);
    let target = match expected {
        Type::Int(t) => Some(*t),
        Type::Option(inner) => inner.as_int(),
        _ => None,
    };
    match (target, found) {
        (Some(to), Type::Int(_)) => e.help(format!(
            "integers never convert implicitly; write `{to}(...)` to convert"
        )),
        _ => e,
    }
}

/// The candidate within a small edit distance of `name`, if any.
fn closest<'a>(name: &str, candidates: impl IntoIterator<Item = &'a str>) -> Option<&'a str> {
    let limit = (name.len() / 3).max(1);
    candidates
        .into_iter()
        .map(|c| (edit_distance(name, c), c))
        .filter(|&(d, _)| d <= limit)
        .min_by_key(|&(d, _)| d)
        .map(|(_, c)| c)
}

/// Edit distance counting insertions, deletions, substitutions and swaps of adjacent
/// characters (optimal string alignment), so `mni` is one edit from `min`.
fn edit_distance(a: &str, b: &str) -> usize {
    let a: Vec<char> = a.chars().collect();
    let b: Vec<char> = b.chars().collect();
    let width = b.len() + 1;
    let mut d = vec![0; (a.len() + 1) * width];
    for i in 0..=a.len() {
        d[i * width] = i;
    }
    for (j, cell) in d.iter_mut().take(width).enumerate() {
        *cell = j;
    }
    for i in 1..=a.len() {
        for j in 1..=b.len() {
            let cost = usize::from(a[i - 1] != b[j - 1]);
            let mut best = (d[(i - 1) * width + j] + 1)
                .min(d[i * width + j - 1] + 1)
                .min(d[(i - 1) * width + j - 1] + cost);
            if i > 1 && j > 1 && a[i - 1] == b[j - 2] && a[i - 2] == b[j - 1] {
                best = best.min(d[(i - 2) * width + j - 2] + 1);
            }
            d[i * width + j] = best;
        }
    }
    d[a.len() * width + b.len()]
}
