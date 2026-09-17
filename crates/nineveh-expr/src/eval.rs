//! Evaluation: a compiled expression plus inputs gives a value, or a located error.
//!
//! The interpreter is a plain tree walk. It's total by construction: there are no
//! loops, no recursion in the language, and no I/O, clock or randomness, so the same
//! inputs always give the same result (ADR 0007).

use std::cmp::Ordering;

use nineveh_core::{Address, Identifier, TypeTag, Value};

use crate::check::Cell;
use crate::error::{EvalError, EvalErrorKind, Span};
use crate::num::{Int, IntError};
use crate::syntax::BinOp;
use crate::types::{IntType, Type};

/// A typechecked expression, ready to evaluate.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Compiled {
    node: Node,
    ty: Type,
}

/// Transaction facts an expression can read. Both are on-chain, so reading them is
/// deterministic.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub struct Tx {
    pub version: u64,
    /// Block timestamp in microseconds since the Unix epoch.
    pub timestamp_micros: u64,
}

/// The project's other state tables, read a row at a time by `table[key].column`.
///
/// Reads are of committed state plus what the fold has already changed, so they're
/// deterministic and replay the same way (ADR 0019).
pub trait Tables {
    /// The stored row of state table `table` at `key`, or `None` if there is none.
    fn row(&self, table: u32, key: &[Value]) -> Option<Vec<Value>>;
}

/// No tables to read: every lookup finds nothing.
#[derive(Debug, Clone, Copy, Default)]
pub struct NoTables;

impl Tables for NoTables {
    fn row(&self, _table: u32, _key: &[Value]) -> Option<Vec<Value>> {
        None
    }
}

/// The values an expression reads, in the order of its [`Env`](crate::Env).
#[derive(Debug, Clone, Copy)]
pub struct Inputs<'a> {
    /// The row's current values, one per column. For a new row, the engine supplies
    /// each column's default, or `Option(None)` for a nullable column.
    pub row: &'a [Value],
    /// The record's field values, one per record field.
    pub record: &'a [Value],
    pub tx: Tx,
    /// The other state tables, for `table[key].column`.
    pub tables: &'a dyn Tables,
}

impl std::fmt::Debug for &dyn Tables {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.write_str("Tables")
    }
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) enum Node {
    Const(Value),
    Column(usize),
    /// A record field; `flatten` is set when its type holds `Object<T>` values, which
    /// expressions see as addresses.
    Record {
        index: usize,
        flatten: Option<TypeTag>,
    },
    Field {
        base: Box<Node>,
        name: Identifier,
        flatten: Option<TypeTag>,
    },
    /// `table[key].column`: the column's value in another table's row, `None` when
    /// that row isn't there or the column has no value in it.
    Lookup {
        table: u32,
        key: Vec<Node>,
        cell: Cell,
        /// Read an `Object<T>` stored in the column as its address, as the API does.
        address: bool,
    },
    TxVersion,
    TxTimestamp,
    Not(Box<Node>),
    Neg(Box<Node>, IntType, Span),
    Arith(BinOp, IntType, Box<Node>, Box<Node>, Span),
    /// `==`, or `!=` when the flag is set.
    Eq(bool, Box<Node>, Box<Node>),
    Ord(BinOp, Box<Node>, Box<Node>),
    And(Box<Node>, Box<Node>),
    Or(Box<Node>, Box<Node>),
    If(Box<Node>, Box<Node>, Box<Node>),
    Cast(IntType, Box<Node>, Span),
    /// `min`, or `max` when the flag is set.
    MinMax(bool, Box<Node>, Box<Node>),
    Abs(Box<Node>, IntType, Span),
    /// `is_some`, or `is_none` when the flag is set.
    IsSome(bool, Box<Node>),
    UnwrapOr(Box<Node>, Box<Node>),
    /// Wraps a `T` where an `Option<T>` is expected.
    Some(Box<Node>),
}

impl Compiled {
    pub(crate) fn new(node: Node, ty: Type) -> Self {
        Self { node, ty }
    }

    /// The expression's type.
    #[must_use]
    pub fn ty(&self) -> &Type {
        &self.ty
    }

