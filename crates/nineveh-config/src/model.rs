//! The validated project config: what `nineveh.yaml` means once it's known to be
//! well-formed. See `docs/config.md` for the user-facing reference.

use std::fmt;

use nineveh_core::{Identifier, Network, StructName, StructTag, TypeTag, Value};

use crate::diagnostic::Span;

/// A validated `nineveh.yaml`, before it's resolved against `nineveh.lock`.
///
/// Everything that can be checked without the lock has been: names, references between
/// sources and state tables, column types and defaults, rule shapes. Spans point back
/// into the YAML so later stages can report errors in place.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Config {
    pub name: Named,
    pub network: Network,
    pub network_span: Option<Span>,
    pub start_version: StartVersion,
    pub sources: Vec<Source>,
    pub state: Vec<StateTable>,
    pub api: Api,
    pub realtime: Vec<Subscription>,
}

impl Config {
    #[must_use]
    pub fn source(&self, name: &str) -> Option<&Source> {
        self.sources.iter().find(|s| s.name.as_str() == name)
    }

    #[must_use]
    pub fn table(&self, name: &str) -> Option<&StateTable> {
        self.state.iter().find(|t| t.name.as_str() == name)
    }

    /// The structs `nineveh init` must pin in the lock: every source's type and, for
    /// table sources, the struct holding the table.
    #[must_use]
    pub fn roots(&self) -> Vec<StructName> {
        let mut roots = Vec::new();
        let mut add = |tag: &StructTag| {
            roots.push(tag.name.clone());
            for arg in &tag.type_args {
                visit(arg, &mut roots);
            }
        };
        for source in &self.sources {
            match &source.kind {
                SourceKind::Event(tag) | SourceKind::Resource(tag) => add(tag),
                SourceKind::Table { parent, .. } => add(parent),
            }
        }
        roots.sort();
        roots.dedup();
        roots
    }

    /// Everything in the config that shapes derived state, as canonical text: the
    /// network, start version, sources and state tables, without spans, comments or
    /// layout. Two configs with the same canonical text build the same state from
    /// the same lock, so a state schema records a hash of it (ADR 0005). The project's
    /// name, `api` and `realtime` don't change what's built and aren't included.
    #[must_use]
    pub fn canonical(&self) -> String {
        use fmt::Write as _;
        let mut out = String::new();
        let start = match self.start_version {
            StartVersion::Auto => "auto".to_owned(),
            StartVersion::Version(v) => v.to_string(),
        };
        // Writing to a String can't fail.
        let _ = writeln!(out, "network {}\nstart {start}", self.network);
        for source in &self.sources {
            let _ = match &source.kind {
                SourceKind::Event(tag) | SourceKind::Resource(tag) => {
                    writeln!(
                        out,
                        "source {} {} {tag}",
                        source.name,
                        source.kind.keyword()
                    )
                }
                SourceKind::Table { parent, field } => {
                    writeln!(out, "source {} table {parent}.{field}", source.name)
                }
            };
        }
        let expr = |e: &Expr| format!("{:?}", e.text);
        for table in &self.state {
            let _ = match &table.kind {
                TableKind::Mirror { source } => {
                    writeln!(out, "table {} mirror {source}", table.name)
                }
                TableKind::Log { source } => writeln!(out, "table {} log {source}", table.name),
                TableKind::Reduce {
                    key,
                    columns,
                    rules,
                } => {
                    let key: Vec<&str> = key.iter().map(Named::as_str).collect();
                    let _ = writeln!(out, "table {} reduce key {}", table.name, key.join(","));
                    for c in columns {
                        let _ = writeln!(
                            out,
                            "  column {} {} nullable={} default={:?}",
                            c.name, c.ty, c.nullable, c.default
                        );
                    }
                    for rule in rules {
                        let _ = write!(
                            out,
                            "  rule on {} deleted={}",
                            rule.on.source, rule.on.deleted
                        );
                        if let Some(when) = &rule.when {
                            let _ = write!(out, " when {}", expr(when));
                        }
                        for (name, e) in &rule.key {
                            let _ = write!(out, " key {name}={}", expr(e));
                        }
                        match &rule.action {
                            Action::Delete => out.push_str(" delete"),
                            Action::Set(set) => {
                                for (name, e) in set {
                                    let _ = write!(out, " set {name}={}", expr(e));
                                }
                            }
                        }
                        out.push('\n');
                    }
                    Ok(())
                }
            };
        }
        out
    }
}

