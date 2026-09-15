//! Nineveh's reducer expression language (ADR 0007).
//!
//! Rules in `nineveh.yaml` compute column values with expressions like
//! `balance + amount` or `if is_long then size else 0`. The language is:
//!
//! - **typed**: checked when the config is validated, against the record's fields
//!   and the row's columns, with errors located in the expression text;
//! - **exact**: every Move integer type, `u8` through `u256` and `i8` through `i256`,
//!   with checked arithmetic and no implicit conversions or floating point;
//! - **total**: no loops, recursion, I/O, clock or randomness, so evaluation always
//!   terminates and the same inputs always give the same result.
//!
//! Overflow, division by zero and out-of-range casts are [`EvalError`]s. They're
//! deterministic, so the engine halts the project at that record rather than retrying.
//!
//! [`compile`] parses and typechecks against an [`Env`]; [`Compiled::eval`] runs the
//! result on [`Inputs`]. The user-facing reference is `docs/expressions.md`.

mod check;
mod error;
mod eval;
mod num;
mod syntax;
mod types;

pub use check::{ColumnVar, Env, Structs, compile};
pub use error::{EvalError, EvalErrorKind, ExprError, Span};
pub use eval::{Compiled, Inputs, Tx};
pub use types::{IntType, Type};
