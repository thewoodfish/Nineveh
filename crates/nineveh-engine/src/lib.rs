//! The fold at Nineveh's core (ADR 0005): decoded records in, state changes out.
//!
//! [`Engine::fold`] takes committed state through a [`StateView`] and a batch of
//! decoded transactions, and returns a [`ChangeSet`]: every row write, the row-level
//! change feed, and the cursor, for the caller to commit in one database
//! transaction. It does no I/O and reads no clock or randomness, so folding the same
//! inputs always gives the same result. That's what makes crash recovery a replay
//! from the cursor, and a rebuild a replay from `start_version`.
//!
//! State tables are built three ways (ADR 0011): `reduce` rules run their compiled
//! expressions, `mirror` tables track the latest value of each resource or table
//! item, and `log` tables append one row per event. Table items count only once their
//! handle has been learned from a decoded parent (ADR 0012); `SmartTable` buckets are
//! diffed into per-entry changes.

mod containers;
mod error;
mod fold;
mod state;

pub use error::{FoldError, Halt};
pub use fold::Engine;
pub use state::{
    ChangeKind, ChangeSet, Key, Lookup, MemoryState, Row, RowChange, StateView, TableId,
};
