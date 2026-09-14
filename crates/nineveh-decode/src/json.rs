//! The type-directed decoder: the stream's JSON plus a pinned layout gives a [`Value`].

use std::borrow::Cow;
use std::fmt::{self, Write as _};

use nineveh_core::{Address, I256, Identifier, StructName, StructTag, TypeTag, Value, parse_u256};
use serde_json::Value as Json;

use crate::layout::{Body, Builtin, Field};
use crate::lock::Lockfile;

/// Decodes the JSON the fullnode renders for Move values, guided by pinned layouts.
///
/// Decoding is strict (ADR 0002). A value that doesn't match its layout exactly is an
/// error that names the JSON path, the expected type and the offending JSON; nothing is
/// coerced and no field is skipped. Rendering conventions are catalogued, with
/// fixtures, in `docs/research/stream-json-conventions.md`.
#[derive(Debug, Clone, Copy)]
pub struct Decoder<'l> {
    lock: &'l Lockfile,
}

/// JSON nesting deeper than this is rejected rather than risking the stack.
const MAX_DEPTH: usize = 128;

impl<'l> Decoder<'l> {
    #[must_use]
    pub fn new(lock: &'l Lockfile) -> Self {
        Self { lock }
    }

    #[must_use]
    pub fn lock(&self) -> &'l Lockfile {
        self.lock
    }

    /// Parse `text` as JSON and decode it as a `ty`.
    ///
    /// # Errors
    ///
    /// If `text` isn't JSON or doesn't match `ty`.
    pub fn decode_str(&self, ty: &TypeTag, text: &str) -> Result<Value, ValueError> {
        let json: Json = serde_json::from_str(text).map_err(|e| ValueError {
            path: String::new(),
            expected: ty.to_string(),
            found: excerpt_str(text),
            reason: Reason::NotJson(e.to_string()),
        })?;
        self.decode(ty, &json)
    }

    /// Decode already-parsed JSON as a `ty`.
    ///
    /// # Errors
    ///
    /// If `json` doesn't match `ty`.
    pub fn decode(&self, ty: &TypeTag, json: &Json) -> Result<Value, ValueError> {
        let mut path = Vec::new();
        self.value(ty, json, &mut path)
            .map_err(|failure| failure.into_error(&path))
    }

    fn value(self, ty: &TypeTag, json: &Json, path: &mut Path) -> Result<Value, Failure> {
        if path.len() > MAX_DEPTH {
            return Err(Failure::new(ty, json, Reason::TooDeep));
        }
        let fail = |reason| Failure::new(ty, json, reason);
        Ok(match ty {
            TypeTag::Bool => Value::Bool(
                json.as_bool()
                    .ok_or_else(|| fail(Reason::Expected("a boolean")))?,
            ),
            TypeTag::U8 => Value::U8(small_int(json).ok_or_else(|| fail(Reason::SmallInt))?),
            TypeTag::U16 => Value::U16(small_int(json).ok_or_else(|| fail(Reason::SmallInt))?),
            TypeTag::U32 => Value::U32(small_int(json).ok_or_else(|| fail(Reason::SmallInt))?),
            TypeTag::I8 => Value::I8(small_int(json).ok_or_else(|| fail(Reason::SmallInt))?),
            TypeTag::I16 => Value::I16(small_int(json).ok_or_else(|| fail(Reason::SmallInt))?),
            TypeTag::I32 => Value::I32(small_int(json).ok_or_else(|| fail(Reason::SmallInt))?),
            TypeTag::U64 => Value::U64(wide_uint(json).ok_or_else(|| fail(Reason::WideInt))?),
            TypeTag::U128 => Value::U128(wide_uint(json).ok_or_else(|| fail(Reason::WideInt))?),
            TypeTag::U256 => Value::U256(
                json.as_str()
                    .and_then(|s| parse_u256(s).ok())
                    .ok_or_else(|| fail(Reason::WideInt))?,
            ),
            TypeTag::I64 => Value::I64(wide_int(json).ok_or_else(|| fail(Reason::WideInt))?),
            TypeTag::I128 => Value::I128(wide_int(json).ok_or_else(|| fail(Reason::WideInt))?),
            TypeTag::I256 => Value::I256(
                json.as_str()
                    .and_then(|s| s.parse::<I256>().ok())
                    .ok_or_else(|| fail(Reason::WideInt))?,
            ),
            TypeTag::Address => Value::Address(address(json).ok_or_else(|| fail(Reason::Address))?),
            TypeTag::Signer => return Err(fail(Reason::Signer)),
            TypeTag::Vector(inner) if **inner == TypeTag::U8 => Value::Bytes(
                json.as_str()
                    .and_then(hex_bytes)
                    .ok_or_else(|| fail(Reason::Hex))?,
            ),
            TypeTag::Vector(inner) => {
                let items = json
                    .as_array()
                    .ok_or_else(|| fail(Reason::Expected("an array")))?;
                let mut values = Vec::with_capacity(items.len());
                for (i, item) in items.iter().enumerate() {
                    path.push(Segment::Index(i));
                    values.push(self.value(inner, item, path)?);
                    path.pop();
                }
                Value::Vector(values)
            }
            TypeTag::Struct(tag) => self.structure(tag, ty, json, path)?,
            TypeTag::Param(_) => return Err(fail(Reason::Unresolved)),
        })
    }

    fn structure(
        self,
        tag: &StructTag,
        ty: &TypeTag,
        json: &Json,
        path: &mut Path,
    ) -> Result<Value, Failure> {
        let fail = |reason| Failure::new(ty, json, reason);
        match Builtin::of(&tag.name) {
            Some(Builtin::String) => {
                let s = json
                    .as_str()
                    .ok_or_else(|| fail(Reason::Expected("a string")))?;
                return Ok(Value::String(s.to_owned()));
            }
            Some(Builtin::Option) => {
                let [inner] = tag.type_args.as_slice() else {
                    return Err(fail(Reason::Arity { declared: 1 }));
                };
                let items = json
                    .as_object()
                    .filter(|o| o.len() == 1)
                    .and_then(|o| o.get("vec"))
                    .and_then(Json::as_array)
                    .ok_or_else(|| fail(Reason::Expected(r#"{"vec": [...]}"#)))?;
                return match items.as_slice() {
                    [] => Ok(Value::Option(None)),
                    [item] => {
                        path.push(Segment::Field("vec".into()));
                        path.push(Segment::Index(0));
                        let value = self.value(inner, item, path)?;
                        path.truncate(path.len() - 2);
                        Ok(Value::Option(Some(Box::new(value))))
                    }
                    _ => Err(fail(Reason::Expected("an option with at most one element"))),
                };
            }
            None => {}
        }

        let layout = self
            .lock
            .get(&tag.name)
            .ok_or_else(|| fail(Reason::NoLayout(Box::new(tag.name.clone()))))?;
        if usize::from(layout.type_params) != tag.type_args.len() {
            return Err(fail(Reason::Arity {
                declared: layout.type_params,
            }));
        }
        let object = json
            .as_object()
            .ok_or_else(|| fail(Reason::Expected("an object")))?;

        match &layout.body {
            Body::Struct(fields) => {
                let fields = self.fields(fields, &tag.type_args, object, None, ty, json, path)?;
                Ok(Value::Struct(fields))
            }
            Body::Enum(variants) => {
                let name = object
                    .get(VARIANT_KEY)
                    .and_then(Json::as_str)
                    .ok_or_else(|| {
                        fail(Reason::Expected(r#"an enum object with "__variant__""#))
                    })?;
                let variant = variants
                    .iter()
                    .find(|v| v.name == *name)
                    .ok_or_else(|| fail(Reason::UnknownVariant(name.to_owned())))?;
                let fields = self.fields(
                    &variant.fields,
                    &tag.type_args,
                    object,
                    Some(VARIANT_KEY),
                    ty,
                    json,
                    path,
                )?;
                Ok(Value::Variant {
                    name: variant.name.clone(),
                    fields,
                })
            }
        }
    }

    /// Decode exactly the declared fields: a missing or unexpected key is an error.
    #[allow(clippy::too_many_arguments, reason = "internal recursion helper")]
    fn fields(
        self,
        declared: &[Field],
        type_args: &[TypeTag],
        object: &serde_json::Map<String, Json>,
        extra_key: Option<&str>,
        ty: &TypeTag,
        json: &Json,
        path: &mut Path,
    ) -> Result<Vec<(Identifier, Value)>, Failure> {
        let expected_len = declared.len() + usize::from(extra_key.is_some());
        if object.len() != expected_len {
            let unknown = object.keys().find(|k| {
                Some(k.as_str()) != extra_key && !declared.iter().any(|f| f.name == *k.as_str())
            });
            if let Some(key) = unknown {
                return Err(Failure::new(ty, json, Reason::UnknownField(key.clone())));
            }
        }

        let mut values = Vec::with_capacity(declared.len());
        for field in declared {
            let item = object
                .get(field.name.as_str())
                .ok_or_else(|| Failure::new(ty, json, Reason::MissingField(field.name.clone())))?;
            let field_ty = if field.ty.is_concrete() {
                Cow::Borrowed(&field.ty)
            } else {
                Cow::Owned(
                    field
                        .ty
                        .substitute(type_args)
                        .map_err(|_| Failure::new(ty, json, Reason::Unresolved))?,
                )
            };
            path.push(Segment::Field(field.name.as_str().into()));
            let value = self.value(&field_ty, item, path)?;
            path.pop();
            values.push((field.name.clone(), value));
        }
        Ok(values)
    }
}

/// The key the fullnode adds to a Move 2 enum value to name its variant.
const VARIANT_KEY: &str = "__variant__";

/// `u8`, `u16` and `u32` (and, by the same convention, `i8`–`i32`) are JSON numbers.
fn small_int<T: TryFrom<i64> + TryFrom<u64>>(json: &Json) -> Option<T> {
    let n = json.as_number()?;
    if let Some(u) = n.as_u64() {
        T::try_from(u).ok()
    } else {
        T::try_from(n.as_i64()?).ok()
    }
}

/// `u64` and `u128` are decimal strings: digits only.
fn wide_uint<T: std::str::FromStr>(json: &Json) -> Option<T> {
    let s = json.as_str()?;
    if s.is_empty() || !s.bytes().all(|b| b.is_ascii_digit()) {
        return None;
    }
    s.parse().ok()
}

/// `i64` and `i128` are decimal strings with an optional leading `-`.
fn wide_int<T: std::str::FromStr>(json: &Json) -> Option<T> {
    let s = json.as_str()?;
    let digits = s.strip_prefix('-').unwrap_or(s);
    if digits.is_empty() || !digits.bytes().all(|b| b.is_ascii_digit()) {
        return None;
    }
    s.parse().ok()
}

fn address(json: &Json) -> Option<Address> {
    json.as_str()?.parse().ok()
}

/// `vector<u8>` is `0x` followed by an even number of hex digits.
pub(crate) fn hex_bytes(s: &str) -> Option<Vec<u8>> {
    let digits = s.strip_prefix("0x")?.as_bytes();
    if digits.len() % 2 != 0 {
        return None;
    }
    digits
        .as_chunks::<2>()
        .0
        .iter()
        .map(|&[high, low]| Some(nibble(high)? << 4 | nibble(low)?))
        .collect()
}

const fn nibble(c: u8) -> Option<u8> {
    match c {
        b'0'..=b'9' => Some(c - b'0'),
        b'a'..=b'f' => Some(c - b'a' + 10),
        b'A'..=b'F' => Some(c - b'A' + 10),
        _ => None,
    }
}

// --- errors ----------------------------------------------------------------------

type Path = Vec<Segment>;

#[derive(Debug, Clone)]
enum Segment {
    Field(Box<str>),
    Index(usize),
}

/// A value that doesn't match its layout, located by JSON path.
#[derive(Debug, Clone, PartialEq, Eq, thiserror::Error)]
#[error("{}: expected {expected}, found {found}: {reason}", display_path(path))]
pub struct ValueError {
    /// Where in the value, like `.positions[3].size`; empty at the root.
    pub path: String,
    /// The Move type expected there.
    pub expected: String,
    /// The JSON found there, truncated.
    pub found: String,
    pub reason: Reason,
}

fn display_path(path: &str) -> &str {
    if path.is_empty() { "at the root" } else { path }
}

/// Why a value doesn't match its type.
#[derive(Debug, Clone, PartialEq, Eq)]
#[non_exhaustive]
pub enum Reason {
    NotJson(String),
    Expected(&'static str),
    SmallInt,
    WideInt,
    Address,
    Hex,
    Signer,
    MissingField(Identifier),
    UnknownField(String),
    UnknownVariant(String),
    NoLayout(Box<StructName>),
    Arity { declared: u16 },
    Unresolved,
    TooDeep,
}

impl fmt::Display for Reason {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::NotJson(e) => write!(f, "not valid JSON ({e})"),
            Self::Expected(what) => write!(f, "expected {what}"),
            Self::SmallInt => f.write_str("expected a JSON number in range"),
            Self::WideInt => f.write_str("expected a decimal string in range"),
            Self::Address => f.write_str("expected a `0x` address of 1 to 64 hex digits"),
            Self::Hex => f.write_str("expected `0x` followed by an even number of hex digits"),
            Self::Signer => f.write_str("a signer can't be stored, so it never appears in data"),
            Self::MissingField(name) => write!(f, "field `{name}` is missing"),
            Self::UnknownField(name) => write!(f, "field `{name}` isn't in the pinned layout"),
            Self::UnknownVariant(name) => {
                write!(f, "variant `{name}` isn't in the pinned layout")
            }
            Self::NoLayout(name) => write!(f, "`{name}` has no layout in nineveh.lock"),
            Self::Arity { declared } => write!(
                f,
                "the type's argument count doesn't match the {declared} it declares"
            ),
            Self::Unresolved => f.write_str("the layout refers to an undeclared type parameter"),
            Self::TooDeep => write!(f, "nested more than {MAX_DEPTH} levels deep"),
        }
    }
}

/// A failure deep in the recursion; the path is attached once, on the way out.
struct Failure {
    expected: String,
    found: String,
    reason: Reason,
}

impl Failure {
    fn new(ty: &TypeTag, json: &Json, reason: Reason) -> Self {
        Self {
            expected: ty.to_string(),
            found: excerpt_str(&json.to_string()),
            reason,
        }
    }

    fn into_error(self, path: &Path) -> ValueError {
        let mut rendered = String::new();
        for segment in path {
            match segment {
                Segment::Field(name) => {
                    rendered.push('.');
                    rendered.push_str(name);
                }
                Segment::Index(i) => {
                    let _ = write!(rendered, "[{i}]");
                }
            }
        }
        ValueError {
            path: rendered,
            expected: self.expected,
            found: self.found,
            reason: self.reason,
        }
    }
}

fn excerpt_str(text: &str) -> String {
    const MAX: usize = 80;
    if text.len() <= MAX {
        return text.to_owned();
    }
    let mut end = MAX;
    while !text.is_char_boundary(end) {
        end -= 1;
    }
    format!("{}…", &text[..end])
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::lock::Lockfile;
    use serde_json::json;

    fn lock() -> Lockfile {
        Lockfile::from_json(
            r#"{
              "format": 1, "network": "testnet",
              "structs": {
                "0x1::object::Object": {"type_params": 1, "fields": [{"name": "inner", "type": "address"}]},
                "0xabc::m::Pair": {"type_params": 2, "fields": [
                    {"name": "a", "type": "T0"}, {"name": "b", "type": "vector<T1>"}]},
                "0xabc::m::Shape": {"variants": [
                    {"name": "Circle", "fields": [{"name": "r", "type": "u64"}]},
                    {"name": "Empty", "fields": []}]}
              }
            }"#,
        )
        .unwrap()
    }

    #[allow(clippy::needless_pass_by_value, reason = "tests pass json! literals")]
    fn decode(ty: &str, json: Json) -> Result<Value, ValueError> {
        Decoder::new(&lock()).decode(&ty.parse().unwrap(), &json)
    }

    fn rendered(ty: &str, json: Json) -> Json {
        serde_json::to_value(decode(ty, json).unwrap()).unwrap()
    }

    #[test]
    fn integers_follow_the_width_convention() {
        assert_eq!(decode("u16", json!(102)).unwrap(), Value::U16(102));
        assert_eq!(
            decode("u64", json!("18441553330519219599")).unwrap(),
            Value::U64(18_441_553_330_519_219_599)
        );
        assert_eq!(
            decode("i128", json!("-6294071852848")).unwrap(),
            Value::I128(-6_294_071_852_848)
        );
        assert_eq!(decode("i8", json!(-5)).unwrap(), Value::I8(-5));

        for (ty, bad) in [
            ("u8", json!(256)),
            ("u8", json!("1")),
            ("u64", json!(1)),
            ("u64", json!("-1")),
            ("u64", json!("+1")),
            ("u64", json!("18446744073709551616")),
            ("u128", json!("1.5")),
            ("i64", json!("-")),
            ("u32", json!(1.5)),
        ] {
            assert!(decode(ty, bad.clone()).is_err(), "{ty} accepted {bad}");
        }
    }

    #[test]
    fn short_addresses_and_short_hex_are_told_apart_by_type() {
        // "0xf600" is a vector<u8> in testnet-11196227807.pb; as an address it's 0xf600.
        assert_eq!(
            decode("vector<u8>", json!("0xf600")).unwrap(),
            Value::Bytes(vec![0xf6, 0x00])
        );
        assert_eq!(
            decode("address", json!("0xf600")).unwrap(),
            Value::Address("0xf600".parse().unwrap())
        );
        assert!(decode("vector<u8>", json!("0xf60")).is_err());
    }

    #[test]
    fn builtins_generics_and_enums() {
        assert_eq!(
            rendered(
                "0xabc::m::Pair<0x1::option::Option<0x1::string::String>, 0x1::object::Object<u8>>",
                json!({"a": {"vec": ["hi"]}, "b": [{"inner": "0xa"}]})
            ),
            json!({"a": "hi", "b": [{"inner": "0x000000000000000000000000000000000000000000000000000000000000000a"}]})
        );
        assert_eq!(
            decode("0x1::option::Option<u8>", json!({"vec": []})).unwrap(),
            Value::Option(None)
        );
        assert_eq!(
            rendered(
                "0xabc::m::Shape",
                json!({"__variant__": "Circle", "r": "7"})
            ),
            json!({"__variant__": "Circle", "r": "7"})
        );
        assert_eq!(
            rendered("0xabc::m::Shape", json!({"__variant__": "Empty"})),
            json!({"__variant__": "Empty"})
        );
    }

    #[test]
    fn mismatches_are_located() {
        let err = decode(
            "0xabc::m::Pair<u8, 0xabc::m::Shape>",
            json!({"a": 1, "b": [{"__variant__": "Circle", "r": "7"}, {"__variant__": "Circle", "r": 7}]}),
        )
        .unwrap_err();
        assert_eq!(err.path, ".b[1].r");
        assert_eq!(err.expected, "u64");
        assert_eq!(err.found, "7");
        assert_eq!(
            err.to_string(),
            ".b[1].r: expected u64, found 7: expected a decimal string in range"
        );
    }

    #[test]
    fn struct_fields_must_match_exactly() {
        let missing = decode("0x1::object::Object<u8>", json!({})).unwrap_err();
        assert_eq!(
            missing.reason,
            Reason::MissingField("inner".parse().unwrap())
        );

        let extra = decode(
            "0x1::object::Object<u8>",
            json!({"inner": "0x1", "outer": 1}),
        )
        .unwrap_err();
        assert_eq!(extra.reason, Reason::UnknownField("outer".into()));

        let variant = decode("0xabc::m::Shape", json!({"__variant__": "Square"})).unwrap_err();
        assert_eq!(variant.reason, Reason::UnknownVariant("Square".into()));

        let arity = decode("0x1::object::Object", json!({"inner": "0x1"})).unwrap_err();
        assert_eq!(arity.reason, Reason::Arity { declared: 1 });

        let unknown = decode("0xabc::m::Nope", json!({})).unwrap_err();
        assert!(matches!(unknown.reason, Reason::NoLayout(_)));
    }

    #[test]
    fn hostile_nesting_is_rejected() {
        // The type parser caps nesting itself, so build the tag directly.
        let mut tag = TypeTag::U8;
        let mut json = json!(1);
        for _ in 0..200 {
            tag = TypeTag::Vector(Box::new(tag));
            json = json!([json]);
        }
        let err = Decoder::new(&lock()).decode(&tag, &json).unwrap_err();
        assert_eq!(err.reason, Reason::TooDeep);
    }
}
