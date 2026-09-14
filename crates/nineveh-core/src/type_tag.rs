use std::fmt;
use std::str::FromStr;

use crate::{Address, Identifier};

/// A Move type, as written in the Transaction Stream's `type_str`, a table item's
/// `key_type`/`value_type`, or a module ABI.
///
/// Types from the stream are always concrete. ABI field types can also refer to the
/// declaring struct's generic parameters, which appear as [`TypeTag::Param`].
/// [`FromStr`] parses concrete types only; [`TypeTag::parse_declared`] also accepts
/// parameters.
#[derive(Debug, Clone, PartialEq, Eq, PartialOrd, Ord, Hash)]
pub enum TypeTag {
    Bool,
    U8,
    U16,
    U32,
    U64,
    U128,
    U256,
    I8,
    I16,
    I32,
    I64,
    I128,
    I256,
    Address,
    Signer,
    Vector(Box<TypeTag>),
    Struct(Box<StructTag>),
    /// The declaring struct's generic parameter `T<index>`.
    Param(u16),
}

impl TypeTag {
    /// Parse a type that may refer to generic parameters (`T0`, `T1`, …), as ABI field
    /// types do.
    ///
    /// # Errors
    ///
    /// If `s` isn't a well-formed Move type.
    pub fn parse_declared(s: &str) -> Result<Self, InvalidTypeTag> {
        Parser::new(s, true).parse_all()
    }

    /// Whether this type mentions no generic parameters.
    #[must_use]
    pub fn is_concrete(&self) -> bool {
        match self {
            Self::Param(_) => false,
            Self::Vector(inner) => inner.is_concrete(),
            Self::Struct(tag) => tag.type_args.iter().all(Self::is_concrete),
            _ => true,
        }
    }

    /// Replace each `T<i>` with `args[i]`.
    ///
    /// # Errors
    ///
    /// If the type refers to a parameter past the end of `args`.
    pub fn substitute(&self, args: &[TypeTag]) -> Result<Self, ParamOutOfRange> {
        Ok(match self {
            Self::Param(index) => {
                args.get(usize::from(*index))
                    .cloned()
                    .ok_or(ParamOutOfRange {
                        index: *index,
                        arity: args.len(),
                    })?
            }
            Self::Vector(inner) => Self::Vector(Box::new(inner.substitute(args)?)),
            Self::Struct(tag) => Self::Struct(Box::new(StructTag {
                name: tag.name.clone(),
                type_args: tag
                    .type_args
                    .iter()
                    .map(|t| t.substitute(args))
                    .collect::<Result<_, _>>()?,
            })),
            other => other.clone(),
        })
    }

    /// The struct tag, if this is a struct type.
    #[must_use]
    pub fn as_struct(&self) -> Option<&StructTag> {
        match self {
            Self::Struct(tag) => Some(tag),
            _ => None,
        }
    }

    fn primitive(name: &str) -> Option<Self> {
        Some(match name {
            "bool" => Self::Bool,
            "u8" => Self::U8,
            "u16" => Self::U16,
            "u32" => Self::U32,
            "u64" => Self::U64,
            "u128" => Self::U128,
            "u256" => Self::U256,
            "i8" => Self::I8,
            "i16" => Self::I16,
            "i32" => Self::I32,
            "i64" => Self::I64,
            "i128" => Self::I128,
            "i256" => Self::I256,
            "address" => Self::Address,
            "signer" => Self::Signer,
            _ => return None,
        })
    }
}

impl fmt::Display for TypeTag {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        let name = match self {
            Self::Bool => "bool",
            Self::U8 => "u8",
            Self::U16 => "u16",
            Self::U32 => "u32",
            Self::U64 => "u64",
            Self::U128 => "u128",
            Self::U256 => "u256",
            Self::I8 => "i8",
            Self::I16 => "i16",
            Self::I32 => "i32",
            Self::I64 => "i64",
            Self::I128 => "i128",
            Self::I256 => "i256",
            Self::Address => "address",
            Self::Signer => "signer",
            Self::Vector(inner) => return write!(f, "vector<{inner}>"),
            Self::Struct(tag) => return tag.fmt(f),
            Self::Param(index) => return write!(f, "T{index}"),
        };
        f.write_str(name)
    }
}

