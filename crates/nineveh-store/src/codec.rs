//! The exact encoding of engine keys and rows, stored in each table's `_key` and
//! `_row` columns.
//!
//! Typed columns serve the API, but they can't give the fold back exactly what it
//! wrote: a `json` column forgets its Move types, and text can't hold a NUL. So the
//! fold reads and writes this encoding instead. It's self-delimiting and one-to-one:
//! two values encode to the same bytes exactly when they're equal, which lets `_key`
//! be the primary key for keys of any shape, structs included.
//!
//! A value is a tag byte, then:
//!
//! | Value | Payload |
//! | --- | --- |
//! | `bool` | one byte, 0 or 1 |
//! | integers | big-endian, at their width (signed: two's complement) |
//! | address | 32 bytes |
//! | string, bytes | length, then the bytes |
//! | vector | length, then each value |
//! | option | tag `NONE`, or tag `SOME` then the value |
//! | struct | field count, then each field's name and value |
//! | enum variant | the variant's name, then its fields as for a struct |
//!
//! Lengths and counts are unsigned LEB128. A key or row is a count, then its values.

use nineveh_core::{Address, I256, Identifier, U256, Value};

/// Why stored bytes didn't decode. The store only reads bytes it wrote, so this means
/// the table was changed by something other than Nineveh.
#[derive(Debug, Clone, PartialEq, Eq, thiserror::Error)]
#[error("{0}")]
pub(crate) struct CodecError(&'static str);

/// Nesting deeper than this is refused rather than risking the stack. Move limits
/// value depth far below it.
const MAX_DEPTH: usize = 256;

mod tag {
    pub(super) const FALSE: u8 = 0;
    pub(super) const TRUE: u8 = 1;
    pub(super) const U8: u8 = 2;
    pub(super) const U16: u8 = 3;
    pub(super) const U32: u8 = 4;
    pub(super) const U64: u8 = 5;
    pub(super) const U128: u8 = 6;
    pub(super) const U256: u8 = 7;
    pub(super) const I8: u8 = 8;
    pub(super) const I16: u8 = 9;
    pub(super) const I32: u8 = 10;
    pub(super) const I64: u8 = 11;
    pub(super) const I128: u8 = 12;
    pub(super) const I256: u8 = 13;
    pub(super) const ADDRESS: u8 = 14;
    pub(super) const STRING: u8 = 15;
    pub(super) const BYTES: u8 = 16;
    pub(super) const VECTOR: u8 = 17;
    pub(super) const NONE: u8 = 18;
    pub(super) const SOME: u8 = 19;
    pub(super) const STRUCT: u8 = 20;
    pub(super) const VARIANT: u8 = 21;
}

/// Encode a key or row.
pub(crate) fn encode(values: &[Value]) -> Vec<u8> {
    let mut out = Vec::new();
    length(&mut out, values.len());
    for value in values {
        encode_value(&mut out, value);
    }
    out
}

/// Decode a key or row, all of `bytes`.
pub(crate) fn decode(bytes: &[u8]) -> Result<Vec<Value>, CodecError> {
    let mut reader = Reader { bytes, depth: 0 };
    let n = reader.length()?;
    let mut values = Vec::with_capacity(n.min(bytes.len()));
    for _ in 0..n {
        values.push(reader.value()?);
    }
    if !reader.bytes.is_empty() {
        return Err(CodecError("trailing bytes"));
    }
    Ok(values)
}

fn encode_value(out: &mut Vec<u8>, value: &Value) {
    match value {
        Value::Bool(b) => out.push(if *b { tag::TRUE } else { tag::FALSE }),
        Value::U8(n) => tagged(out, tag::U8, &n.to_be_bytes()),
        Value::U16(n) => tagged(out, tag::U16, &n.to_be_bytes()),
        Value::U32(n) => tagged(out, tag::U32, &n.to_be_bytes()),
        Value::U64(n) => tagged(out, tag::U64, &n.to_be_bytes()),
        Value::U128(n) => tagged(out, tag::U128, &n.to_be_bytes()),
        Value::U256(n) => tagged(out, tag::U256, &n.to_be_bytes::<32>()),
        Value::I8(n) => tagged(out, tag::I8, &n.to_be_bytes()),
        Value::I16(n) => tagged(out, tag::I16, &n.to_be_bytes()),
        Value::I32(n) => tagged(out, tag::I32, &n.to_be_bytes()),
        Value::I64(n) => tagged(out, tag::I64, &n.to_be_bytes()),
        Value::I128(n) => tagged(out, tag::I128, &n.to_be_bytes()),
        Value::I256(n) => tagged(out, tag::I256, &n.to_bits().to_be_bytes::<32>()),
        Value::Address(a) => tagged(out, tag::ADDRESS, a.as_bytes()),
        Value::String(s) => {
            out.push(tag::STRING);
            bytes(out, s.as_bytes());
        }
        Value::Bytes(b) => {
            out.push(tag::BYTES);
            bytes(out, b);
        }
        Value::Vector(items) => {
            out.push(tag::VECTOR);
            length(out, items.len());
            for item in items {
                encode_value(out, item);
            }
        }
        Value::Option(None) => out.push(tag::NONE),
        Value::Option(Some(inner)) => {
            out.push(tag::SOME);
            encode_value(out, inner);
        }
        Value::Struct(fields) => {
            out.push(tag::STRUCT);
            encode_fields(out, fields);
        }
        Value::Variant { name, fields } => {
            out.push(tag::VARIANT);
            bytes(out, name.as_str().as_bytes());
            encode_fields(out, fields);
        }
    }
}

fn encode_fields(out: &mut Vec<u8>, fields: &[(Identifier, Value)]) {
    length(out, fields.len());
    for (name, value) in fields {
        bytes(out, name.as_str().as_bytes());
        encode_value(out, value);
    }
}

fn tagged(out: &mut Vec<u8>, tag: u8, payload: &[u8]) {
    out.push(tag);
    out.extend_from_slice(payload);
}

fn bytes(out: &mut Vec<u8>, b: &[u8]) {
    length(out, b.len());
    out.extend_from_slice(b);
}

/// Unsigned LEB128.
fn length(out: &mut Vec<u8>, n: usize) {
    // usize is at most 64 bits on every target Rust supports.
    let mut n = u64::try_from(n).unwrap_or(u64::MAX);
    loop {
        let low = (n & 0x7f).to_le_bytes()[0];
        n >>= 7;
        if n == 0 {
            out.push(low);
            return;
        }
        out.push(low | 0x80);
    }
}

struct Reader<'a> {
    bytes: &'a [u8],
    depth: usize,
}

