//! What the engine knows about the framework's table containers: where each keeps its
//! handle, and what a `SmartTable` bucket holds.

use nineveh_core::{Address, Value};
use nineveh_decode::Container;

/// The handle of the table held in `field` of a parent value (ADR 0012).
///
/// - `Table`: `field.handle`; `TableWithLength`: `field.inner.handle`.
/// - `SmartTable`: `field.buckets.inner.handle` (its buckets live in a
///   `TableWithLength`).
///
/// `BigOrderedMap` isn't read yet; config resolution rejects those sources.
pub(crate) fn handle_in(parent: &Value, field: &str, container: Container) -> Option<Address> {
    let held = parent.field(field)?;
    match container {
        Container::Table => table_handle(held),
        Container::SmartTable => table_handle(held.field("buckets")?),
        Container::BigOrderedMap => None,
    }
}

/// The handle of a `Table` or `TableWithLength` value.
fn table_handle(value: &Value) -> Option<Address> {
    match value.field("handle") {
        Some(Value::Address(handle)) => Some(*handle),
        _ => match value.field("inner")?.field("handle")? {
            Value::Address(handle) => Some(*handle),
            _ => None,
        },
    }
}

/// The entries of a `SmartTable` bucket: a `vector<Entry<K, V>>` of
/// `{hash, key, value}`.
pub(crate) fn bucket_entries(bucket: &Value) -> Option<Vec<(Value, Value)>> {
    let Value::Vector(items) = bucket else {
        return None;
    };
    items
        .iter()
        .map(|entry| Some((entry.field("key")?.clone(), entry.field("value")?.clone())))
        .collect()
}

/// A bucket's entries as stored in the engine's internal `Buckets` table: a vector of
/// `[key, value]` pairs.
pub(crate) fn encode_entries(entries: &[(Value, Value)]) -> Value {
    Value::Vector(
        entries
            .iter()
            .map(|(k, v)| Value::Vector(vec![k.clone(), v.clone()]))
            .collect(),
    )
}

pub(crate) fn decode_entries(stored: &Value) -> Vec<(Value, Value)> {
    let Value::Vector(pairs) = stored else {
        return Vec::new();
    };
    pairs
        .iter()
        .filter_map(|pair| match pair {
            Value::Vector(kv) if kv.len() == 2 => Some((kv[0].clone(), kv[1].clone())),
            _ => None,
        })
        .collect()
}

#[cfg(test)]
mod tests {
    use super::*;

    fn s(fields: Vec<(&str, Value)>) -> Value {
        Value::Struct(
            fields
                .into_iter()
                .map(|(n, v)| (n.parse().unwrap(), v))
                .collect(),
        )
    }

    #[test]
    fn finds_handles_through_each_container() {
        let h = Address::special(0xa);
        let table = s(vec![("handle", Value::Address(h))]);
        let with_length = s(vec![("inner", table.clone()), ("length", Value::U64(1))]);
        let smart = s(vec![
            ("buckets", with_length.clone()),
            ("size", Value::U64(1)),
        ]);
        let parent = s(vec![("t", table), ("w", with_length), ("st", smart)]);
        assert_eq!(handle_in(&parent, "t", Container::Table), Some(h));
        assert_eq!(handle_in(&parent, "w", Container::Table), Some(h));
        assert_eq!(handle_in(&parent, "st", Container::SmartTable), Some(h));
        assert_eq!(handle_in(&parent, "nope", Container::Table), None);
    }

    #[test]
    fn bucket_entries_round_trip() {
        let bucket = Value::Vector(vec![s(vec![
            ("hash", Value::U64(9)),
            ("key", Value::Address(Address::ONE)),
            ("value", Value::U64(5)),
        ])]);
        let entries = bucket_entries(&bucket).unwrap();
        assert_eq!(entries, [(Value::Address(Address::ONE), Value::U64(5))]);
        assert_eq!(decode_entries(&encode_entries(&entries)), entries);
    }
}