impl FromStr for TypeTag {
    type Err = InvalidTypeTag;

    fn from_str(s: &str) -> Result<Self, Self::Err> {
        Parser::new(s, false).parse_all()
    }
}

/// A struct's fully qualified name, without type arguments: `0x1::coin::CoinStore`.
#[derive(Debug, Clone, PartialEq, Eq, PartialOrd, Ord, Hash)]
pub struct StructName {
    pub address: Address,
    pub module: Identifier,
    pub name: Identifier,
}

impl StructName {
    #[must_use]
    pub fn new(address: Address, module: Identifier, name: Identifier) -> Self {
        Self {
            address,
            module,
            name,
        }
    }

    /// Whether this is `address::module::name`.
    #[must_use]
    pub fn is(&self, address: Address, module: &str, name: &str) -> bool {
        self.address == address && self.module == *module && self.name == *name
    }
}

impl fmt::Display for StructName {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(
            f,
            "{}::{}::{}",
            self.address.to_standard_string(),
            self.module,
            self.name
        )
    }
}

impl FromStr for StructName {
    type Err = InvalidTypeTag;

    /// Parse `address::module::name`, with no type arguments.
    fn from_str(s: &str) -> Result<Self, Self::Err> {
        let tag: StructTag = s.parse()?;
        if tag.type_args.is_empty() && !s.contains('<') {
            Ok(tag.name)
        } else {
            Err(InvalidTypeTag {
                input: s.to_owned(),
                offset: s.find('<').unwrap_or(0),
                reason: "expected a struct name without type arguments",
            })
        }
    }
}

/// A struct type: its name plus concrete (or, in declarations, parameter) arguments.
#[derive(Debug, Clone, PartialEq, Eq, PartialOrd, Ord, Hash)]
pub struct StructTag {
    pub name: StructName,
    pub type_args: Vec<TypeTag>,
}

impl fmt::Display for StructTag {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        self.name.fmt(f)?;
        if let Some((first, rest)) = self.type_args.split_first() {
            write!(f, "<{first}")?;
            for arg in rest {
                write!(f, ", {arg}")?;
            }
            f.write_str(">")?;
        }
        Ok(())
    }
}

impl FromStr for StructTag {
    type Err = InvalidTypeTag;

    fn from_str(s: &str) -> Result<Self, Self::Err> {
        match s.parse()? {
            TypeTag::Struct(tag) => Ok(*tag),
            _ => Err(InvalidTypeTag {
                input: s.to_owned(),
                offset: 0,
                reason: "expected a struct type",
            }),
        }
    }
}

macro_rules! serde_via_display {
    ($($ty:ty),*) => {$(
        impl serde::Serialize for $ty {
            fn serialize<S: serde::Serializer>(&self, serializer: S) -> Result<S::Ok, S::Error> {
                serializer.collect_str(self)
            }
        }

        impl<'de> serde::Deserialize<'de> for $ty {
            fn deserialize<D: serde::Deserializer<'de>>(deserializer: D) -> Result<Self, D::Error> {
                let s = <std::borrow::Cow<'de, str>>::deserialize(deserializer)?;
                s.parse().map_err(serde::de::Error::custom)
            }
        }
    )*};
}

serde_via_display!(StructName, StructTag);

/// A type string that doesn't parse.
#[derive(Debug, Clone, PartialEq, Eq, thiserror::Error)]
#[error("invalid Move type `{input}` at byte {offset}: {reason}")]
pub struct InvalidTypeTag {
    pub input: String,
    pub offset: usize,
    pub reason: &'static str,
}

/// A type refers to a generic parameter its struct doesn't declare.
#[derive(Debug, Clone, Copy, PartialEq, Eq, thiserror::Error)]
#[error("type parameter T{index} is out of range: the struct declares {arity}")]
pub struct ParamOutOfRange {
    pub index: u16,
    pub arity: usize,
}

