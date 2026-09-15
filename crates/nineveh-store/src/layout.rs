//! How a project's state is laid out in Postgres: one schema per project, one table
//! per state table, and the SQL that creates and writes them.
//!
//! Every table is keyed by `_key`, the exact encoding of the engine's key
//! ([`codec`](crate::codec)). Tables the fold reads back (`reduce` and `mirror`) also
//! keep the engine's row in `_row`; `log` tables are append-only and never read, so
//! they don't. `_version` is the version of the row's last change. Then come the typed
//! columns the API serves, mapped by ADR 0008.
//!
//! The engine's internal tables live beside the state they gate, as `_handles` and
//! `_buckets`, so a rebuild reproduces them.
//!
//! Identifiers come only from the validated config and are always quoted; values are
//! always bound.

use nineveh_config::{ColumnType, Project, ResolvedTable, SchemaColumn, TableSchema};
use nineveh_engine::TableId;

/// Bump when the tables this module creates change shape, so existing schemas are
/// rebuilt rather than written in a layout they don't have.
///
/// 2: enum values get a column per field (and `_variant`), not one JSON `value`.
pub(crate) const LAYOUT_VERSION: u32 = 2;

/// A table in a project's schema, as the store writes it.
#[derive(Debug, Clone)]
pub(crate) struct Table {
    pub(crate) id: TableId,
    pub(crate) name: String,
    /// Keeps the engine's row in `_row`, for the fold to read back.
    pub(crate) readable: bool,
    /// Typed columns, for state tables. Internal tables have none.
    pub(crate) schema: Option<TableSchema>,
}

impl Table {
    pub(crate) fn all(project: &Project) -> Vec<Self> {
        let mut tables: Vec<Self> = project
            .config()
            .state
            .iter()
            .zip(project.tables())
            .zip(project.schemas())
            .enumerate()
            .map(|(i, ((table, resolved), schema))| Self {
                id: TableId::State(u32::try_from(i).unwrap_or(u32::MAX)),
                name: table.name.name.clone(),
                readable: !matches!(resolved, ResolvedTable::Log { .. }),
                schema: Some(schema.clone()),
            })
            .collect();
        for (id, name) in [
            (TableId::Handles, "_handles"),
            (TableId::Buckets, "_buckets"),
        ] {
            tables.push(Self {
                id,
                name: name.to_owned(),
                readable: true,
                schema: None,
            });
        }
        tables
    }

    fn columns(&self) -> &[SchemaColumn] {
        self.schema.as_ref().map_or(&[], |s| s.columns.as_slice())
    }

    /// `CREATE TABLE` and its indexes.
    pub(crate) fn create(&self, schema: &str) -> Vec<String> {
        let table = qualified(schema, &self.name);
        let mut defs = vec![format!("{} bytea PRIMARY KEY", ident("_key"))];
        if self.readable {
            defs.push(format!("{} bytea NOT NULL", ident("_row")));
        }
        if self.schema.is_some() {
            defs.push(format!("{} bigint NOT NULL", ident("_version")));
        }
        for c in self.columns() {
            let null = if c.nullable { "" } else { " NOT NULL" };
            defs.push(format!("{} {}{null}", ident(&c.name), sql_type(c.ty)));
        }
        let mut statements = vec![format!("CREATE TABLE {table} ({})", defs.join(", "))];
        // Lookups by key through the API. Not unique: typed values can collide where
        // the exact key doesn't, such as strings that differ only in a NUL.
        if let Some(schema) = &self.schema {
            let keyed: Vec<String> = schema
                .key
                .iter()
                .map(|&i| &schema.columns[i])
                .filter(|c| c.ty.is_keyable())
                .map(|c| ident(&c.name))
                .collect();
            if !keyed.is_empty() {
                statements.push(format!("CREATE INDEX ON {table} ({})", keyed.join(", ")));
            }
        }
        statements
    }

    /// `INSERT … SELECT FROM UNNEST(…) ON CONFLICT (_key) DO UPDATE`: one statement
    /// per table per batch, with one array parameter per column. Parameters are
    /// `_key`, then `_row` if readable, then `_version` and the typed columns.
    pub(crate) fn upsert(&self, schema: &str) -> String {
        let mut names = vec![ident("_key")];
        let mut params = vec!["$1::bytea[]".to_owned()];
        let mut next = 2;
        let mut param = |cast: &str| {
            let p = format!("${next}::{cast}");
            next += 1;
            p
        };
        if self.readable {
            names.push(ident("_row"));
            params.push(param("bytea[]"));
        }
        if self.schema.is_some() {
            names.push(ident("_version"));
            params.push(param("bigint[]"));
        }
        for c in self.columns() {
            names.push(ident(&c.name));
            params.push(param(bind_cast(c.ty)));
        }
        let updates: Vec<String> = names[1..]
            .iter()
            .map(|n| format!("{n} = EXCLUDED.{n}"))
            .collect();
        let conflict = if updates.is_empty() {
            "DO NOTHING".to_owned()
        } else {
            format!("DO UPDATE SET {}", updates.join(", "))
        };
        format!(
            "INSERT INTO {} ({}) SELECT * FROM UNNEST({}) ON CONFLICT ({}) {conflict}",
            qualified(schema, &self.name),
            names.join(", "),
            params.join(", "),
            ident("_key"),
        )
    }