fn visit(ty: &TypeTag, roots: &mut Vec<StructName>) {
    match ty {
        TypeTag::Vector(inner) => visit(inner, roots),
        TypeTag::Struct(tag) => {
            roots.push(tag.name.clone());
            for arg in &tag.type_args {
                visit(arg, roots);
            }
        }
        _ => {}
    }
}

/// A user-chosen name (project, source, table, column): lower snake case, at most 63
/// bytes, not starting with `_`. Names become Postgres identifiers and GraphQL fields,
/// so the rule is the intersection of what both accept unquoted.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Named {
    pub name: String,
    pub span: Option<Span>,
}

impl Named {
    #[must_use]
    pub fn as_str(&self) -> &str {
        &self.name
    }

    /// Whether `s` is a valid name.
    #[must_use]
    pub fn is_valid(s: &str) -> bool {
        const MAX: usize = 63;
        let mut bytes = s.bytes();
        matches!(bytes.next(), Some(b'a'..=b'z'))
            && s.len() <= MAX
            && bytes.all(|b| matches!(b, b'a'..=b'z' | b'0'..=b'9' | b'_'))
    }
}

impl fmt::Display for Named {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str(&self.name)
    }
}

/// Where processing starts.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum StartVersion {
    /// `nineveh init` resolves this to the version that published the first source's
    /// module, so nothing before it is scanned.
    Auto,
    Version(u64),
}

/// An input the project subscribes to.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Source {
    pub name: Named,
    pub kind: SourceKind,
    /// The type text in the YAML.
    pub type_span: Option<Span>,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum SourceKind {
    /// `event: <struct>`: a Move event. Without type arguments, a generic struct
    /// matches every instantiation.
    Event(StructTag),
    /// `resource: <struct>`: writes and deletes of a resource.
    Resource(StructTag),
    /// `table: <struct>.<field>`: items of the `Table`, `SmartTable` or
    /// `BigOrderedMap` held in that field (ADR 0003).
    Table {
        parent: StructTag,
        field: Identifier,
    },
}

impl SourceKind {
    /// `event`, `resource` or `table`.
    #[must_use]
    pub fn keyword(&self) -> &'static str {
        match self {
            Self::Event(_) => "event",
            Self::Resource(_) => "resource",
            Self::Table { .. } => "table",
        }
    }

    /// Whether the source has deletes (resources and table items do; events don't).
    #[must_use]
    pub fn has_deletes(&self) -> bool {
        !matches!(self, Self::Event(_))
    }
}

/// A materialized read model.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct StateTable {
    pub name: Named,
    pub kind: TableKind,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum TableKind {
    /// Rows keyed by `key`, folded from records by `rules`.
    Reduce {
        key: Vec<Named>,
        columns: Vec<Column>,
        rules: Vec<Rule>,
    },
    /// The latest value of each resource or table item from a source, deleted when it
    /// is. Columns come from the source's layout.
    Mirror { source: Named },
    /// One row per event from a source, append-only. Columns come from the event's
    /// layout.
    Log { source: Named },
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Column {
    pub name: Named,
    pub ty: ColumnType,
    pub nullable: bool,
    /// The value for a new row that no rule sets.
    pub default: Option<Value>,
}

/// A state column's type: Move's value types plus `json` for structured values.
/// Storage and API mapping follow ADR 0008.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum ColumnType {
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
    String,
    Bytes,
    Json,
}

impl ColumnType {
    pub const ALL: [Self; 17] = [
        Self::Bool,
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
        Self::Address,
        Self::String,
        Self::Bytes,
        Self::Json,
    ];

