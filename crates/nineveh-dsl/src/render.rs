//! DSL expressions to `nineveh-expr` source text.
//!
//! The two grammars agree on almost everything — the operators, the literals, field
//! access — so rendering is mostly a matter of resolving names and respelling three
//! forms: `c ? a : b`, `x?.c ?? d` and `address("0x1")`.
//!
//! The output is text rather than a tree because that is what [`nineveh_config::Expr`]
//! holds: a rule keeps its expression as source until `nineveh-expr` checks it against
//! the resolved types. Text also means a rule reads back the way the developer wrote
//! it, so a type error quotes something recognisable. Parentheses are added only where
//! precedence needs them.

use std::collections::HashMap;

use nineveh_config::{Diagnostic, Span};

use crate::ast::{BinOp, Expr, ExprKind, Name, UnOp};

/// What a name means inside one handler.
pub(crate) enum Binding {
    /// `const x = <expr>`: substituted wherever it's used.
    Value(Expr),
    /// `const b = <table>.row(…)`: only `b.column` is a value, and only for the row
    /// the rule being built writes. The key expressions travel with the binding, so a
    /// write through it knows the row it means.
    Row { table: String, keys: Vec<Expr> },
}

pub(crate) struct Ctx<'a> {
    /// The handler's parameter, a local alias for the record.
    pub(crate) param: &'a str,
    /// The source the record came from: what `<param>.field` is called downstream.
    pub(crate) source: &'a str,
    /// `const` bindings in scope, innermost last.
    pub(crate) scope: &'a [(String, Binding)],
    /// Tables a rule may read by key, with the number of key columns where it's known
    /// before the config is resolved.
    pub(crate) readable: &'a HashMap<String, Option<usize>>,
    /// Tables that exist but can't be read: a rule can't read a `log` (ADR 0019).
    pub(crate) logs: &'a [String],
    /// The row binding the rule being built writes, if it was named by a `const`.
    pub(crate) target: Option<&'a str>,
}

impl Ctx<'_> {
    fn lookup(&self, name: &str) -> Option<&Binding> {
        self.scope
            .iter()
            .rev()
            .find(|(n, _)| n == name)
            .map(|(_, b)| b)
    }
}

/// Atoms never need wrapping; `if … then … else` and `unwrap_or` are rendered as a
/// conditional and a call, so they don't either once written.
const ATOM: u8 = 100;
const UNARY: u8 = 7;

pub(crate) fn render(e: &Expr, ctx: &Ctx<'_>) -> Result<String, Diagnostic> {
    expr(e, ctx, 0)
}

/// Render `e` as an operand that binds at least as tightly as `min`, so the caller can
/// splice it into a larger expression without changing what it means.
pub(crate) fn render_at(e: &Expr, ctx: &Ctx<'_>, min: u8) -> Result<String, Diagnostic> {
    expr(e, ctx, min)
}

/// Render `e` as a `when` condition, negated if `negate`. Negating a comparison flips
/// the operator rather than wrapping it, so a generated `when` still reads like
/// something a person would write.
pub(crate) fn render_cond(e: &Expr, ctx: &Ctx<'_>, negate: bool) -> Result<String, Diagnostic> {
    if !negate {
        return expr(e, ctx, BinOp::And.power());
    }
    match &e.kind {
        ExprKind::Unary(UnOp::Not, inner) => expr(inner, ctx, BinOp::And.power()),
        ExprKind::Binary(op, l, r) => match op.negated() {
            Some(flipped) => {
                let e = Expr {
                    kind: ExprKind::Binary(flipped, l.clone(), r.clone()),
                    span: e.span,
                };
                expr(&e, ctx, BinOp::And.power())
            }
            None => Ok(format!("!({})", expr(e, ctx, 0)?)),
        },
        _ => {
            let inner = expr(e, ctx, UNARY)?;
            Ok(format!("!{inner}"))
        }
    }
}