    /// Evaluate against `inputs`.
    ///
    /// # Errors
    ///
    /// On overflow, division by zero, or an out-of-range cast, located within the
    /// expression. Never retryable.
    pub fn eval(&self, inputs: &Inputs<'_>) -> Result<Value, EvalError> {
        eval(&self.node, inputs)
    }
}

fn eval(node: &Node, inputs: &Inputs<'_>) -> Result<Value, EvalError> {
    let missing = |what| EvalError {
        kind: EvalErrorKind::MissingInput(what),
        span: Span::new(0, 0),
    };
    Ok(match node {
        Node::Const(v) => v.clone(),
        Node::Column(i) => inputs.row.get(*i).cloned().ok_or_else(|| missing("row"))?,
        Node::Record { index, flatten } => {
            let value = inputs.record.get(*index).ok_or_else(|| missing("record"))?;
            match flatten {
                Some(tag) => flatten_objects(value, tag),
                None => value.clone(),
            }
        }
        Node::Field {
            base,
            name,
            flatten,
        } => {
            let base = eval(base, inputs)?;
            let value = base.field(name.as_str()).ok_or_else(|| missing("field"))?;
            match flatten {
                Some(tag) => flatten_objects(value, tag),
                None => value.clone(),
            }
        }
        Node::Lookup {
            table,
            key,
            cell,
            address,
        } => lookup(*table, key, cell, *address, inputs)?,
        Node::TxVersion => Value::U64(inputs.tx.version),
        Node::TxTimestamp => Value::U64(inputs.tx.timestamp_micros),
        Node::Not(operand) => Value::Bool(!bool_of(&eval(operand, inputs)?)),
        Node::Neg(operand, ty, span) => {
            let int = int_of(&eval(operand, inputs)?);
            fit(int.neg(), *ty, "-", *span)?
        }
        Node::Arith(op, ty, lhs, rhs, span) => arith(*op, *ty, lhs, rhs, *span, inputs)?,
        Node::Eq(negate, lhs, rhs) => {
            let equal = eval(lhs, inputs)? == eval(rhs, inputs)?;
            Value::Bool(equal != *negate)
        }
        Node::Ord(op, lhs, rhs) => {
            let ordering = int_of(&eval(lhs, inputs)?).cmp(&int_of(&eval(rhs, inputs)?));
            Value::Bool(match op {
                BinOp::Lt => ordering == Ordering::Less,
                BinOp::Le => ordering != Ordering::Greater,
                BinOp::Gt => ordering == Ordering::Greater,
                _ => ordering != Ordering::Less,
            })
        }
        Node::And(lhs, rhs) => {
            Value::Bool(bool_of(&eval(lhs, inputs)?) && bool_of(&eval(rhs, inputs)?))
        }
        Node::Or(lhs, rhs) => {
            Value::Bool(bool_of(&eval(lhs, inputs)?) || bool_of(&eval(rhs, inputs)?))
        }
        Node::If(cond, then, otherwise) => {
            if bool_of(&eval(cond, inputs)?) {
                eval(then, inputs)?
            } else {
                eval(otherwise, inputs)?
            }
        }
        Node::Cast(to, operand, span) => {
            let int = int_of(&eval(operand, inputs)?);
            int.to_value(*to).map_err(|_| EvalError {
                kind: EvalErrorKind::CastOutOfRange { to: *to },
                span: *span,
            })?
        }
        Node::MinMax(max, lhs, rhs) => {
            let a = eval(lhs, inputs)?;
            let b = eval(rhs, inputs)?;
            let a_first = int_of(&a) <= int_of(&b);
            if a_first == *max { b } else { a }
        }
        Node::Abs(operand, ty, span) => {
            let int = int_of(&eval(operand, inputs)?);
            fit(int.abs(), *ty, "abs", *span)?
        }
        Node::IsSome(negate, operand) => {
            let is_some = matches!(eval(operand, inputs)?, Value::Option(Some(_)));
            Value::Bool(is_some != *negate)
        }
        Node::UnwrapOr(option, default) => match eval(option, inputs)? {
            Value::Option(Some(v)) => *v,
            _ => eval(default, inputs)?,
        },
        Node::Some(inner) => Value::Option(Some(Box::new(eval(inner, inputs)?))),
    })
}

