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

pub use check::{Cell, ColumnVar, Env, Structs, TableColumn, TableVar, compile};
pub use error::{EvalError, EvalErrorKind, ExprError, Span};
pub use eval::{Compiled, Inputs, NoTables, Tables, Tx};
pub use types::{IntType, Type};

/// Where a bare name stands in an expression.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum NamePosition {
    /// On its own, as a value: the `balance` in `balance + 1`.
    Value,
    /// Owning the `.field` that follows it: the `row` in `row.balance`, or a column
    /// holding a struct in `position.size`.
    Owner,
}

/// Rewrite the bare names in `text`, leaving every other byte exactly as it was.
///
/// `rename` is asked about each bare name, with where it stands, and answers with what to
/// put in its place or `None` to leave it alone. Only the names are touched — the text is
/// spliced, never re-printed — so spacing, parentheses and literals come back unchanged
/// and an expression cannot change meaning on the way through.
///
/// This exists because the two frontends spell the same expression differently: a config
/// rule says `balance + amount`, where a name is a column or a record field depending on
/// which one has it, and the DSL says `b.balance + r.amount`, where the owner is always
/// written. Going from the first to the second is renaming bare names and nothing else.
///
/// # Errors
///
/// [`ExprError`] if `text` doesn't parse, located in `text`.
pub fn rename_names(
    text: &str,
    rename: &dyn Fn(&str, NamePosition) -> Option<String>,
) -> Result<String, ExprError> {
    let parsed = syntax::parse(text)?;
    let mut names = Vec::new();
    syntax::bare_names(&parsed, &mut names);
    // Back to front, so each splice leaves the earlier offsets still true.
    names.sort_by_key(|(span, _, _)| std::cmp::Reverse(span.start));
    let mut out = text.to_owned();
    for (span, name, owner) in names {
        let position = if owner {
            NamePosition::Owner
        } else {
            NamePosition::Value
        };
        if let Some(replacement) = rename(&name, position) {
            out.replace_range(span.start..span.end, &replacement);
        }
    }
    Ok(out)
}

#[cfg(test)]
mod rename_tests {
    use super::{NamePosition, rename_names};

    /// The config's two spellings of "which one did you mean", and the DSL's. `deposits`
    /// is both the source and a column, which is the case that caught this out.
    fn dsl(name: &str, position: NamePosition) -> Option<String> {
        if position == NamePosition::Owner {
            match name {
                "row" => return Some("b".to_owned()),
                "deposits" => return Some("r".to_owned()),
                _ => {}
            }
        }
        match name {
            "balance" | "count" | "deposits" => Some(format!("b.{name}")),
            "amount" | "sender" => Some(format!("r.{name}")),
            _ => None,
        }
    }

    #[test]
    fn a_column_named_after_its_source_is_still_the_column() {
        assert_eq!(
            rename_names("deposits + 1", &dsl).expect("parses"),
            "b.deposits + 1"
        );
        assert_eq!(
            rename_names("deposits.amount", &dsl).expect("parses"),
            "r.amount"
        );
    }

    #[test]
    fn a_bare_name_takes_the_owner_its_binding_has() {
        assert_eq!(
            rename_names("balance + amount", &dsl).expect("parses"),
            "b.balance + r.amount"
        );
    }

    #[test]
    fn everything_that_is_not_a_name_survives_byte_for_byte() {
        let text = "if count == 0 then 1u64 else (balance * 2) / amount";
        assert_eq!(
            rename_names(text, &dsl).expect("parses"),
            "if b.count == 0 then 1u64 else (b.balance * 2) / r.amount"
        );
    }

    #[test]
    fn an_already_qualified_name_keeps_its_meaning() {
        assert_eq!(
            rename_names("row.balance + deposits.amount", &dsl).expect("parses"),
            "b.balance + r.amount"
        );
    }

    #[test]
    fn a_table_read_is_left_alone_and_its_keys_are_not() {
        // `holders` names a table, not a value, so it keeps its name even though the
        // key inside it is a record field that needs one.
        assert_eq!(
            rename_names("holders[sender].balance", &dsl).expect("parses"),
            "holders[r.sender].balance"
        );
    }

    #[test]
    fn built_ins_and_literals_are_nobody_s_to_rename() {
        assert_eq!(
            rename_names("tx.timestamp + 1", &dsl).expect("parses"),
            "tx.timestamp + 1"
        );
    }

    #[test]
    fn text_that_does_not_parse_is_an_error_rather_than_a_guess() {
        assert!(rename_names("balance +", &dsl).is_err());
    }
}
