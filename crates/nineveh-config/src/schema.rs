//! The columns each state table is stored and served with.
//!
//! A `reduce` table's columns are the ones the config declares. A `mirror` or `log`
//! table's come from its source's layout: its key, then one column per field of the
//! value's struct, typed by ADR 0008. The engine's rows for those tables hold the whole
//! value (ADR 0013), so each column also says where in the engine's row its value is.

use nineveh_core::{Address, Identifier, StructName, TypeTag};
use nineveh_decode::{Body, Lockfile, TypeMatcher};

use crate::diagnostic::{Diagnostic, Span};
use crate::model::{ColumnType, Named, StateTable, TableKind};
use crate::resolve::Input;

/// A state table's columns, in order, and which of them form its key.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct TableSchema {
    pub columns: Vec<SchemaColumn>,
    /// The key columns, as indices into `columns`, in key order. The engine's key for a
    /// row holds their values in this order.
    pub key: Vec<usize>,
}

impl TableSchema {
    /// The column named `name`.
    #[must_use]
    pub fn column(&self, name: &str) -> Option<&SchemaColumn> {
        self.columns.iter().find(|c| c.name == name)
    }
}

/// One stored column.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct SchemaColumn {
    pub name: String,
    pub ty: ColumnType,
    /// Holds `Option` values: `None` is stored as null.
    pub nullable: bool,
    /// Where the column's value is in the engine's row.
    pub from: Projection,
}

/// Where a column's value is in the engine's row for its table.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Projection {
    /// The row's value at this index.
    Row(usize),
    /// A field of the struct at this index of the row.
    Field(usize, Identifier),
}

/// Key columns Nineveh adds to `mirror` and `log` tables.
const RESOURCE_KEY: [&str; 2] = ["address", "type"];
const TABLE_KEY: [&str; 2] = ["handle", "key"];
const LOG_KEY: [&str; 2] = ["version", "event_index"];
/// The column holding a value that isn't stored field by field.
const VALUE: &str = "value";

pub(crate) fn table_schema(
    lock: &Lockfile,
    table: &StateTable,
    input: Option<&Input>,
    source_span: Option<Span>,
    diagnostics: &mut Vec<Diagnostic>,
) -> TableSchema {
    match (&table.kind, input) {
        (
            TableKind::Reduce {
                key: key_names,
                columns,
                ..
            },
            _,
        ) => TableSchema {
            columns: columns
                .iter()
                .enumerate()
                .map(|(i, c)| SchemaColumn {
                    name: c.name.name.clone(),
                    ty: c.ty,
                    nullable: c.nullable,
                    from: Projection::Row(i),
                })
                .collect(),
            key: key_names
                .iter()
                .filter_map(|k| columns.iter().position(|c| c.name.name == k.name))
                .collect(),
        },
        (TableKind::Mirror { .. }, Some(Input::Resource(m))) => {
            let mut builder = Builder::new(&table.name, source_span);
            builder.key(RESOURCE_KEY[0], ColumnType::Address);
            let (name, args) = match m {
                TypeMatcher::Exact(tag) => (&tag.name, Some(tag.type_args.as_slice())),
                TypeMatcher::AnyInstance(name) => {
                    builder.key(RESOURCE_KEY[1], ColumnType::String);
                    (name, None)
                }
            };
            builder.value_fields(lock, name, args);
            builder.finish(diagnostics)
        }
        (TableKind::Mirror { .. }, Some(Input::Table(t))) => {
            let mut builder = Builder::new(&table.name, source_span);
            builder.key(TABLE_KEY[0], ColumnType::Address);
            let (ty, nullable) = column_for(&t.key);
            builder.push(TABLE_KEY[1], ty, nullable, Projection::Row(1), true);
            builder.value(lock, &t.value);
            builder.finish(diagnostics)
        }
        (TableKind::Log { .. }, Some(Input::Event(m))) => {
            let mut builder = Builder::new(&table.name, source_span);
            builder.key(LOG_KEY[0], ColumnType::U64);
            builder.key(LOG_KEY[1], ColumnType::U32);
            let (name, args) = match m {
                TypeMatcher::Exact(tag) => (&tag.name, Some(tag.type_args.as_slice())),
                TypeMatcher::AnyInstance(name) => (name, None),
            };
            builder.value_fields(lock, name, args);
            builder.finish(diagnostics)
        }
        // Validation guarantees mirrors follow resources or tables and logs follow
        // events, and a source that didn't resolve stops resolution before this.
        _ => TableSchema {
            columns: Vec::new(),
            key: Vec::new(),
        },
    }
}

/// Builds a `mirror` or `log` table's columns: its key, then its value.
struct Builder<'a> {
    table: &'a Named,
    span: Option<Span>,
    columns: Vec<SchemaColumn>,
    key: Vec<usize>,
    /// Fields whose names can't be column names: (field, reason).
    unusable: Vec<(String, &'static str)>,
    record: String,
}

impl<'a> Builder<'a> {
    fn new(table: &'a Named, span: Option<Span>) -> Self {
        Self {
            table,
            span,
            columns: Vec::new(),
            key: Vec::new(),
            unusable: Vec::new(),
            record: String::new(),
        }
    }

