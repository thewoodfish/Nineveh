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
    /// Has the `key` ability, so it's stored at an address as a resource.
    pub is_resource: bool,
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

    /// The declared type of field `name`.
    ///
    /// For an enum, the field may be declared by several variants (versioned layouts
    /// like `V1`/`V2` usually repeat their fields); it's found when every variant that
    /// declares it gives it the same type.
    #[must_use]
    pub fn field(&self, name: &str) -> Option<&TypeTag> {
        match &self.body {
            Body::Struct(fields) => fields.iter().find(|f| f.name == *name).map(|f| &f.ty),
            Body::Enum(variants) => {
                let mut declared = variants
                    .iter()
                    .filter_map(|v| v.fields.iter().find(|f| f.name == *name))
                    .map(|f| &f.ty);
                let first = declared.next()?;
                declared.all(|ty| ty == first).then_some(first)
            }
        }
    }

    /// The fields every value of this type has: all of a struct's fields, or for an
    /// enum the fields that every variant declares with the same type. These are the
    /// fields a reducer can read without knowing the variant.
    #[must_use]
    pub fn common_fields(&self) -> Vec<&Field> {
        match &self.body {
            Body::Struct(fields) => fields.iter().collect(),
            Body::Enum(variants) => {
                let Some((first, rest)) = variants.split_first() else {
                    return Vec::new();
                };
                first
                    .fields
                    .iter()
                    .filter(|f| {
                        rest.iter()
                            .all(|v| v.fields.iter().any(|g| g.name == f.name && g.ty == f.ty))
                    })
                    .collect()
            }
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

#[cfg(test)]
mod tests {
    use super::*;

    fn field(name: &str, ty: &str) -> Field {
        Field {
            name: name.parse().unwrap(),
            ty: ty.parse().unwrap(),
        }
    }

    fn versioned() -> StructLayout {
        StructLayout {
            type_params: 0,
            body: Body::Enum(vec![
                Variant {
                    name: "V1".parse().unwrap(),
                    fields: vec![field("account", "address"), field("size", "u64")],
                },
                Variant {
                    name: "V2".parse().unwrap(),
                    fields: vec![
                        field("account", "address"),
                        field("size", "u128"),
                        field("counter_party", "address"),
                    ],
                },
            ]),
            is_event: true,
            is_resource: false,
            group: None,
        }
    }

    #[test]
    fn enum_fields_resolve_only_when_variants_agree() {
        let layout = versioned();
        assert_eq!(layout.field("account"), Some(&TypeTag::Address));
        assert_eq!(layout.field("counter_party"), Some(&TypeTag::Address));
        assert_eq!(layout.field("size"), None, "V1 and V2 disagree on its type");
        assert_eq!(layout.field("nope"), None);

        let common: Vec<&str> = layout
            .common_fields()
            .iter()
            .map(|f| f.name.as_str())
            .collect();
        assert_eq!(common, ["account"]);
    }
}