/// `table[key].column`: the column's value in another table's row, as an option,
/// since neither the row nor the value in it need be there.
fn lookup(
    table: u32,
    key: &[Node],
    cell: &Cell,
    address: bool,
    inputs: &Inputs<'_>,
) -> Result<Value, EvalError> {
    let mut values = Vec::with_capacity(key.len());
    for part in key {
        values.push(eval(part, inputs)?);
    }
    let found = inputs
        .tables
        .row(table, &values)
        .and_then(|row| read_cell(&row, cell));
    let found = match found {
        Some(value) if address => Some(object_address(value)),
        found => found,
    };
    Ok(match found {
        // A column that's an option already carries its own absence.
        Some(option @ Value::Option(_)) => option,
        Some(value) => Value::Option(Some(Box::new(value))),
        None => Value::Option(None),
    })
}

/// A column's value in a stored row, or `None` when the row has none there: an enum
/// field a variant doesn't declare, or a row shorter than the table's layout.
fn read_cell(row: &[Value], cell: &Cell) -> Option<Value> {
    match cell {
        Cell::At(i) => row.get(*i).cloned(),
        Cell::Field(i, name) => row.get(*i)?.field(name.as_str()).cloned(),
        Cell::Variant(i) => match row.get(*i)? {
            Value::Variant { name, .. } => Some(Value::String(name.to_string())),
            _ => None,
        },
    }
}

fn arith(
    op: BinOp,
    ty: IntType,
    lhs: &Node,
    rhs: &Node,
    span: Span,
    inputs: &Inputs<'_>,
) -> Result<Value, EvalError> {
    let a = int_of(&eval(lhs, inputs)?);
    let b = int_of(&eval(rhs, inputs)?);
    let result = match op {
        BinOp::Add => a.add(b),
        BinOp::Sub => a.sub(b),
        BinOp::Mul => a.mul(b),
        BinOp::Div => a.div(b),
        _ => a.rem(b),
    };
    result
        .and_then(|int| int.to_value(ty))
        .map_err(|e| int_error(e, ty, op.symbol(), span))
}

fn fit(int: Int, ty: IntType, op: &'static str, span: Span) -> Result<Value, EvalError> {
    int.to_value(ty).map_err(|e| int_error(e, ty, op, span))
}

fn int_error(e: IntError, ty: IntType, op: &'static str, span: Span) -> EvalError {
    EvalError {
        kind: match e {
            IntError::OutOfRange => EvalErrorKind::Overflow { op, ty },
            IntError::DivideByZero => EvalErrorKind::DivideByZero,
        },
        span,
    }
}

// The typechecker guarantees operand types; these fallbacks are unreachable for
// well-typed inputs and keep evaluation panic-free if a caller supplies wrong ones.

fn int_of(value: &Value) -> Int {
    Int::from_value(value).map_or(Int::ZERO, |(_, i)| i)
}

fn bool_of(value: &Value) -> bool {
    matches!(value, Value::Bool(true))
}

/// An `Object<T>` as its address; anything else unchanged. Storage reads object
/// columns this way (ADR 0008), so a lookup of one matches what the API serves.
fn object_address(value: Value) -> Value {
    match &value {
        Value::Struct(fields) => match fields.as_slice() {
            [(name, Value::Address(address))] if name.as_str() == "inner" => {
                Value::Address(*address)
            }
            _ => value,
        },
        _ => value,
    }
}

/// Convert the `Object<T>` values inside `value` (of Move type `tag`) to addresses.
fn flatten_objects(value: &Value, tag: &TypeTag) -> Value {
    match (tag, value) {
        (TypeTag::Struct(s), Value::Struct(fields))
            if s.name.is(Address::ONE, "object", "Object") =>
        {
            match fields.first() {
                Some((_, Value::Address(a))) => Value::Address(*a),
                _ => value.clone(),
            }
        }
        (TypeTag::Vector(inner), Value::Vector(items)) => {
            Value::Vector(items.iter().map(|v| flatten_objects(v, inner)).collect())
        }
        (TypeTag::Struct(s), Value::Option(inner))
            if s.name.is(Address::ONE, "option", "Option") =>
        {
            let arg = s.type_args.first();
            Value::Option(
                inner.as_ref().map(|v| {
                    Box::new(arg.map_or_else(|| (**v).clone(), |a| flatten_objects(v, a)))
                }),
            )
        }
        _ => value.clone(),
    }
}