    pub(crate) fn delete(&self, schema: &str) -> String {
        format!(
            "DELETE FROM {} WHERE {} = ANY($1::bytea[])",
            qualified(schema, &self.name),
            ident("_key")
        )
    }

    pub(crate) fn select(&self, schema: &str) -> String {
        format!(
            "SELECT {k}, {r} FROM {} WHERE {k} = ANY($1::bytea[])",
            qualified(schema, &self.name),
            k = ident("_key"),
            r = ident("_row"),
        )
    }
}

/// The Postgres type of a column (ADR 0008).
pub(crate) fn sql_type(ty: ColumnType) -> &'static str {
    match ty {
        ColumnType::Bool => "boolean",
        ColumnType::U8 | ColumnType::U16 | ColumnType::I8 | ColumnType::I16 | ColumnType::I32 => {
            "integer"
        }
        ColumnType::U32 | ColumnType::I64 => "bigint",
        ColumnType::U64 => "numeric(20,0)",
        ColumnType::U128 | ColumnType::I128 => "numeric(39,0)",
        ColumnType::I256 => "numeric(77,0)",
        ColumnType::U256 => "numeric(78,0)",
        ColumnType::Address | ColumnType::String => "text",
        ColumnType::Bytes => "bytea",
        ColumnType::Json => "jsonb",
    }
}

/// How a column's array parameter is bound: numerics as decimal text and JSON as
/// text, cast in SQL.
fn bind_cast(ty: ColumnType) -> &'static str {
    match ty {
        ColumnType::Bool => "boolean[]",
        ColumnType::U8 | ColumnType::U16 | ColumnType::I8 | ColumnType::I16 | ColumnType::I32 => {
            "integer[]"
        }
        ColumnType::U32 | ColumnType::I64 => "bigint[]",
        ColumnType::U64
        | ColumnType::U128
        | ColumnType::U256
        | ColumnType::I128
        | ColumnType::I256 => "text[]::numeric[]",
        ColumnType::Address | ColumnType::String => "text[]",
        ColumnType::Bytes => "bytea[]",
        ColumnType::Json => "text[]::jsonb[]",
    }
}

/// A quoted identifier. Names come from the validated config, but quoting keeps SQL
/// keywords like `user` usable and costs nothing.
pub(crate) fn ident(name: &str) -> String {
    format!("\"{}\"", name.replace('"', "\"\""))
}

pub(crate) fn qualified(schema: &str, table: &str) -> String {
    format!("{}.{}", ident(schema), ident(table))
}

#[cfg(test)]
mod tests {
    use nineveh_config::Projection;

    use super::*;

    fn balances() -> Table {
        let column = |name: &str, ty, i| SchemaColumn {
            name: name.to_owned(),
            ty,
            nullable: false,
            from: Projection::Row(i),
        };
        Table {
            id: TableId::State(0),
            name: "balances".into(),
            readable: true,
            schema: Some(TableSchema {
                columns: vec![
                    column("user", ColumnType::Address, 0),
                    column("balance", ColumnType::U128, 1),
                ],
                key: vec![0],
            }),
        }
    }

    #[test]
    fn creates_typed_tables_keyed_by_the_exact_key() {
        assert_eq!(
            balances().create("vault"),
            [
                r#"CREATE TABLE "vault"."balances" ("_key" bytea PRIMARY KEY, "_row" bytea NOT NULL, "_version" bigint NOT NULL, "user" text NOT NULL, "balance" numeric(39,0) NOT NULL)"#,
                r#"CREATE INDEX ON "vault"."balances" ("user")"#,
            ]
        );
    }

    #[test]
    fn upserts_from_arrays() {
        assert_eq!(
            balances().upsert("vault"),
            r#"INSERT INTO "vault"."balances" ("_key", "_row", "_version", "user", "balance") SELECT * FROM UNNEST($1::bytea[], $2::bytea[], $3::bigint[], $4::text[], $5::text[]::numeric[]) ON CONFLICT ("_key") DO UPDATE SET "_row" = EXCLUDED."_row", "_version" = EXCLUDED."_version", "user" = EXCLUDED."user", "balance" = EXCLUDED."balance""#
        );
    }

    #[test]
    fn identifiers_are_quoted() {
        assert_eq!(ident("user"), "\"user\"");
        assert_eq!(ident("a\"b"), "\"a\"\"b\"");
    }
}
