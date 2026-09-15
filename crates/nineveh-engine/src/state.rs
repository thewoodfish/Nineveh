//! State as the fold sees it: keyed rows in tables, read through a [`StateView`] and
//! changed through a [`ChangeSet`].

use std::collections::BTreeMap;

use nineveh_core::{Value, Version};

/// Which table a row belongs to.
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Hash)]
pub enum TableId {
    /// A state table, by its index in the config's `state`.
    State(u32),
    /// Internal: table handles attributed to table sources (ADR 0012).
    /// Key `[handle, source]`, empty row.
    Handles,
    /// Internal: the last seen contents of each `SmartTable` bucket, for diffing.
    /// Key `[source, handle, bucket]`, row `[entries]`.
    Buckets,
}

/// A row's key: its key columns' values, in key order.
pub type Key = Vec<Value>;

/// A row: one value per column, in column order.
pub type Row = Vec<Value>;

/// What a view knows about one key.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Lookup {
    Absent,
    Present(Row),
    /// The view wasn't given this key. A database-backed view preloads the keys a
    /// batch touches; the fold reports any it missed so the caller can load them and
    /// fold again.
    NotLoaded,
}

/// Committed state, read-only. The fold never writes through a view; its writes come
/// back as a [`ChangeSet`] for the caller to commit atomically (ADR 0005).
pub trait StateView {
    fn get(&self, table: TableId, key: &[Value]) -> Lookup;
}

/// A batch's effect on state, ready to commit in one transaction with the cursor.
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct ChangeSet {
    /// The last version folded: the cursor to commit. `None` for an empty batch.
    pub last_version: Option<Version>,
    /// The final value of every row the batch changed; `None` deletes the row.
    pub writes: BTreeMap<(TableId, Key), Option<Row>>,
    /// Every change to a state-table row, in the order it happened: the change feed
    /// the outbox records (ADR 0006).
    pub changes: Vec<RowChange>,
}

/// One change to a state-table row.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct RowChange {
    pub version: Version,
    /// The state table's index in the config's `state`.
    pub table: u32,
    pub key: Key,
    pub kind: ChangeKind,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ChangeKind {
    Inserted,
    Updated,
    Deleted,
}

/// State held in memory: the reference [`StateView`], used by tests and replay
/// checks. Every key is loaded; a key it doesn't hold is absent.
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct MemoryState {
    tables: BTreeMap<TableId, BTreeMap<Key, Row>>,
    cursor: Option<Version>,
}

impl MemoryState {
    #[must_use]
    pub fn new() -> Self {
        Self::default()
    }

    /// Commit a change set: its writes and its cursor.
    pub fn apply(&mut self, changes: &ChangeSet) {
        for ((table, key), row) in &changes.writes {
            let rows = self.tables.entry(*table).or_default();
            match row {
                Some(row) => {
                    rows.insert(key.clone(), row.clone());
                }
                None => {
                    rows.remove(key);
                }
            }
        }
        self.tables.retain(|_, rows| !rows.is_empty());
        if changes.last_version.is_some() {
            self.cursor = changes.last_version;
        }
    }

    /// The last committed version.
    #[must_use]
    pub fn cursor(&self) -> Option<Version> {
        self.cursor
    }

    /// A table's rows, in key order.
    pub fn rows(&self, table: TableId) -> impl Iterator<Item = (&Key, &Row)> {
        self.tables.get(&table).into_iter().flatten()
    }
}

impl StateView for MemoryState {
    fn get(&self, table: TableId, key: &[Value]) -> Lookup {
        match self.tables.get(&table).and_then(|rows| rows.get(key)) {
            Some(row) => Lookup::Present(row.clone()),
            None => Lookup::Absent,
        }
    }
}
