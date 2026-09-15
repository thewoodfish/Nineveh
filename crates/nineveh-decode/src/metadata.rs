//! Resource-group membership, read from a compiled module's metadata.
//!
//! REST ABIs don't say which structs are `#[resource_group_member]`s, but a group
//! member's delete arrives only as a delete of its group (ADR 0002), so the lock has to
//! know. The compiler records the attribute in the module's `aptos::metadata_v1`
//! entry, and this reads it from the bytecode `GET /v1/accounts/{a}/module/{m}` returns.
//!
//! Only as much of the binary format is parsed as that takes:
//!
//! - the header: magic `a1 1c eb 0b`, a little-endian `u32` version, then the table
//!   count and each table's `(kind, offset, length)`, all ULEB128 but the kind;
//! - the metadata table (kind `0x10`): `(key, value)` byte vectors until its length
//!   runs out;
//! - `aptos::metadata_v1`'s value, in BCS: an error map, then a map from struct name to
//!   attributes `(kind, args)`, where kind 3 is `resource_group_member` and its one
//!   argument is the group's name.
//!
//! Checked against `0x1::object` and `0x1::fungible_asset` from testnet
//! (`fixtures/modules/`).

use nineveh_core::{Identifier, StructName};

/// Why a module's metadata couldn't be read.
#[derive(Debug, Clone, PartialEq, Eq, thiserror::Error)]
#[non_exhaustive]
pub enum MetadataError {
    #[error("not a compiled Move module (bad magic bytes)")]
    NotAModule,
    #[error("the module's bytecode ends early, in {0}")]
    Truncated(&'static str),
    #[error("the module's metadata names `{0}`, which isn't a valid {1}")]
    BadName(String, &'static str),
}

const MAGIC: [u8; 4] = [0xa1, 0x1c, 0xeb, 0x0b];
const METADATA_TABLE: u8 = 0x10;
const METADATA_V1: &[u8] = b"aptos::metadata_v1";
const RESOURCE_GROUP_MEMBER: u8 = 3;

/// Every `#[resource_group_member]` struct the module declares, with its group.
///
/// A module without `aptos::metadata_v1` (older compilers write `v0`, which has no
/// struct attributes) declares none.
///
/// # Errors
///
/// If the bytes aren't a module, or its metadata doesn't parse.
pub fn resource_group_members(
    bytecode: &[u8],
) -> Result<Vec<(Identifier, StructName)>, MetadataError> {
    let mut header = Reader::new(bytecode);
    if header.bytes(4, "the magic")? != MAGIC {
        return Err(MetadataError::NotAModule);
    }
    header.bytes(4, "the version")?;
    let tables = header.uleb("the table count")?;
    let mut metadata = None;
    for _ in 0..tables {
        let kind = header.byte("a table header")?;
        let offset = header.uleb("a table header")?;
        let length = header.uleb("a table header")?;
        if kind == METADATA_TABLE {
            metadata = Some((offset, length));
        }
    }
    let Some((offset, length)) = metadata else {
        return Ok(Vec::new());
    };
    // Table offsets count from the end of the headers.
    let start = header
        .position
        .checked_add(offset)
        .ok_or(MetadataError::Truncated("the metadata table"))?;
    let end = start
        .checked_add(length)
        .ok_or(MetadataError::Truncated("the metadata table"))?;
    let table = bytecode
        .get(start..end)
        .ok_or(MetadataError::Truncated("the metadata table"))?;

    let mut entries = Reader::new(table);
    while !entries.is_empty() {
        let key_length = entries.uleb("a metadata key")?;
        let key = entries.bytes(key_length, "a metadata key")?;
        let value_length = entries.uleb("a metadata value")?;
        let value = entries.bytes(value_length, "a metadata value")?;
        if key == METADATA_V1 {
            return members(value);
        }
    }
    Ok(Vec::new())
}

/// The group members in a BCS `RuntimeModuleMetadataV1`.
fn members(value: &[u8]) -> Result<Vec<(Identifier, StructName)>, MetadataError> {
    let mut bcs = Reader::new(value);
    // error_map: BTreeMap<u64, ErrorDescription { code_name, code_description }>
    let errors = bcs.uleb("the error map")?;
    for _ in 0..errors {
        bcs.bytes(8, "the error map")?;
        bcs.string("the error map")?;
        bcs.string("the error map")?;
    }
    // struct_attributes: BTreeMap<String, Vec<KnownAttribute { kind: u8, args }>>
    let mut out = Vec::new();
    let structs = bcs.uleb("the struct attributes")?;
    for _ in 0..structs {
        let name = bcs.string("the struct attributes")?;
        let attributes = bcs.uleb("the struct attributes")?;
        for _ in 0..attributes {
            let kind = bcs.byte("the struct attributes")?;
            let count = bcs.uleb("the struct attributes")?;
            let mut args = Vec::new();
            for _ in 0..count {
                args.push(bcs.string("the struct attributes")?);
            }
            if kind == RESOURCE_GROUP_MEMBER
                && let [group] = args.as_slice()
            {
                let member = name
                    .parse::<Identifier>()
                    .map_err(|_| MetadataError::BadName(name.to_owned(), "struct name"))?;
                let group = group
                    .parse::<StructName>()
                    .map_err(|_| MetadataError::BadName((*group).to_owned(), "resource group"))?;
                out.push((member, group));
            }
        }
    }
    // Function attributes and anything later versions append aren't needed.
    Ok(out)
}

struct Reader<'b> {
    bytes: &'b [u8],
    position: usize,
}

impl<'b> Reader<'b> {
    fn new(bytes: &'b [u8]) -> Self {
        Self { bytes, position: 0 }
    }

