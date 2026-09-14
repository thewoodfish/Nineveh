//! Move struct layouts: the field types the stream's JSON carries no trace of.

use nineveh_core::{Address, Identifier, StructName, StructTag, TypeTag};

/// How to decode one struct, from its module's ABI.
///
/// Field types can refer to the struct's own generic parameters as
/// [`TypeTag::Param`]; they're substituted with the concrete type arguments of each
/// value being decoded.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct StructLayout {
    /// How many generic type parameters the struct declares.
    pub type_params: u16,
    pub body: Body,
    /// Declared `#[event]`.
    pub is_event: bool,
    /// The resource group this struct belongs to, such as `0x1::object::ObjectGroup`.
    ///
    /// Group members are deleted by a single `DeleteResource` of the group type, never
    /// individually, so the decoder needs this to route those deletes (ADR 0002).
    pub group: Option<StructName>,
}

/// A struct's fields, or a Move 2 enum's variants.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Body {
    Struct(Vec<Field>),
    Enum(Vec<Variant>),
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Field {
    pub name: Identifier,
    pub ty: TypeTag,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Variant {
    pub name: Identifier,
    pub fields: Vec<Field>,
}

impl StructLayout {
    /// Every field type in the layout, across all variants.
    pub fn field_types(&self) -> impl Iterator<Item = &TypeTag> {
        let fields: Box<dyn Iterator<Item = &Field>> = match &self.body {
            Body::Struct(fields) => Box::new(fields.iter()),
            Body::Enum(variants) => Box::new(variants.iter().flat_map(|v| v.fields.iter())),
        };
        fields.map(|f| &f.ty)
    }

    /// The named field's declared type, for a plain struct.
    #[must_use]
    pub fn field(&self, name: &str) -> Option<&TypeTag> {
        match &self.body {
            Body::Struct(fields) => fields.iter().find(|f| f.name == *name).map(|f| &f.ty),
            Body::Enum(_) => None,
        }
    }
}

/// Framework types the fullnode renders specially, so they're decoded without a
/// layout and never need one in the lock.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) enum Builtin {
    /// `0x1::string::String`, rendered as a JSON string.
    String,
    /// `0x1::option::Option<T>`, rendered as `{"vec": []}` or `{"vec": [x]}`.
    Option,
}

impl Builtin {
    pub(crate) fn of(name: &StructName) -> Option<Self> {
        if name.is(Address::ONE, "string", "String") {
            Some(Self::String)
        } else if name.is(Address::ONE, "option", "Option") {
            Some(Self::Option)
        } else {
            None
        }
    }
}

/// Call `f` on every struct name `ty` mentions, including inside type arguments.
pub(crate) fn visit_struct_names<'a>(ty: &'a TypeTag, f: &mut impl FnMut(&'a StructTag)) {
    match ty {
        TypeTag::Vector(inner) => visit_struct_names(inner, f),
        TypeTag::Struct(tag) => {
            f(tag);
            for arg in &tag.type_args {
                visit_struct_names(arg, f);
            }
        }
        _ => {}
    }
}

/// Well-known framework names used by the decoder.
pub(crate) mod framework {
    use nineveh_core::{Address, StructName};

    pub(crate) fn is_table(name: &StructName) -> bool {
        name.is(Address::ONE, "table", "Table")
            || name.is(Address::ONE, "table_with_length", "TableWithLength")
    }

    pub(crate) fn is_smart_table(name: &StructName) -> bool {
        name.is(Address::ONE, "smart_table", "SmartTable")
    }

    pub(crate) fn is_big_ordered_map(name: &StructName) -> bool {
        name.is(Address::ONE, "big_ordered_map", "BigOrderedMap")
    }
}
