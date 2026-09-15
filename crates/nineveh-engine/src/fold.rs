//! The fold: `StateView × [DecodedTransaction] → ChangeSet` (ADR 0005).

use std::collections::{BTreeMap, BTreeSet, HashMap};

use nineveh_config::{
    Column, CompiledExpr, Project, ResolvedAction, ResolvedTable, TableKind, Watcher,
};
use nineveh_core::{Address, Value, Version};
use nineveh_decode::{Container, DecodedTransaction, Origin, RecordData, SourceId, TypeMatcher};
use nineveh_expr::{Inputs, Tx};

use crate::containers::{bucket_entries, decode_entries, encode_entries, handle_in};
use crate::error::{FoldError, Halt};
use crate::state::{ChangeKind, ChangeSet, Key, Lookup, Row, RowChange, StateView, TableId};

/// A project's fold, precompiled: which tables and rules each source feeds.
///
/// Folding is pure: no I/O, clock or randomness. Transactions apply in version order
/// and records in their order within each transaction, so every key sees its changes
/// in order and a replay reproduces state exactly.
#[derive(Debug)]
pub struct Engine<'p> {
    project: &'p Project,
    plans: HashMap<SourceId, SourcePlan>,
    watchers: HashMap<SourceId, &'p Watcher>,
}

#[derive(Debug, Default)]
struct SourcePlan {
    /// `(table, rule)` pairs fired by events and writes.
    write_rules: Vec<(usize, usize)>,
    /// `(table, rule)` pairs fired by deletes.
    delete_rules: Vec<(usize, usize)>,
    mirrors: Vec<usize>,
    logs: Vec<usize>,
    /// A resource source matching every instantiation: its mirror rows are keyed by
    /// address and type.
    any_instance: bool,
}

impl<'p> Engine<'p> {
    #[must_use]
    pub fn new(project: &'p Project) -> Self {
        let mut plans: HashMap<SourceId, SourcePlan> = HashMap::new();
        for source in &project.config().sources {
            let Some(id) = project.source_id(source.name.as_str()) else {
                continue;
            };
            let any_instance = matches!(
                project.input(id),
                Some(nineveh_config::Input::Resource(TypeMatcher::AnyInstance(_)))
            );
            plans.entry(id).or_default().any_instance = any_instance;
        }
        for (t, table) in project.tables().iter().enumerate() {
            match table {
                ResolvedTable::Mirror { source } => {
                    plans.entry(*source).or_default().mirrors.push(t);
                }
                ResolvedTable::Log { source } => plans.entry(*source).or_default().logs.push(t),
                ResolvedTable::Reduce { rules } => {
                    for (r, rule) in rules.iter().enumerate() {
                        let plan = plans.entry(rule.source).or_default();
                        if rule.deleted {
                            plan.delete_rules.push((t, r));
                        } else {
                            plan.write_rules.push((t, r));
                        }
                    }
                }
            }
        }
        let watchers = project.watchers().iter().map(|w| (w.id, w)).collect();
        Self {
            project,
            plans,
            watchers,
        }
    }

    /// Fold a batch of transactions over committed state.
    ///
    /// # Errors
    ///
    /// [`FoldError::NotLoaded`] if the view lacked keys the batch reads (load them
    /// and fold again), or [`FoldError::Halt`] on a deterministic failure such as an
    /// arithmetic overflow in a rule.
    pub fn fold(
        &self,
        view: &dyn StateView,
        batch: &[DecodedTransaction],
    ) -> Result<ChangeSet, FoldError> {
        let mut fold = Fold {
            engine: self,
            view,
            overlay: BTreeMap::new(),
            changes: Vec::new(),
            missing: BTreeSet::new(),
        };
        let mut result = Ok(());
        for tx in batch {
            result = fold.transaction(tx);
            if result.is_err() {
                break;
            }
        }
        // A missing key makes everything after it suspect, a halt included.
        if !fold.missing.is_empty() {
            return Err(FoldError::NotLoaded(fold.missing.into_iter().collect()));
        }
        result.map_err(|halt| FoldError::Halt(Box::new(halt)))?;
        Ok(ChangeSet {
            last_version: batch.last().map(|tx| tx.version),
            writes: fold.overlay,
            changes: fold.changes,
        })
    }
}