/// Recursive descent over the grammar:
///
/// ```text
/// type   := primitive | "vector" "<" type ">" | struct | param
/// struct := address "::" ident "::" ident [ "<" type { "," type } ">" ]
/// param  := "T" digits              (declarations only)
/// ```
///
/// Whitespace is allowed around punctuation; Aptos writes `, ` between arguments.
struct Parser<'a> {
    input: &'a str,
    pos: usize,
    allow_params: bool,
}

impl<'a> Parser<'a> {
    fn new(input: &'a str, allow_params: bool) -> Self {
        Self {
            input,
            pos: 0,
            allow_params,
        }
    }

    fn parse_all(mut self) -> Result<TypeTag, InvalidTypeTag> {
        let ty = self.parse_type(0)?;
        self.skip_ws();
        if self.pos == self.input.len() {
            Ok(ty)
        } else {
            Err(self.error("unexpected trailing input"))
        }
    }

    /// Nesting is bounded so a hostile type string can't overflow the stack.
    fn parse_type(&mut self, depth: usize) -> Result<TypeTag, InvalidTypeTag> {
        const MAX_DEPTH: usize = 64;
        if depth > MAX_DEPTH {
            return Err(self.error("type is nested too deeply"));
        }
        self.skip_ws();
        let start = self.pos;
        let word = self.take_while(|c| c.is_ascii_alphanumeric() || c == '_');
        if word.is_empty() {
            return Err(self.error("expected a type"));
        }

        if word.starts_with("0x") {
            self.pos = start;
            return self
                .parse_struct(depth)
                .map(|t| TypeTag::Struct(Box::new(t)));
        }
        if word == "vector" {
            self.expect('<')?;
            let inner = self.parse_type(depth + 1)?;
            self.expect('>')?;
            return Ok(TypeTag::Vector(Box::new(inner)));
        }
        if let Some(ty) = TypeTag::primitive(word) {
            return Ok(ty);
        }
        if let Some(index) = word.strip_prefix('T').and_then(|d| d.parse::<u16>().ok()) {
            if self.allow_params {
                return Ok(TypeTag::Param(index));
            }
            self.pos = start;
            return Err(self.error("generic parameters aren't allowed in a concrete type"));
        }
        self.pos = start;
        Err(self.error("unknown type; structs are written `address::module::Name`"))
    }

    fn parse_struct(&mut self, depth: usize) -> Result<StructTag, InvalidTypeTag> {
        let start = self.pos;
        let addr_text = self.take_while(|c| c.is_ascii_alphanumeric());
        let address: Address = addr_text.parse().map_err(|_| {
            self.pos = start;
            self.error("invalid address")
        })?;
        self.expect_str("::")?;
        let module = self.ident()?;
        self.expect_str("::")?;
        let name = self.ident()?;

        let mut type_args = Vec::new();
        self.skip_ws();
        if self.peek() == Some('<') {
            self.pos += 1;
            loop {
                type_args.push(self.parse_type(depth + 1)?);
                self.skip_ws();
                match self.peek() {
                    Some(',') => self.pos += 1,
                    Some('>') => {
                        self.pos += 1;
                        break;
                    }
                    _ => return Err(self.error("expected `,` or `>`")),
                }
            }
        }
        Ok(StructTag {
            name: StructName::new(address, module, name),
            type_args,
        })
    }

    fn ident(&mut self) -> Result<Identifier, InvalidTypeTag> {
        let start = self.pos;
        let word = self.take_while(|c| c.is_ascii_alphanumeric() || c == '_');
        word.parse().map_err(|_| {
            self.pos = start;
            self.error("expected an identifier")
        })
    }

    fn expect(&mut self, c: char) -> Result<(), InvalidTypeTag> {
        self.skip_ws();
        if self.peek() == Some(c) {
            self.pos += c.len_utf8();
            Ok(())
        } else {
            Err(self.error(match c {
                '<' => "expected `<`",
                '>' => "expected `>`",
                _ => "unexpected character",
            }))
        }
    }

