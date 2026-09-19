//! Core domain types shared by every Nineveh crate.
//!
//! - chain identity: [`Version`], [`ChainId`], [`Network`];
//! - the Move vocabulary: [`Address`], [`Identifier`], [`TypeTag`] and [`StructTag`];
//! - exact integers up to 256 bits: [`U256`], [`I256`];
//! - decoded Move values: [`Value`], and [`Stored`], their storage encoding.
//!
//! This crate has no I/O and no async runtime. Everything else depends on it, so it
//! stays small and dependency-light. CI checks that it never picks up tokio, sqlx or
//! tonic.

mod address;
mod chain;
mod ident;
mod int;
mod stored;
mod type_tag;
mod value;

pub use address::{Address, InvalidAddress};
pub use chain::{ChainId, InvalidChainId, Network, UnknownNetwork, Version};
pub use ident::{Identifier, InvalidIdentifier};
pub use int::{I256, InvalidInteger, U256, parse_u256};
pub use stored::{InvalidStored, Stored};
pub use type_tag::{InvalidTypeTag, ParamOutOfRange, StructName, StructTag, TypeTag};
pub use value::{Fields, Value};