struct Fold<'e, 'p, 'v> {
    engine: &'e Engine<'p>,
    view: &'v dyn StateView,
    overlay: BTreeMap<(TableId, Key), Option<Row>>,
    changes: Vec<RowChange>,
    missing: BTreeSet<(TableId, Key)>,
}

/// Where a record's fields come from, for building rule inputs. A struct field
/// shadows a built-in name, matching the scope config resolution computed.
#[derive(Default)]
struct RecordView<'a> {
    value: Option<&'a Value>,
    address: Option<Address>,
    handle: Option<Address>,
    key: Option<&'a Value>,
    item: Option<&'a Value>,
}

impl RecordView<'_> {
    fn field(&self, name: &str) -> Option<Value> {
        if let Some(v) = self.value.and_then(|v| v.field(name)) {
            return Some(v.clone());
        }
        match name {
            "address" => self.address.map(Value::Address),
            "handle" => self.handle.map(Value::Address),
            "key" => self.key.cloned(),
            "value" => self.item.cloned(),
            _ => None,
        }
    }
}

/// Where we are, for locating failures and stamping changes.
#[derive(Clone, Copy)]
struct At {
    version: Version,
    origin: Origin,
    tx: Tx,
}

/// `SmartTable` bucket writes seen in one transaction for one source and table:
/// bucket index to new contents (`None` for a deleted bucket).
type PendingBuckets =
    BTreeMap<(SourceId, Address), (At, BTreeMap<u64, Option<Vec<(Value, Value)>>>)>;