fn expr(e: &Expr, ctx: &Ctx<'_>, min: u8) -> Result<String, Diagnostic> {
    let (text, power) = match &e.kind {
        ExprKind::Int(digits) => (int(digits, e.span)?, ATOM),
        ExprKind::Str(s) => (format!("'{s}'"), ATOM),
        ExprKind::Bool(b) => (b.to_string(), ATOM),
        ExprKind::Null => ("null".to_owned(), ATOM),
        ExprKind::Name(n) => (name(n, e.span, ctx)?, ATOM),
        ExprKind::Field(base, field) => (self_field(base, field, ctx)?, ATOM),
        ExprKind::Get { table, .. } => {
            return Err(Diagnostic::new(
                format!("`{}.get(…)` is a row, not a value", table.text),
                Some(e.span),
            )
            .help(format!(
                "read a column of it: `{}.get(…)?.<column>`",
                table.text
            )));
        }
        ExprKind::OptField(base, field) => (lookup(base, field, ctx)?, ATOM),
        ExprKind::Coalesce(a, b) => (
            format!("unwrap_or({}, {})", expr(a, ctx, 0)?, expr(b, ctx, 0)?),
            ATOM,
        ),
        ExprKind::Ternary(c, a, b) => (
            format!(
                "if {} then {} else {}",
                expr(c, ctx, 1)?,
                expr(a, ctx, 1)?,
                expr(b, ctx, 1)?
            ),
            0,
        ),
        ExprKind::Unary(op, inner) => {
            let sym = match op {
                UnOp::Not => "!",
                UnOp::Neg => "-",
            };
            (format!("{sym}{}", expr(inner, ctx, UNARY)?), UNARY)
        }
        ExprKind::Binary(op, l, r) => (
            format!(
                "{} {} {}",
                expr(l, ctx, op.power())?,
                op.text(),
                // The right side binds one level tighter, so `a - (b - c)` keeps its
                // parentheses and `a - b - c` doesn't grow any.
                expr(r, ctx, op.power() + 1)?
            ),
            op.power(),
        ),
        ExprKind::Call(callee, args) => (call(callee, args, ctx)?, ATOM),
    };
    Ok(if power < min {
        format!("({text})")
    } else {
        text
    })
}

/// A bare name. Only a `const` value resolves; everything a record or row holds is
/// reached through its owner, so there's never a question which one a name meant.
fn name(n: &str, span: Span, ctx: &Ctx<'_>) -> Result<String, Diagnostic> {
    match ctx.lookup(n) {
        Some(Binding::Value(value)) => {
            // Substituted at its use; the caller's `min` decides the parentheses.
            expr(value, ctx, ATOM)
        }
        Some(Binding::Row { table, .. }) => Err(Diagnostic::new(
            format!("`{n}` is a row, not a value"),
            Some(span),
        )
        .help(format!("read a column of it: `{n}.<column of {table}>`"))),
        None if n == ctx.param => Err(Diagnostic::new(
            format!("`{n}` is the whole record"),
            Some(span),
        )
        .help(format!("read a field of it: `{n}.<field>`"))),
        None if ctx.readable.contains_key(n) => Err(Diagnostic::new(
            format!("`{n}` is a table, not a value"),
            Some(span),
        )
        .help(format!("read a row of it: `{n}.get(<key>)?.<column>`"))),
        None => Err(unknown(n, span, ctx)),
    }
}

/// `<param>.field`, `<row>.column`, `tx.version`, or a field of a struct value.
fn self_field(base: &Expr, field: &Name, ctx: &Ctx<'_>) -> Result<String, Diagnostic> {
    if let ExprKind::Name(n) = &base.kind {
        if n == "tx" && ctx.lookup(n).is_none() {
            return Ok(format!("tx.{}", field.text));
        }
        if n == ctx.param && ctx.lookup(n).is_none() {
            return Ok(format!("{}.{}", ctx.source, field.text));
        }
        if let Some(Binding::Row { table, .. }) = ctx.lookup(n) {
            return if ctx.target == Some(n.as_str()) {
                Ok(format!("row.{}", field.text))
            } else {
                Err(Diagnostic::new(
                    format!("`{n}` isn't the row this rule writes"),
                    Some(base.span),
                )
                .help(format!(
                    "a rule reads its own row; for another, use `{table}.get(<key>)?.{}`",
                    field.text
                )))
            };
        }
    }
    Ok(format!("{}.{}", expr(base, ctx, ATOM)?, field.text))
}