    fn is_empty(&self) -> bool {
        self.position >= self.bytes.len()
    }

    fn byte(&mut self, what: &'static str) -> Result<u8, MetadataError> {
        Ok(self.bytes(1, what)?[0])
    }

    fn bytes(&mut self, n: usize, what: &'static str) -> Result<&'b [u8], MetadataError> {
        let end = self
            .position
            .checked_add(n)
            .ok_or(MetadataError::Truncated(what))?;
        let slice = self
            .bytes
            .get(self.position..end)
            .ok_or(MetadataError::Truncated(what))?;
        self.position = end;
        Ok(slice)
    }

    /// An unsigned LEB128 length or count, which Move caps at 32 bits.
    fn uleb(&mut self, what: &'static str) -> Result<usize, MetadataError> {
        let mut value: u64 = 0;
        for shift in (0..35).step_by(7) {
            let byte = self.byte(what)?;
            value |= u64::from(byte & 0x7f) << shift;
            if byte & 0x80 == 0 {
                return u32::try_from(value)
                    .ok()
                    .and_then(|v| usize::try_from(v).ok())
                    .ok_or(MetadataError::Truncated(what));
            }
        }
        Err(MetadataError::Truncated(what))
    }

    fn string(&mut self, what: &'static str) -> Result<&'b str, MetadataError> {
        let length = self.uleb(what)?;
        let bytes = self.bytes(length, what)?;
        std::str::from_utf8(bytes)
            .map_err(|_| MetadataError::BadName(String::from_utf8_lossy(bytes).into(), what))
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn rejects_what_isnt_a_module() {
        assert_eq!(
            resource_group_members(b"not a module"),
            Err(MetadataError::NotAModule)
        );
        assert!(matches!(
            resource_group_members(&MAGIC),
            Err(MetadataError::Truncated(_))
        ));
    }

    #[test]
    fn a_module_without_a_metadata_table_has_no_members() {
        // Magic, version 6, no tables.
        let bytes = [0xa1, 0x1c, 0xeb, 0x0b, 6, 0, 0, 0, 0];
        assert_eq!(resource_group_members(&bytes), Ok(Vec::new()));
    }
}
