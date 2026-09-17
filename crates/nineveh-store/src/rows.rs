//! Rendering a fold's rows the way the API serves them, for callers that hold rows
//! the store never wrote: the state-table preview folds a candidate config in memory
//! and shows what it would produce (ADR 0017).

use serde_json::Value as Json;

use nineveh_config::TableSchema;
use nineveh_core::Value;

use crate::cells;
use crate::error::StoreError;

/// One row of `schema`, as an object of its columns, exactly as the API would serve
/// it: `Option`s unwrapped, `Object<T>` read as its address, wide integers as strings
/// (ADR 0008).
///
/// # Errors
///
/// [`StoreError::Mismatch`] if the row doesn't fit the schema, which config
/// resolution rules out and so means a bug.
pub fn row_json(table: &str, schema: &TableSchema, row: &[Value]) -> Result<Json, StoreError> {
    cells::row_json(schema, row).map_err(|m| StoreError::Mismatch {
        table: table.to_owned(),
        column: m.column,
        reason: m.reason,
    })
}