    #[must_use]
    pub fn as_str(self) -> &'static str {
        match self {
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
            Self::String => "string",
            Self::Bytes => "bytes",
            Self::Json => "json",
        }
    }

    #[must_use]
    pub fn parse(s: &str) -> Option<Self> {
        Self::ALL.into_iter().find(|t| t.as_str() == s)
    }

    /// Whether a column of this type can be part of a table's key. JSON can't: it has
    /// no canonical ordering or equality in Postgres.
    #[must_use]
    pub fn is_keyable(self) -> bool {
        self != Self::Json
    }

    /// Whether a Move value of type `ty` can be stored in this column as is, with no
    /// conversion expression. `Object<T>` stores as its address and `Option<T>` as a
    /// nullable `T` (ADR 0008).
    #[must_use]
    pub fn accepts(self, ty: &TypeTag, nullable: bool) -> bool {
        if self == Self::Json {
            return true;
        }
        if let Some(tag) = ty.as_struct() {
            let framework =
                |module: &str, name: &str| tag.name.is(nineveh_core::Address::ONE, module, name);
            if framework("option", "Option") {
                return nullable
                    && tag
                        .type_args
                        .first()
                        .is_some_and(|inner| self.accepts(inner, false));
            }
            if framework("object", "Object") {
                return self == Self::Address;
            }
            if framework("string", "String") {
                return self == Self::String;
            }
        }
        match (self, ty) {
            (Self::Bytes, TypeTag::Vector(inner)) => **inner == TypeTag::U8,
            (Self::Bool, TypeTag::Bool)
            | (Self::U8, TypeTag::U8)
            | (Self::U16, TypeTag::U16)
            | (Self::U32, TypeTag::U32)
            | (Self::U64, TypeTag::U64)
            | (Self::U128, TypeTag::U128)
            | (Self::U256, TypeTag::U256)
            | (Self::I8, TypeTag::I8)
            | (Self::I16, TypeTag::I16)
            | (Self::I32, TypeTag::I32)
            | (Self::I64, TypeTag::I64)
            | (Self::I128, TypeTag::I128)
            | (Self::I256, TypeTag::I256)
            | (Self::Address, TypeTag::Address) => true,
            _ => false,
        }
    }
}

impl fmt::Display for ColumnType {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str(self.as_str())
    }
}

/// One way records change a reduce table.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Rule {
    pub on: Trigger,
    /// Apply only to records for which this is true.
    pub when: Option<Expr>,
    /// Explicit key expressions. A key column not listed here takes the record field
    /// of the same name.
    pub key: Vec<(Named, Expr)>,
    pub action: Action,
}

/// The records a rule fires on.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Trigger {
    pub source: Named,
    /// `on: <source>.deleted`: the source's deletes rather than its writes.
    pub deleted: bool,
    pub span: Option<Span>,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Action {
    /// Upsert the keyed row, setting these columns.
    Set(Vec<(Named, Expr)>),
    /// Delete the keyed row.
    Delete,
}

/// A reducer expression, kept as source text until `nineveh-expr` checks it against
/// the resolved types.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Expr {
    pub text: String,
    pub span: Option<Span>,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct Api {
    pub rest: bool,
    pub graphql: bool,
}

impl Default for Api {
    fn default() -> Self {
        Self {
            rest: true,
            graphql: true,
        }
    }
}

/// A webhook fired by changes to a state table.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Subscription {
    pub table: Named,
    pub change: Change,
    pub webhook: String,
}

/// Which row changes a subscription fires on.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Change {
    Changed,
    Inserted,
    Updated,
    Deleted,
}

impl Change {
    pub const ALL: [Self; 4] = [Self::Changed, Self::Inserted, Self::Updated, Self::Deleted];

    #[must_use]
    pub fn as_str(self) -> &'static str {
        match self {
            Self::Changed => "changed",
            Self::Inserted => "inserted",
            Self::Updated => "updated",
            Self::Deleted => "deleted",
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn names_are_lower_snake_case_and_fit_postgres() {
        for ok in ["balances", "a", "user_id2"] {
            assert!(Named::is_valid(ok), "{ok}");
        }
        for bad in ["", "_x", "Balances", "2x", "a-b", "a b", &"a".repeat(64)] {
            assert!(!Named::is_valid(bad), "{bad}");
        }
    }

    #[test]
    fn columns_accept_their_move_types() {
        let ty = |s: &str| s.parse::<TypeTag>().unwrap();
        assert!(ColumnType::U64.accepts(&ty("u64"), false));
        assert!(
            !ColumnType::U128.accepts(&ty("u64"), false),
            "no implicit widening"
        );
        assert!(ColumnType::Address.accepts(
            &ty("0x1::object::Object<0x1::fungible_asset::Metadata>"),
            false
        ));
        assert!(ColumnType::String.accepts(&ty("0x1::string::String"), false));
        assert!(ColumnType::Bytes.accepts(&ty("vector<u8>"), false));
        assert!(!ColumnType::Bytes.accepts(&ty("vector<u64>"), false));
        assert!(ColumnType::U8.accepts(&ty("0x1::option::Option<u8>"), true));
        assert!(!ColumnType::U8.accepts(&ty("0x1::option::Option<u8>"), false));
        assert!(ColumnType::Json.accepts(&ty("vector<u64>"), false));
    }
}