impl Fold<'_, '_, '_> {
    fn get(&mut self, table: TableId, key: &[Value]) -> Option<Row> {
        if let Some(row) = self.overlay.get(&(table, key.to_vec())) {
            return row.clone();
        }
        match self.view.get(table, key) {
            Lookup::Present(row) => Some(row),
            Lookup::Absent => None,
            Lookup::NotLoaded => {
                self.missing.insert((table, key.to_vec()));
                None
            }
        }
    }

    /// Write a state-table row, recording the change if there is one.
    fn put_state(&mut self, table: usize, key: Key, old: Option<&Row>, new: Option<Row>, at: At) {
        if old == new.as_ref() {
            return;
        }
        let kind = match (old, &new) {
            (None, _) => ChangeKind::Inserted,
            (Some(_), Some(_)) => ChangeKind::Updated,
            (Some(_), None) => ChangeKind::Deleted,
        };
        let index = u32::try_from(table).unwrap_or(u32::MAX);
        self.changes.push(RowChange {
            version: at.version,
            table: index,
            key: key.clone(),
            kind,
            row: new.clone(),
        });
        self.overlay.insert((TableId::State(index), key), new);
    }

    fn transaction(&mut self, tx: &DecodedTransaction) -> Result<(), Halt> {
        let base = At {
            version: tx.version,
            origin: Origin::Event(0),
            tx: Tx {
                version: tx.version.get(),
                timestamp_micros: tx.timestamp_micros,
            },
        };

        // Pass 1: learn handles, so items written alongside their parent route
        // correctly (ADR 0012).
        for record in &tx.records {
            if let Some(watcher) = self.engine.watchers.get(&record.source) {
                self.learn_handle(watcher, &record.data);
            }
        }

        // Pass 2: everything else, in order.
        let mut buckets = PendingBuckets::new();
        for record in &tx.records {
            if self.engine.watchers.contains_key(&record.source) {
                continue;
            }
            let at = At {
                origin: record.origin,
                ..base
            };
            self.record(record.source, &record.data, at, &mut buckets)?;
        }

        // SmartTable buckets net out at the end of the transaction, so entries that
        // moved between buckets in a split are neither deleted nor re-announced.
        for ((source, handle), (at, contents)) in buckets {
            self.net_buckets(source, handle, &contents, at)?;
        }
        Ok(())
    }

    fn learn_handle(&mut self, watcher: &Watcher, data: &RecordData) {
        let (RecordData::ResourceWrite { value: parent, .. }
        | RecordData::TableWrite { value: parent, .. }
        | RecordData::TableValue { value: parent, .. }) = data
        else {
            return;
        };
        let Some(handle) = handle_in(parent, watcher.field.as_str(), watcher.container) else {
            return;
        };
        let key = vec![Value::Address(handle), source_value(watcher.table_source)];
        if self.get(TableId::Handles, &key).is_none() {
            self.overlay
                .insert((TableId::Handles, key), Some(Vec::new()));
        }
    }

    fn attributed(&mut self, source: SourceId, handle: Address) -> bool {
        let key = vec![Value::Address(handle), source_value(source)];
        self.get(TableId::Handles, &key).is_some()
    }

    fn record(
        &mut self,
        source: SourceId,
        data: &RecordData,
        at: At,
        buckets: &mut PendingBuckets,
    ) -> Result<(), Halt> {
        match data {
            RecordData::Event { value, .. } => {
                let view = RecordView {
                    value: Some(value),
                    ..RecordView::default()
                };
                self.rules(source, false, &view, at)?;
                self.log(source, value, at);
            }
            RecordData::ResourceWrite { address, ty, value } => {
                let key = self.resource_key(source, *address, &ty.to_string());
                let mut row = key.clone();
                row.push(value.clone());
                self.mirror(source, &key, Some(&row), at);
                let view = RecordView {
                    value: Some(value),
                    address: Some(*address),
                    ..RecordView::default()
                };
                self.rules(source, false, &view, at)?;
            }
            RecordData::ResourceDelete { address, ty } => {
                let key = self.resource_key(source, *address, &ty.to_string());
                self.resource_deleted(source, &key, *address, at)?;
            }
            RecordData::GroupDelete { address, .. } => {
                // Config resolution guarantees group members are exact types, keyed
                // by address alone.
                self.resource_deleted(source, &vec![Value::Address(*address)], *address, at)?;
            }
            RecordData::TableWrite {
                container,
                handle,
                key,
                value,
            } => {
                if !self.attributed(source, *handle) {
                    return Ok(());
                }
                match container {
                    Container::SmartTable => {
                        let bucket = bucket_index(key);
                        let entries = bucket_entries(value);
                        buckets
                            .entry((source, *handle))
                            .or_insert_with(|| (at, BTreeMap::new()))
                            .1
                            .insert(bucket, Some(entries.unwrap_or_default()));
                    }
                    _ => self.item_written(source, *handle, key, value, at)?,
                }
            }
            // Only watchers select items by value alone, and they're handled above.
            RecordData::TableValue { .. } => {}
            RecordData::TableDelete {
                container,
                handle,
                key,
            } => {
                if !self.attributed(source, *handle) {
                    return Ok(());
                }
                match container {
                    Container::SmartTable => {
                        buckets
                            .entry((source, *handle))
                            .or_insert_with(|| (at, BTreeMap::new()))
                            .1
                            .insert(bucket_index(key), None);
                    }
                    _ => self.item_deleted(source, *handle, key, at)?,
                }
            }
        }
        Ok(())
    }

    fn resource_key(&self, source: SourceId, address: Address, ty: &str) -> Key {
        let any_instance = self
            .engine
            .plans
            .get(&source)
            .is_some_and(|p| p.any_instance);
        if any_instance {
            vec![Value::Address(address), Value::String(ty.to_owned())]
        } else {
            vec![Value::Address(address)]
        }
    }

    fn resource_deleted(
        &mut self,
        source: SourceId,
        key: &Key,
        address: Address,
        at: At,
    ) -> Result<(), Halt> {
        self.mirror(source, key, None, at);
        let view = RecordView {
            address: Some(address),
            ..RecordView::default()
        };
        self.rules(source, true, &view, at)
    }

    fn item_written(
        &mut self,
        source: SourceId,
        handle: Address,
        key: &Value,
        value: &Value,
        at: At,
    ) -> Result<(), Halt> {
        let row_key = vec![Value::Address(handle), key.clone()];
        let row = vec![Value::Address(handle), key.clone(), value.clone()];
        self.mirror(source, &row_key, Some(&row), at);
        let view = RecordView {
            handle: Some(handle),
            key: Some(key),
            item: Some(value),
            ..RecordView::default()
        };
        self.rules(source, false, &view, at)
    }

    fn item_deleted(
        &mut self,
        source: SourceId,
        handle: Address,
        key: &Value,
        at: At,
    ) -> Result<(), Halt> {
        self.mirror(source, &vec![Value::Address(handle), key.clone()], None, at);
        let view = RecordView {
            handle: Some(handle),
            key: Some(key),
            ..RecordView::default()
        };
        self.rules(source, true, &view, at)
    }

    /// Compare the buckets a transaction rewrote with what they held before, as one
    /// logical map, and apply the per-entry differences: deletes, then writes.
    fn net_buckets(
        &mut self,
        source: SourceId,
        handle: Address,
        contents: &BTreeMap<u64, Option<Vec<(Value, Value)>>>,
        at: At,
    ) -> Result<(), Halt> {
        let mut before = BTreeMap::new();
        let mut after = BTreeMap::new();
        for (bucket, entries) in contents {
            let key = vec![
                source_value(source),
                Value::Address(handle),
                Value::U64(*bucket),
            ];
            if let Some(row) = self.get(TableId::Buckets, &key) {
                before.extend(row.first().map(decode_entries).unwrap_or_default());
            }
            let entries = entries.clone().unwrap_or_default();
            after.extend(entries.iter().cloned());
            let stored = (!entries.is_empty()).then(|| vec![encode_entries(&entries)]);
            self.overlay.insert((TableId::Buckets, key), stored);
        }
        for key in before.keys() {
            if !after.contains_key(key) {
                self.item_deleted(source, handle, key, at)?;
            }
        }
        for (key, value) in &after {
            if before.get(key) != Some(value) {
                self.item_written(source, handle, key, value, at)?;
            }
        }
        Ok(())
    }

    fn mirror(&mut self, source: SourceId, key: &Key, row: Option<&Row>, at: At) {
        let tables = self
            .engine
            .plans
            .get(&source)
            .map(|p| p.mirrors.clone())
            .unwrap_or_default();
        for table in tables {
            let old = self.get(state_id(table), key);
            self.put_state(table, key.clone(), old.as_ref(), row.cloned(), at);
        }
    }

    fn log(&mut self, source: SourceId, value: &Value, at: At) {
        let tables = self
            .engine
            .plans
            .get(&source)
            .map(|p| p.logs.clone())
            .unwrap_or_default();
        let index = match at.origin {
            Origin::Event(i) | Origin::Change(i) => i,
        };
        for table in tables {
            let key = vec![Value::U64(at.version.get()), Value::U32(index)];
            let mut row = key.clone();
            row.push(value.clone());
            self.put_state(table, key, None, Some(row), at);
        }
    }

    fn rules(
        &mut self,
        source: SourceId,
        deleted: bool,
        record: &RecordView<'_>,
        at: At,
    ) -> Result<(), Halt> {
        let rules = self
            .engine
            .plans
            .get(&source)
            .map(|p| {
                if deleted {
                    p.delete_rules.clone()
                } else {
                    p.write_rules.clone()
                }
            })
            .unwrap_or_default();
        for (table, rule) in rules {
            self.rule(table, rule, record, at)?;
        }
        Ok(())
    }

    fn rule(
        &mut self,
        table: usize,
        rule_index: usize,
        record: &RecordView<'_>,
        at: At,
    ) -> Result<(), Halt> {
        let project = self.engine.project;
        let state = &project.config().state[table];
        let TableKind::Reduce {
            key: key_names,
            columns,
            ..
        } = &state.kind
        else {
            return Ok(());
        };
        let ResolvedTable::Reduce { rules } = &project.tables()[table] else {
            return Ok(());
        };
        let rule = &rules[rule_index];
        let halt = |message: String, span| Halt {
            version: at.version,
            origin: Some(at.origin),
            table: Some(state.name.name.clone()),
            rule: Some(rule_index),
            message,
            span,
        };

        let mut inputs_record = Vec::with_capacity(rule.scope.fields.len());
        for (name, _) in &rule.scope.fields {
            let value = record.field(name.as_str()).ok_or_else(|| {
                halt(
                    format!("internal error: the record has no field `{name}`"),
                    None,
                )
            })?;
            inputs_record.push(value);
        }

        let eval = |expr: &CompiledExpr, row: &[Value]| {
            expr.compiled
                .eval(&Inputs {
                    row,
                    record: &inputs_record,
                    tx: at.tx,
                })
                .map_err(|e| {
                    halt(
                        format!("`{}`: {e}", expr.source.text),
                        expr.yaml_span(e.span).or(expr.source.span),
                    )
                })
        };

        let mut key = Vec::with_capacity(rule.key.len());
        for (_, expr) in &rule.key {
            key.push(eval(expr, &[])?);
        }

        let old = self.get(state_id(table), &key);
        let (current, mut unset) = match &old {
            Some(row) => (row.clone(), vec![false; row.len()]),
            None => new_row(columns, key_names, &key),
        };

        if let Some(when) = &rule.when
            && eval(when, &current)? != Value::Bool(true)
        {
            return Ok(());
        }

        match &rule.action {
            ResolvedAction::Delete => {
                if old.is_some() {
                    self.put_state(table, key, old.as_ref(), None, at);
                }
            }
            ResolvedAction::Set(assignments) => {
                // Every expression sees the row as it was before this rule.
                let mut next = current.clone();
                for (column, expr) in assignments {
                    let Some(index) = columns.iter().position(|c| c.name.name == *column) else {
                        continue;
                    };
                    next[index] = eval(expr, &current)?;
                    unset[index] = false;
                }
                if let Some(index) = unset.iter().position(|&u| u) {
                    return Err(halt(
                        format!(
                            "internal error: a new row has no value for `{}`",
                            columns[index].name
                        ),
                        None,
                    ));
                }
                self.put_state(table, key, old.as_ref(), Some(next), at);
            }
        }
        Ok(())
    }
}