impl Reader<'_> {
    fn take(&mut self, n: usize) -> Result<&[u8], CodecError> {
        if self.bytes.len() < n {
            return Err(CodecError("truncated"));
        }
        let (head, rest) = self.bytes.split_at(n);
        self.bytes = rest;
        Ok(head)
    }

    fn array<const N: usize>(&mut self) -> Result<[u8; N], CodecError> {
        let mut out = [0; N];
        out.copy_from_slice(self.take(N)?);
        Ok(out)
    }

    fn byte(&mut self) -> Result<u8, CodecError> {
        Ok(self.array::<1>()?[0])
    }

    fn length(&mut self) -> Result<usize, CodecError> {
        let mut n: u64 = 0;
        for shift in (0..64).step_by(7) {
            let b = self.byte()?;
            let bits = u64::from(b & 0x7f);
            if shift == 63 && bits > 1 {
                return Err(CodecError("length overflows"));
            }
            n |= bits << shift;
            if b & 0x80 == 0 {
                if b == 0 && shift > 0 {
                    return Err(CodecError("length isn't minimally encoded"));
                }
                // A length longer than the remaining input is caught when it's read.
                return usize::try_from(n).map_err(|_| CodecError("length overflows"));
            }
        }
        Err(CodecError("length overflows"))
    }

    fn bytes(&mut self) -> Result<Vec<u8>, CodecError> {
        let n = self.length()?;
        Ok(self.take(n)?.to_vec())
    }

    fn string(&mut self) -> Result<String, CodecError> {
        String::from_utf8(self.bytes()?).map_err(|_| CodecError("string isn't UTF-8"))
    }

    fn identifier(&mut self) -> Result<Identifier, CodecError> {
        self.string()?
            .parse()
            .map_err(|_| CodecError("invalid identifier"))
    }

    fn value(&mut self) -> Result<Value, CodecError> {
        self.depth += 1;
        if self.depth > MAX_DEPTH {
            return Err(CodecError("value nests too deeply"));
        }
        let value = self.value_inner();
        self.depth -= 1;
        value
    }

    fn value_inner(&mut self) -> Result<Value, CodecError> {
        Ok(match self.byte()? {
            tag::FALSE => Value::Bool(false),
            tag::TRUE => Value::Bool(true),
            tag::U8 => Value::U8(u8::from_be_bytes(self.array()?)),
            tag::U16 => Value::U16(u16::from_be_bytes(self.array()?)),
            tag::U32 => Value::U32(u32::from_be_bytes(self.array()?)),
            tag::U64 => Value::U64(u64::from_be_bytes(self.array()?)),
            tag::U128 => Value::U128(u128::from_be_bytes(self.array()?)),
            tag::U256 => Value::U256(U256::from_be_bytes::<32>(self.array()?)),
            tag::I8 => Value::I8(i8::from_be_bytes(self.array()?)),
            tag::I16 => Value::I16(i16::from_be_bytes(self.array()?)),
            tag::I32 => Value::I32(i32::from_be_bytes(self.array()?)),
            tag::I64 => Value::I64(i64::from_be_bytes(self.array()?)),
            tag::I128 => Value::I128(i128::from_be_bytes(self.array()?)),
            tag::I256 => Value::I256(I256::from_bits(U256::from_be_bytes::<32>(self.array()?))),
            tag::ADDRESS => Value::Address(Address::new(self.array()?)),
            tag::STRING => Value::String(self.string()?),
            tag::BYTES => Value::Bytes(self.bytes()?),
            tag::VECTOR => {
                let n = self.length()?;
                let mut items = Vec::with_capacity(n.min(self.bytes.len()));
                for _ in 0..n {
                    items.push(self.value()?);
                }
                Value::Vector(items)
            }
            tag::NONE => Value::Option(None),
            tag::SOME => Value::Option(Some(Box::new(self.value()?))),
            tag::STRUCT => Value::Struct(self.fields()?),
            tag::VARIANT => Value::Variant {
                name: self.identifier()?,
                fields: self.fields()?,
            },
            _ => return Err(CodecError("unknown value tag")),
        })
    }

    fn fields(&mut self) -> Result<Vec<(Identifier, Value)>, CodecError> {
        let n = self.length()?;
        let mut fields = Vec::with_capacity(n.min(self.bytes.len()));
        for _ in 0..n {
            fields.push((self.identifier()?, self.value()?));
        }
        Ok(fields)
    }
}