/// `markets.get(m)?.fee_bps` → `markets[m].fee_bps`.
fn lookup(base: &Expr, field: &Name, ctx: &Ctx<'_>) -> Result<String, Diagnostic> {
    let ExprKind::Get { table, keys } = &base.kind else {
        return Err(Diagnostic::new(
            "`?.` reads a column of a row that may not be there".to_owned(),
            Some(base.span),
        )
        .help("it follows a table lookup, like `markets.get(id)?.fee_bps`"));
    };
    let Some(&arity) = ctx.readable.get(&table.text) else {
        if ctx.logs.contains(&table.text) {
            return Err(Diagnostic::new(
                format!("`{}` is a log, so a rule can't read it", table.text),
                Some(table.span),
            )
            .help("logs are append-only history, not state (ADR 0019)"));
        }
        return Err(unknown(&table.text, table.span, ctx));
    };
    if let Some(arity) = arity
        && keys.len() != arity
    {
        return Err(Diagnostic::new(
            format!(
                "`{}` is keyed by {arity} column{}, but {} {} given",
                table.text,
                if arity == 1 { "" } else { "s" },
                keys.len(),
                if keys.len() == 1 { "was" } else { "were" }
            ),
            Some(table.span),
        ));
    }
    let keys = keys
        .iter()
        .map(|k| expr(k, ctx, 0))
        .collect::<Result<Vec<_>, _>>()?
        .join(", ");
    Ok(format!("{}[{keys}].{}", table.text, field.text))
}

fn call(callee: &Name, args: &[Expr], ctx: &Ctx<'_>) -> Result<String, Diagnostic> {
    // `address("0x1")` is how a `.ts` file writes what `nineveh-expr` calls `@0x1`.
    if callee.text == "address" {
        let [arg] = args else {
            return Err(Diagnostic::new(
                "`address(…)` takes one string".to_owned(),
                Some(callee.span),
            ));
        };
        let ExprKind::Str(text) = &arg.kind else {
            return Err(Diagnostic::new(
                "an address is written as a string".to_owned(),
                Some(arg.span),
            )
            .help("like `address(\"0x1\")`"));
        };
        return match text.parse::<nineveh_core::Address>() {
            Ok(a) => Ok(format!("@{a}")),
            Err(e) => Err(Diagnostic::new(e.to_string(), Some(arg.span))),
        };
    }
    let args = args
        .iter()
        .map(|a| expr(a, ctx, 0))
        .collect::<Result<Vec<_>, _>>()?
        .join(", ");
    Ok(format!("{}({args})", callee.text))
}

/// Integers pass through as written, except that hex becomes decimal: `nineveh-expr`
/// takes digits, and `0x…` in a `.ts` file is a number, not an address.
fn int(digits: &str, span: Span) -> Result<String, Diagnostic> {
    let plain = digits.replace('_', "");
    let Some(hex) = plain
        .strip_prefix("0x")
        .or_else(|| plain.strip_prefix("0X"))
    else {
        return Ok(plain);
    };
    u128::from_str_radix(hex, 16)
        .map(|v| v.to_string())
        .map_err(|_| {
            Diagnostic::new("this hex number is too large".to_owned(), Some(span))
                .help("write an address as `address(\"0x…\")`")
        })
}

fn unknown(n: &str, span: Span, ctx: &Ctx<'_>) -> Diagnostic {
    let mut candidates: Vec<&str> = vec![ctx.param, "tx"];
    candidates.extend(ctx.scope.iter().map(|(n, _)| n.as_str()));
    candidates.extend(ctx.readable.keys().map(String::as_str));
    Diagnostic::new(format!("nothing here is called `{n}`"), Some(span)).did_you_mean(n, candidates)
}
