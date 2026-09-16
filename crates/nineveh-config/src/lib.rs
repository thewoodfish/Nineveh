//! Nineveh project configs: `nineveh.yaml`.
//!
//! A config names the chain data a project follows (sources), the state tables built
//! from it, and how they're served. It's the product surface for teams that don't
//! write Rust, so every problem is reported at the YAML text it's about, with a
//! suggested fix where there is one.
//!
//! Loading is two steps:
//!
//! 1. [`parse`] checks everything that doesn't need `nineveh.lock` and returns a
//!    [`Config`]. `nineveh init` uses [`Config::roots`] from here to know which
//!    structs to pin.
//! 2. [`Config::resolve`] checks the config against the lock's layouts and returns a
//!    [`Project`], carrying the decode [`Selection`](nineveh_decode::Selection) and
//!    each rule's record [`Scope`].
//!
//! `parse` keeps reducer expressions as source text ([`Expr`]); `resolve` typechecks
//! and compiles each one with `nineveh-expr` against its rule's record scope and the
//! table's columns, so expression errors are located in the YAML too. The format is
//! documented for users in `docs/config.md`.

mod diagnostic;
mod model;
mod raw;
mod resolve;
mod schema;
mod validate;

pub use diagnostic::{Diagnostic, Diagnostics, Span};
pub use model::{
    Action, Api, Change, Column, ColumnType, Config, Expr, Named, Rule, Source, SourceKind,
    StartVersion, StateTable, Subscription, TableKind, Trigger,
};
pub use resolve::{
    CompiledExpr, Input, Project, ResolvedAction, ResolvedRule, ResolvedTable, Scope, Watcher,
    record_scope,
};
pub use schema::{Projection, SchemaColumn, TableSchema, column_for};
pub use validate::parse;
