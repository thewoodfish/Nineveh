//! Engine rows as typed columns: the arrays an upsert binds, and the JSON the outbox
//! records. Both follow ADR 0008. `Option` is null or its value, `Object<T>` is its
//! address, integers wider than 32 bits are exact decimals, and structured values are
//! JSON with wide integers as strings.
//!
//! Postgres text and JSON can't hold U+0000, which Move strings can. The typed
//! columns replace it with U+FFFD; the exact value stays in `_row` and `_key`, so
//! the fold never sees the replacement.

use std::borrow::Cow;

use nineveh_config::{ColumnType, Projection, SchemaColumn, TableSchema};
use nineveh_core::Value;
use serde_json::{Map, Value as Json};

/// A value that doesn't fit its column. The config's type checks rule this out, so
/// it's an internal error.
#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) struct Mismatch {
    pub(crate) column: String,
    pub(crate) reason: String,
}

/// One column's values for a batch of rows, as bound to its array parameter.
#[derive(Debug)]
pub(crate) enum Cells {
    Bool(Vec<Option<bool>>),
    Int4(Vec<Option<i32>>),
    Int8(Vec<Option<i64>>),
    /// Numerics, addresses, strings and JSON, each cast from text in SQL.
    Text(Vec<Option<String>>),
    Bytea(Vec<Option<Vec<u8>>>),
}

impl Cells {
    fn new(ty: ColumnType, capacity: usize) -> Self {
        match ty {
            ColumnType::Bool => Self::Bool(Vec::with_capacity(capacity)),
            ColumnType::U8
            | ColumnType::U16
            | ColumnType::I8
            | ColumnType::I16
            | ColumnType::I32 => Self::Int4(Vec::with_capacity(capacity)),
            ColumnType::U32 | ColumnType::I64 => Self::Int8(Vec::with_capacity(capacity)),
            ColumnType::Bytes => Self::Bytea(Vec::with_capacity(capacity)),
            ColumnType::U64
            | ColumnType::U128
            | ColumnType::U256
            | ColumnType::I128
            | ColumnType::I256
            | ColumnType::Address
            | ColumnType::String
            | ColumnType::Json => Self::Text(Vec::with_capacity(capacity)),
        }
    }

    fn push_null(&mut self) {
        match self {
            Self::Bool(v) => v.push(None),
            Self::Int4(v) => v.push(None),
            Self::Int8(v) => v.push(None),
            Self::Text(v) => v.push(None),
            Self::Bytea(v) => v.push(None),
        }
    }

    fn push(&mut self, ty: ColumnType, value: &Value) -> Result<(), String> {
        let wrong = || Err(format!("a {ty} column can't hold {value:?}"));
        match (self, ty, value) {
            (Self::Bool(v), ColumnType::Bool, Value::Bool(b)) => v.push(Some(*b)),
            (Self::Int4(v), ColumnType::U8, Value::U8(n)) => v.push(Some(i32::from(*n))),
            (Self::Int4(v), ColumnType::U16, Value::U16(n)) => v.push(Some(i32::from(*n))),
            (Self::Int4(v), ColumnType::I8, Value::I8(n)) => v.push(Some(i32::from(*n))),
            (Self::Int4(v), ColumnType::I16, Value::I16(n)) => v.push(Some(i32::from(*n))),
            (Self::Int4(v), ColumnType::I32, Value::I32(n)) => v.push(Some(*n)),
            (Self::Int8(v), ColumnType::U32, Value::U32(n)) => v.push(Some(i64::from(*n))),
            (Self::Int8(v), ColumnType::I64, Value::I64(n)) => v.push(Some(*n)),
            (Self::Text(v), ColumnType::U64, Value::U64(n)) => v.push(Some(n.to_string())),
            (Self::Text(v), ColumnType::U128, Value::U128(n)) => v.push(Some(n.to_string())),
            (Self::Text(v), ColumnType::U256, Value::U256(n)) => v.push(Some(n.to_string())),
            (Self::Text(v), ColumnType::I128, Value::I128(n)) => v.push(Some(n.to_string())),
            (Self::Text(v), ColumnType::I256, Value::I256(n)) => v.push(Some(n.to_string())),
            (Self::Text(v), ColumnType::Address, Value::Address(a)) => v.push(Some(a.to_string())),
            (Self::Text(v), ColumnType::String, Value::String(s)) => v.push(Some(text(s))),
            (Self::Text(v), ColumnType::Json, value) => v.push(Some(json(value)?.to_string())),
            (Self::Bytea(v), ColumnType::Bytes, Value::Bytes(b)) => v.push(Some(b.clone())),
            _ => return wrong(),
        }
        Ok(())
    }
}

