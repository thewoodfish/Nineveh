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
//! Reducer expressions stay source text ([`Expr`]) here; `nineveh-expr` parses and
//! typechecks them against the scopes this crate resolves. The format is documented
//! for users in `docs/config.md`.

mod diagnostic;
mod model;
mod raw;
mod resolve;
mod validate;

pub use diagnostic::{Diagnostic, Diagnostics, Span};
pub use model::{
    Action, Api, Change, Column, ColumnType, Config, Expr, Named, Rule, Source, SourceKind,
    StartVersion, StateTable, Subscription, TableKind, Trigger,
};
pub use resolve::{Input, KeySource, Project, ResolvedRule, ResolvedTable, Scope};
pub use validate::parse;
