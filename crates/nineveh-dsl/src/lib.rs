//! Nineveh's reducer DSL: reduce tables and handlers written in a `.nineveh.ts` file.
//!
//! A reducer is behaviour, and behaviour is easier to read event-first — "when a
//! deposit arrives, here is everything that changes" — than table-first, which is how
//! `nineveh.yaml` has to spell it. This crate is the frontend that lets a project be
//! written the first way and run as the second (ADR 0025):
//!
//! ```text
//! .nineveh.ts  →  lex  →  parse  →  scatter  →  StateTable { Reduce { rules } }
//! ```
//!
//! Nothing downstream changes. The output is the same [`StateTable`] a `reduce:` block
//! parses to, so `nineveh-config` resolves it, `nineveh-expr` typechecks its
//! expressions and `nineveh-engine` folds it exactly as before. No JavaScript is
//! executed, here or at runtime: the file is parsed, not evaluated.
//!
//! ```
//! use nineveh_dsl::{Context, SourceInfo, compile};
//!
//! let source = r#"
//!     export const balances = table({
//!       key:     { user: address },
//!       columns: { balance: u128.default(0) },
//!     })
//!
//!     on(deposits, (d) => {
//!       balances.row(d.user).balance += u128(d.amount)
//!     })
//! "#;
//! let ctx = Context {
//!     sources: vec![SourceInfo { name: "deposits".into(), has_deletes: false }],
//!     tables: Vec::new(),
//! };
//! let tables = compile(source, &ctx).expect("compiles");
//! assert_eq!(tables.len(), 1);
//! ```

mod ast;
mod dts;
mod lex;
mod parse;
mod render;
mod scatter;

use nineveh_config::{Diagnostics, StateTable};

pub use dts::{SourceDecl, TableDecl, declarations};
pub use scatter::{Context, SourceInfo, TableInfo};

/// Compile a `.nineveh.ts` file into the state tables it declares, each carrying the
/// rules scattered out of the handlers that write it.
///
/// # Errors
///
/// [`Diagnostics`] locating every problem in the source, rendered against the file
/// with [`Diagnostics::render`].
pub fn compile(source: &str, ctx: &Context) -> Result<Vec<StateTable>, Diagnostics> {
    let program = parse::parse(source).map_err(Diagnostics::single)?;
    scatter::scatter(&program, ctx)
}