/// The typed columns of a batch of rows, one [`Cells`] per column.
pub(crate) fn columns(schema: &TableSchema, rows: &[&[Value]]) -> Result<Vec<Cells>, Mismatch> {
    schema
        .columns
        .iter()
        .map(|column| {
            let mut cells = Cells::new(column.ty, rows.len());
            for row in rows {
                let result = match cell(column, project(column, row)?)? {
                    None => {
                        cells.push_null();
                        Ok(())
                    }
                    Some(value) => cells.push(column.ty, &value),
                };
                result.map_err(|reason| mismatch(column, reason))?;
            }
            Ok(cells)
        })
        .collect()
}

/// A row as the API sees it: an object of its columns.
pub(crate) fn row_json(schema: &TableSchema, row: &[Value]) -> Result<Json, Mismatch> {
    let mut object = Map::new();
    for column in &schema.columns {
        let value = cell(column, project(column, row)?)?;
        object.insert(column.name.clone(), column_json(column, value)?);
    }
    Ok(Json::Object(object))
}

/// A key as the API sees it: an object of its key columns.
pub(crate) fn key_json(schema: &TableSchema, key: &[Value]) -> Result<Json, Mismatch> {
    let mut object = Map::new();
    for (i, &index) in schema.key.iter().enumerate() {
        let column = &schema.columns[index];
        let raw = key
            .get(i)
            .ok_or_else(|| mismatch(column, "missing from the key"))?;
        let value = cell(column, raw)?;
        object.insert(column.name.clone(), column_json(column, value)?);
    }
    Ok(Json::Object(object))
}

fn column_json(column: &SchemaColumn, value: Option<Cow<'_, Value>>) -> Result<Json, Mismatch> {
    value.map_or(Ok(Json::Null), |v| {
        json(&v).map_err(|reason| mismatch(column, reason))
    })
}

/// The value a column takes from the engine's row.
fn project<'r>(column: &SchemaColumn, row: &'r [Value]) -> Result<&'r Value, Mismatch> {
    let value = match &column.from {
        Projection::Row(i) => row.get(*i),
        Projection::Field(i, field) => row.get(*i).and_then(|v| v.field(field.as_str())),
    };
    value.ok_or_else(|| mismatch(column, "the engine's row has no value for it"))
}

/// A column's value, or `None` for null: `Option` unwrapped for nullable columns, and
/// `Object<T>` read as its address.
fn cell<'v>(column: &SchemaColumn, value: &'v Value) -> Result<Option<Cow<'v, Value>>, Mismatch> {
    let value = if column.nullable {
        match value {
            Value::Option(None) => return Ok(None),
            Value::Option(Some(inner)) => inner,
            other => {
                return Err(mismatch(
                    column,
                    format!("a nullable column needs an option, got {other:?}"),
                ));
            }
        }
    } else {
        value
    };
    if column.ty == ColumnType::Address
        && let Value::Struct(fields) = value
        && let [(name, Value::Address(address))] = fields.as_slice()
        && name.as_str() == "inner"
    {
        return Ok(Some(Cow::Owned(Value::Address(*address))));
    }
    Ok(Some(Cow::Borrowed(value)))
}

fn json(value: &Value) -> Result<Json, String> {
    let mut json = serde_json::to_value(value).map_err(|e| e.to_string())?;
    strip_nul(&mut json);
    Ok(json)
}

fn text(s: &str) -> String {
    s.replace('\0', "\u{FFFD}")
}

