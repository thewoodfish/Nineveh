//! Expression types: Move's value types, with the framework wrappers that storage
//! treats specially (ADR 0008) flattened to what they hold.

use std::fmt;

use nineveh_core::{Address, StructTag, TypeTag};

/// The type of an expression.
#[derive(Debug, Clone, PartialEq, Eq, Hash)]
pub enum Type {
    Bool,
    Int(IntType),
    /// `address`, and `Object<T>`, which is read as its address.
    Address,
    /// `0x1::string::String`.
    String,
    /// `vector<u8>`.
    Bytes,
    Vector(Box<Type>),
    /// `0x1::option::Option<T>`, and nullable columns.
    Option(Box<Type>),
    /// Any other struct (or enum). Its fields are read with `.name`.
    Struct(StructTag),
    /// A `json` column. Anything can be stored in one; nothing can be read from one.
    Json,
}

/// An exact integer type.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, PartialOrd, Ord)]
pub enum IntType {
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
}

impl IntType {
    pub const ALL: [Self; 12] = [
        Self::U8,
        Self::U16,
        Self::U32,
        Self::U64,
        Self::U128,
        Self::U256,
        Self::I8,
        Self::I16,
        Self::I32,
        Self::I64,
        Self::I128,
        Self::I256,
    ];

    #[must_use]
    pub fn as_str(self) -> &'static str {
        match self {
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
        }
    }

    #[must_use]
    pub fn parse(s: &str) -> Option<Self> {
        Self::ALL.into_iter().find(|t| t.as_str() == s)
    }

    #[must_use]
    pub fn is_signed(self) -> bool {
        self >= Self::I8
    }

    /// Width in bits.
    #[must_use]
    pub fn bits(self) -> u32 {
        match self {
            Self::U8 | Self::I8 => 8,
            Self::U16 | Self::I16 => 16,
            Self::U32 | Self::I32 => 32,
            Self::U64 | Self::I64 => 64,
            Self::U128 | Self::I128 => 128,
            Self::U256 | Self::I256 => 256,
        }
    }
}

impl fmt::Display for IntType {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str(self.as_str())
    }
}

impl Type {
    /// The expression type of a Move type, or `None` for `signer` and unresolved
    /// generic parameters, which can't appear in data.
    #[must_use]
    pub fn from_move(ty: &TypeTag) -> Option<Self> {
        Some(match ty {
            TypeTag::Bool => Self::Bool,
            TypeTag::U8 => Self::Int(IntType::U8),
            TypeTag::U16 => Self::Int(IntType::U16),
            TypeTag::U32 => Self::Int(IntType::U32),
            TypeTag::U64 => Self::Int(IntType::U64),
            TypeTag::U128 => Self::Int(IntType::U128),
            TypeTag::U256 => Self::Int(IntType::U256),
            TypeTag::I8 => Self::Int(IntType::I8),
            TypeTag::I16 => Self::Int(IntType::I16),
            TypeTag::I32 => Self::Int(IntType::I32),
            TypeTag::I64 => Self::Int(IntType::I64),
            TypeTag::I128 => Self::Int(IntType::I128),
            TypeTag::I256 => Self::Int(IntType::I256),
            TypeTag::Address => Self::Address,
            TypeTag::Vector(inner) if **inner == TypeTag::U8 => Self::Bytes,
            TypeTag::Vector(inner) => Self::Vector(Box::new(Self::from_move(inner)?)),
            TypeTag::Struct(tag) => {
                let framework = |module: &str, name: &str| tag.name.is(Address::ONE, module, name);
                if framework("string", "String") {
                    Self::String
                } else if framework("object", "Object") {
                    Self::Address
                } else if framework("option", "Option") {
                    Self::Option(Box::new(Self::from_move(tag.type_args.first()?)?))
                } else {
                    Self::Struct((**tag).clone())
                }
            }
            TypeTag::Signer | TypeTag::Param(_) => return None,
        })
    }

    #[must_use]
    pub fn as_int(&self) -> Option<IntType> {
        match self {
            Self::Int(t) => Some(*t),
            _ => None,
        }
    }

    /// Whether values of this type can be compared with `==` and `!=`.
    #[must_use]
    pub fn has_equality(&self) -> bool {
        match self {
            Self::Json => false,
            Self::Vector(inner) | Self::Option(inner) => inner.has_equality(),
            _ => true,
        }
    }
}

impl fmt::Display for Type {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::Bool => f.write_str("bool"),
            Self::Int(t) => t.fmt(f),
            Self::Address => f.write_str("address"),
            Self::String => f.write_str("string"),
            Self::Bytes => f.write_str("bytes"),
            Self::Vector(inner) => write!(f, "vector<{inner}>"),
            Self::Option(inner) => write!(f, "option<{inner}>"),
            Self::Struct(tag) => tag.fmt(f),
            Self::Json => f.write_str("json"),
        }
    }
}

/// Whether `ty` mentions `Object<T>` anywhere, so its values need converting to the
/// flattened representation when read.
pub(crate) fn mentions_object(ty: &TypeTag) -> bool {
    match ty {
        TypeTag::Vector(inner) => mentions_object(inner),
        TypeTag::Struct(tag) => {
            tag.name.is(Address::ONE, "object", "Object")
                || tag.type_args.iter().any(mentions_object)
        }
        _ => false,
    }
}