    /// A key column holding the next value of the engine's row.
    fn key(&mut self, name: &str, ty: ColumnType) {
        let index = self.columns.len();
        self.push(name, ty, false, Projection::Row(index), true);
    }

    fn push(&mut self, name: &str, ty: ColumnType, nullable: bool, from: Projection, key: bool) {
        if key {
            self.key.push(self.columns.len());
        }
        self.columns.push(SchemaColumn {
            name: name.to_owned(),
            ty,
            nullable,
            from,
        });
    }

    /// The value at the end of the engine's row: a struct stored field by field, or
    /// anything else in one `value` column.
    fn value(&mut self, lock: &Lockfile, ty: &TypeTag) {
        if let Some(tag) = ty.as_struct()
            && column_for(ty).0 == ColumnType::Json
            && lock
                .get(&tag.name)
                .is_some_and(|l| matches!(l.body, Body::Struct(_)))
        {
            self.value_fields(lock, &tag.name, Some(tag.type_args.as_slice()));
            return;
        }
        let (column, nullable) = column_for(ty);
        let index = self.columns.len();
        self.push(VALUE, column, nullable, Projection::Row(index), false);
    }

    /// One column per field of the struct `name` at the end of the engine's row. An
    /// enum is stored whole, since its fields depend on the variant. `args` are the
    /// struct's type arguments, if the source fixes them; a field whose type depends on
    /// open arguments is stored as JSON.
    fn value_fields(&mut self, lock: &Lockfile, name: &StructName, args: Option<&[TypeTag]>) {
        let row_index = self.columns.len();
        let Some(layout) = lock.get(name) else {
            return;
        };
        let Body::Struct(fields) = &layout.body else {
            self.push(
                VALUE,
                ColumnType::Json,
                false,
                Projection::Row(row_index),
                false,
            );
            return;
        };
        self.record = name.to_string();
        for field in fields {
            let ty = match args {
                Some(args) => field.ty.substitute(args).ok(),
                None => Some(field.ty.clone()).filter(TypeTag::is_concrete),
            };
            let (column, nullable) = ty.as_ref().map_or((ColumnType::Json, false), column_for);
            let field_name = field.name.as_str();
            if !Named::is_valid(field_name) {
                self.unusable.push((
                    field_name.to_owned(),
                    "isn't a valid column name (lower snake case, at most 63 characters, \
                     not starting with `_`)",
                ));
                continue;
            }
            if self.columns.iter().any(|c| c.name == field_name) {
                self.unusable
                    .push((field_name.to_owned(), "clashes with a key column"));
                continue;
            }
            self.push(
                field_name,
                column,
                nullable,
                Projection::Field(row_index, field.name.clone()),
                false,
            );
        }
    }

    fn finish(self, diagnostics: &mut Vec<Diagnostic>) -> TableSchema {
        for (field, reason) in &self.unusable {
            diagnostics.push(
                Diagnostic::new(
                    format!(
                        "table `{}` can't store field `{field}` of `{}`: the name {reason}",
                        self.table, self.record
                    ),
                    self.span,
                )
                .help(
                    "build this table with `reduce` instead, where you name the columns and \
                     say what goes in each",
                ),
            );
        }
        TableSchema {
            columns: self.columns,
            key: self.key,
        }
    }
}

/// The column type a Move value of type `ty` is stored in, and whether it's nullable
/// (ADR 0008). `Option<T>` is a nullable `T`; structured values are JSON.
#[must_use]
pub fn column_for(ty: &TypeTag) -> (ColumnType, bool) {
    if let Some(tag) = ty.as_struct()
        && tag.name.is(Address::ONE, "option", "Option")
    {
        return match tag.type_args.first().map(column_for) {
            Some((inner, false)) => (inner, true),
            _ => (ColumnType::Json, true),
        };
    }
    let column = ColumnType::ALL
        .into_iter()
        .find(|c| *c != ColumnType::Json && c.accepts(ty, false))
        .unwrap_or(ColumnType::Json);
    (column, false)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn move_types_map_to_columns() {
        let ty = |s: &str| s.parse::<TypeTag>().unwrap();
        assert_eq!(column_for(&ty("u64")), (ColumnType::U64, false));
        assert_eq!(column_for(&ty("vector<u8>")), (ColumnType::Bytes, false));
        assert_eq!(column_for(&ty("vector<u64>")), (ColumnType::Json, false));
        assert_eq!(
            column_for(&ty("0x1::string::String")),
            (ColumnType::String, false)
        );
        assert_eq!(
            column_for(&ty("0x1::object::Object<0x1::fungible_asset::Metadata>")),
            (ColumnType::Address, false)
        );
        assert_eq!(
            column_for(&ty("0x1::option::Option<address>")),
            (ColumnType::Address, true)
        );
        assert_eq!(
            column_for(&ty("0x1::option::Option<0x1::option::Option<u8>>")),
            (ColumnType::Json, true)
        );
        assert_eq!(
            column_for(&ty("0x1::fungible_asset::Metadata")),
            (ColumnType::Json, false)
        );
    }
}