fn strip_nul(json: &mut Json) {
    match json {
        Json::String(s) if s.contains('\0') => *s = text(s),
        Json::Array(items) => items.iter_mut().for_each(strip_nul),
        Json::Object(map) => map.values_mut().for_each(strip_nul),
        _ => {}
    }
}

fn mismatch(column: &SchemaColumn, reason: impl Into<String>) -> Mismatch {
    Mismatch {
        column: column.name.clone(),
        reason: reason.into(),
    }
}

#[cfg(test)]
mod tests {
    use nineveh_core::{Address, Identifier};

    use super::*;

    fn column(name: &str, ty: ColumnType, nullable: bool, from: Projection) -> SchemaColumn {
        SchemaColumn {
            name: name.into(),
            ty,
            nullable,
            from,
        }
    }

    fn ident(s: &str) -> Identifier {
        s.parse().unwrap()
    }

    /// A mirror of a resource: `address`, then the value's fields.
    fn mirror() -> TableSchema {
        TableSchema {
            columns: vec![
                column("address", ColumnType::Address, false, Projection::Row(0)),
                column(
                    "balance",
                    ColumnType::U64,
                    false,
                    Projection::Field(1, ident("balance")),
                ),
                column(
                    "metadata",
                    ColumnType::Address,
                    false,
                    Projection::Field(1, ident("metadata")),
                ),
                column(
                    "memo",
                    ColumnType::String,
                    true,
                    Projection::Field(1, ident("memo")),
                ),
                column(
                    "extra",
                    ColumnType::Json,
                    false,
                    Projection::Field(1, ident("extra")),
                ),
            ],
            key: vec![0],
        }
    }

    fn row() -> Vec<Value> {
        let object = Value::Struct(vec![(
            ident("inner"),
            Value::Address(Address::special(0xa)),
        )]);
        vec![
            Value::Address(Address::ONE),
            Value::Struct(vec![
                (ident("balance"), Value::U64(18_441_553_330_519_219_599)),
                (ident("metadata"), object),
                (
                    ident("memo"),
                    Value::Option(Some(Box::new(Value::String("a\0b".into())))),
                ),
                (
                    ident("extra"),
                    Value::Vector(vec![Value::U128(1), Value::String("\0".into())]),
                ),
            ]),
        ]
    }

    #[test]
    fn rows_become_typed_cells() {
        let row = row();
        let cells = columns(&mirror(), &[row.as_slice()]).unwrap();
        let text = |i: usize| match &cells[i] {
            Cells::Text(v) => v[0].clone().unwrap(),
            other => panic!("{other:?}"),
        };
        assert_eq!(text(0), Address::ONE.to_string());
        assert_eq!(
            text(1),
            "18441553330519219599",
            "u64 above i64::MAX is exact"
        );
        assert_eq!(
            text(2),
            Address::special(0xa).to_string(),
            "an Object is its address"
        );
        assert_eq!(text(3), "a\u{FFFD}b", "NUL is replaced in text");
        assert_eq!(
            text(4),
            r#"["1","�"]"#,
            "JSON has integers as strings, and no NUL"
        );
    }

    #[test]
    fn rows_and_keys_render_for_the_api() {
        let schema = mirror();
        let row = row();
        let json = row_json(&schema, &row).unwrap();
        assert_eq!(json["balance"], "18441553330519219599");
        assert_eq!(json["memo"], "a\u{FFFD}b");
        assert_eq!(
            key_json(&schema, &row[..1]).unwrap(),
            serde_json::json!({ "address": Address::ONE.to_string() })
        );

        let mut none = row.clone();
        let Value::Struct(fields) = &mut none[1] else {
            unreachable!()
        };
        fields[2].1 = Value::Option(None);
        assert_eq!(row_json(&schema, &none).unwrap()["memo"], Json::Null);
    }

    #[test]
    fn values_that_dont_fit_are_internal_errors() {
        let schema = TableSchema {
            columns: vec![column("n", ColumnType::U64, false, Projection::Row(0))],
            key: vec![0],
        };
        let row = [Value::U32(1)];
        let err = columns(&schema, &[row.as_slice()]).unwrap_err();
        assert_eq!(err.column, "n");
    }
}