    fn expect_str(&mut self, s: &str) -> Result<(), InvalidTypeTag> {
        if self.input[self.pos..].starts_with(s) {
            self.pos += s.len();
            Ok(())
        } else {
            Err(self.error("expected `::`"))
        }
    }

    fn take_while(&mut self, f: impl Fn(char) -> bool) -> &'a str {
        let rest = &self.input[self.pos..];
        let len = rest.find(|c| !f(c)).unwrap_or(rest.len());
        self.pos += len;
        &rest[..len]
    }

    fn skip_ws(&mut self) {
        self.take_while(char::is_whitespace);
    }

    fn peek(&self) -> Option<char> {
        self.input[self.pos..].chars().next()
    }

    fn error(&self, reason: &'static str) -> InvalidTypeTag {
        InvalidTypeTag {
            input: self.input.to_owned(),
            offset: self.pos,
            reason,
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    const PERP: &str = "0x50ead22afd6ffd9769e3b3d6e0e64a2a350d68e8b102c4e72e33d0b8cfdfdb06";

    #[test]
    fn round_trips_stream_type_strings() {
        // Verbatim from mainnet-7205731421.pb and testnet-6000000000.pb.
        for s in [
            "0x1::coin::CoinStore<0x1::aptos_coin::AptosCoin>",
            "u64",
            "vector<u8>",
            &format!(
                "0x1::storage_slots_allocator::Link<0x1::big_ordered_map::Node<address, \
                 0x5::bulk_order_types::BulkOrder<{PERP}::perp_engine_types::OrderMetadata>>>"
            ),
            &format!("{PERP}::perp_positions::TradeEvent"),
        ] {
            let ty: TypeTag = s.parse().unwrap();
            assert_eq!(ty.to_string(), s);
        }
    }

    #[test]
    fn normalizes_addresses_and_spacing() {
        let ty: TypeTag = "0x0001::coin::CoinStore< 0x1::aptos_coin::AptosCoin >"
            .parse()
            .unwrap();
        assert_eq!(
            ty.to_string(),
            "0x1::coin::CoinStore<0x1::aptos_coin::AptosCoin>"
        );
    }

    #[test]
    fn params_only_in_declarations() {
        let s = "0x1::object::Object<T0>";
        assert!(s.parse::<TypeTag>().is_err());
        let declared = TypeTag::parse_declared(s).unwrap();
        assert!(!declared.is_concrete());

        let concrete = declared
            .substitute(&["0x1::fungible_asset::Metadata".parse().unwrap()])
            .unwrap();
        assert_eq!(
            concrete.to_string(),
            "0x1::object::Object<0x1::fungible_asset::Metadata>"
        );
        assert!(concrete.is_concrete());
        assert_eq!(
            declared.substitute(&[]),
            Err(ParamOutOfRange { index: 0, arity: 0 })
        );
    }

    #[test]
    fn rejects_malformed_types() {
        for bad in [
            "",
            "u7",
            "vector<u8",
            "vector<>",
            "0x1::coin",
            "0x1::coin::",
            "0xZ::coin::Coin",
            "0x1::coin::Coin<u8,>",
            "0x1::coin::Coin<u8> extra",
            "Coin",
        ] {
            assert!(bad.parse::<TypeTag>().is_err(), "{bad:?} should not parse");
        }
    }

    #[test]
    fn deep_nesting_is_an_error_not_a_stack_overflow() {
        let s = format!("{}u8{}", "vector<".repeat(10_000), ">".repeat(10_000));
        assert!(s.parse::<TypeTag>().is_err());
    }

    #[test]
    fn struct_name_rejects_type_arguments() {
        assert!("0x1::coin::CoinStore".parse::<StructName>().is_ok());
        assert!(
            "0x1::coin::CoinStore<0x1::aptos_coin::AptosCoin>"
                .parse::<StructName>()
                .is_err()
        );
    }
}