/// A new row: key columns from the key, then defaults, null for nullable columns,
/// and a placeholder (flagged in the returned mask) for columns a rule must set.
fn new_row(
    columns: &[Column],
    key_names: &[nineveh_config::Named],
    key: &[Value],
) -> (Row, Vec<bool>) {
    let mut row = Vec::with_capacity(columns.len());
    let mut unset = Vec::with_capacity(columns.len());
    for column in columns {
        let key_value = key_names
            .iter()
            .position(|k| k.name == column.name.name)
            .and_then(|i| key.get(i));
        let value = match (key_value, &column.default, column.nullable) {
            (Some(k), _, _) => Some(k.clone()),
            (None, Some(d), true) => Some(Value::Option(Some(Box::new(d.clone())))),
            (None, Some(d), false) => Some(d.clone()),
            (None, None, true) => Some(Value::Option(None)),
            (None, None, false) => None,
        };
        unset.push(value.is_none());
        // The typechecker guarantees nothing reads a placeholder.
        row.push(value.unwrap_or(Value::Bool(false)));
    }
    (row, unset)
}

fn state_id(table: usize) -> TableId {
    TableId::State(u32::try_from(table).unwrap_or(u32::MAX))
}

fn source_value(source: SourceId) -> Value {
    Value::U32(source.0)
}

fn bucket_index(key: &Value) -> u64 {
    match key {
        Value::U64(i) => *i,
        _ => u64::MAX,
    }
}