#[cfg(test)]
mod tests {
    use proptest::prelude::*;

    use super::*;

    fn ident() -> impl Strategy<Value = Identifier> {
        "[a-zA-Z][a-zA-Z0-9_]{0,8}".prop_map(|s| s.parse().unwrap())
    }

    pub(crate) fn value() -> impl Strategy<Value = Value> {
        let leaf = prop_oneof![
            any::<bool>().prop_map(Value::Bool),
            any::<u8>().prop_map(Value::U8),
            any::<u16>().prop_map(Value::U16),
            any::<u32>().prop_map(Value::U32),
            any::<u64>().prop_map(Value::U64),
            any::<u128>().prop_map(Value::U128),
            any::<[u8; 32]>().prop_map(|b| Value::U256(U256::from_be_bytes(b))),
            any::<i8>().prop_map(Value::I8),
            any::<i16>().prop_map(Value::I16),
            any::<i32>().prop_map(Value::I32),
            any::<i64>().prop_map(Value::I64),
            any::<i128>().prop_map(Value::I128),
            any::<[u8; 32]>().prop_map(|b| Value::I256(I256::from_bits(U256::from_be_bytes(b)))),
            any::<[u8; 32]>().prop_map(|b| Value::Address(Address::new(b))),
            ".*".prop_map(Value::String),
            proptest::collection::vec(any::<u8>(), 0..40).prop_map(Value::Bytes),
        ];
        leaf.prop_recursive(4, 32, 6, |inner| {
            prop_oneof![
                proptest::collection::vec(inner.clone(), 0..6).prop_map(Value::Vector),
                proptest::option::of(inner.clone()).prop_map(|v| Value::Option(v.map(Box::new))),
                proptest::collection::vec((ident(), inner.clone()), 0..4).prop_map(Value::Struct),
                (ident(), proptest::collection::vec((ident(), inner), 0..4))
                    .prop_map(|(name, fields)| Value::Variant { name, fields }),
            ]
        })
    }

    proptest! {
        #[test]
        fn round_trips(values in proptest::collection::vec(value(), 0..5)) {
            prop_assert_eq!(decode(&encode(&values)).unwrap(), values);
        }

        #[test]
        fn is_one_to_one(a in proptest::collection::vec(value(), 0..3), b in proptest::collection::vec(value(), 0..3)) {
            prop_assert_eq!(a == b, encode(&a) == encode(&b));
        }

        #[test]
        fn never_panics_on_garbage(bytes in proptest::collection::vec(any::<u8>(), 0..64)) {
            let _ = decode(&bytes);
        }
    }

    #[test]
    fn lengths_are_leb128() {
        let mut out = Vec::new();
        length(&mut out, 300);
        assert_eq!(out, [0xac, 0x02]);
        let mut reader = Reader {
            bytes: &out,
            depth: 0,
        };
        assert_eq!(reader.length().unwrap(), 300);
        // Overlong and truncated forms are refused.
        for bad in [&[0x80, 0x00][..], &[0x80][..]] {
            let mut reader = Reader {
                bytes: bad,
                depth: 0,
            };
            assert!(reader.length().is_err(), "{bad:?}");
        }
    }

    #[test]
    fn refuses_trailing_bytes_and_unknown_tags() {
        let mut bytes = encode(&[Value::U8(1)]);
        bytes.push(0);
        assert_eq!(decode(&bytes), Err(CodecError("trailing bytes")));
        assert_eq!(decode(&[1, 99]), Err(CodecError("unknown value tag")));
    }

    #[test]
    fn nul_survives() {
        let values = vec![Value::String("a\0b".into())];
        assert_eq!(decode(&encode(&values)).unwrap(), values);
    }
}
