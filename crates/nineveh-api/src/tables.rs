//! A project's state tables as the API serves them, and the SQL that reads them.
//!
//! Rows come back as JSON built in SQL, in exactly the shape the change feed carries
//! (ADR 0008): integers up to 32 bits as numbers, wider ones as decimal strings,
//! addresses at full width, bytes as `0x` hex, structured values as JSON. So a client
//! can merge feed changes into rows it listed.
//!
//! Identifiers come only from the validated config and are always quoted; every value
//! is bound.

use nineveh_config::{ColumnType, Project, TableKind};
use nineveh_core::Address;
use serde::Serialize;

/// `jsonb_build_object` takes at most 100 arguments: 50 columns a call.
const COLUMNS_PER_OBJECT: usize = 50;

/// A state table's shape.
#[derive(Debug, Clone, Serialize)]
pub struct TableInfo {
    pub name: String,
    /// `reduce`, `mirror` or `log`.
    pub kind: &'static str,
    /// The key columns, in key order.
    pub key: Vec<String>,
    pub columns: Vec<ColumnInfo>,
}

#[derive(Debug, Clone, Serialize)]
pub struct ColumnInfo {
    pub name: String,
    /// The config's type name: `u64`, `address`, `json`, ….
    #[serde(rename = "type")]
    pub ty: String,
    pub nullable: bool,
    #[serde(skip)]
    pub(crate) column_type: ColumnType,
}

impl TableInfo {
    pub(crate) fn all(project: &Project) -> Vec<Self> {
        project
            .config()
            .state
            .iter()
            .zip(project.schemas())
            .map(|(table, schema)| Self {
                name: table.name.name.clone(),
                kind: match table.kind {
                    TableKind::Reduce { .. } => "reduce",
                    TableKind::Mirror { .. } => "mirror",
                    TableKind::Log { .. } => "log",
                },
                key: schema
                    .key
                    .iter()
                    .filter_map(|&i| schema.columns.get(i))
                    .map(|c| c.name.clone())
                    .collect(),
                columns: schema
                    .columns
                    .iter()
                    .map(|c| ColumnInfo {
                        name: c.name.clone(),
                        ty: c.ty.to_string(),
                        nullable: c.nullable,
                        column_type: c.ty,
                    })
                    .collect(),
            })
            .collect()
    }

    fn column(&self, name: &str) -> Option<&ColumnInfo> {
        self.columns.iter().find(|c| c.name == name)
    }
}

/// What a request asks of a table.
#[derive(Debug, Default)]
pub(crate) struct Query {
    pub(crate) filters: Vec<(String, String)>,
    pub(crate) order: Option<String>,
    pub(crate) limit: i64,
    pub(crate) offset: i64,
}

pub(crate) const DEFAULT_LIMIT: i64 = 50;
pub(crate) const MAX_LIMIT: i64 = 1000;

/// SQL and its bound values.
#[derive(Debug)]
pub(crate) struct Sql {
    pub(crate) text: String,
    pub(crate) binds: Vec<String>,
}

/// The rows of `table` a query selects, each as one JSON object, and the SQL to
/// count them.
pub(crate) fn select(schema: &str, table: &TableInfo, query: &Query) -> Result<(Sql, Sql), String> {
    let from = format!("{}.{}", ident(schema), ident(&table.name));
    let (conditions, binds) = conditions(table, &query.filters)?;
    let order = order(table, query.order.as_deref())?;

    let mut pairs = vec![format!("'_version', {}::text", ident("_version"))];
    pairs.extend(
        table
            .columns
            .iter()
            .map(|c| format!("{}, {}", literal(&c.name), json_expr(c))),
    );
    let object = pairs
        .chunks(COLUMNS_PER_OBJECT)
        .map(|chunk| format!("jsonb_build_object({})", chunk.join(", ")))
        .collect::<Vec<_>>()
        .join(" || ");

    let limit = binds.len() + 1;
    let rows = Sql {
        text: format!(
            "SELECT {object} FROM {from}{conditions} ORDER BY {order} LIMIT ${limit}::bigint \
             OFFSET ${}::bigint",
            limit + 1
        ),
        binds: {
            let mut all = binds.clone();
            all.push(query.limit.to_string());
            all.push(query.offset.to_string());
            all
        },
    };
    let count = Sql {
        text: format!("SELECT count(*) FROM {from}{conditions}"),
        binds,
    };
    Ok((rows, count))
}

/// A column as JSON, as the feed renders it.
fn json_expr(column: &ColumnInfo) -> String {
    let name = ident(&column.name);
    match column.column_type {
        ColumnType::Bool
        | ColumnType::U8
        | ColumnType::U16
        | ColumnType::U32
        | ColumnType::I8
        | ColumnType::I16
        | ColumnType::I32
        | ColumnType::Address
        | ColumnType::String
        | ColumnType::Json => name,
        ColumnType::U64
        | ColumnType::U128
        | ColumnType::U256
        | ColumnType::I64
        | ColumnType::I128
        | ColumnType::I256 => format!("{name}::text"),
        ColumnType::Bytes => format!("'0x' || encode({name}, 'hex')"),
    }
}

/// `WHERE column = value AND …`, one equality per filter.
fn conditions(
    table: &TableInfo,
    filters: &[(String, String)],
) -> Result<(String, Vec<String>), String> {
    let mut clauses = Vec::new();
    let mut binds = Vec::new();
    for (name, value) in filters {
        let column = table
            .column(name)
            .ok_or_else(|| format!("table `{}` has no column `{name}`", table.name))?;
        let n = binds.len() + 1;
        let (placeholder, value) = match column.column_type {
            ColumnType::Json => {
                return Err(format!("column `{name}` is JSON and can't be filtered on"));
            }
            ColumnType::Bool => (format!("${n}::boolean"), value.clone()),
            ColumnType::U8
            | ColumnType::U16
            | ColumnType::U32
            | ColumnType::I8
            | ColumnType::I16
            | ColumnType::I32
            | ColumnType::I64 => (format!("${n}::bigint"), value.clone()),
            ColumnType::U64
            | ColumnType::U128
            | ColumnType::U256
            | ColumnType::I128
            | ColumnType::I256 => (format!("${n}::numeric"), value.clone()),
            // Addresses are stored at full width, so `0x1` has to become that.
            ColumnType::Address => {
                let address: Address = value
                    .parse()
                    .map_err(|_| format!("`{value}` isn't an address"))?;
                (format!("${n}"), address.to_string())
            }
            ColumnType::String => (format!("${n}"), value.clone()),
            ColumnType::Bytes => (
                format!("decode(${n}, 'hex')"),
                value.strip_prefix("0x").unwrap_or(value).to_owned(),
            ),
        };
        clauses.push(format!("{} = {placeholder}", ident(name)));
        binds.push(value);
    }
    let text = if clauses.is_empty() {
        String::new()
    } else {
        format!(" WHERE {}", clauses.join(" AND "))
    };
    Ok((text, binds))
}

/// `ORDER BY`: a column or `_version`, optionally `.desc`, then `_key` so pages are
/// stable. By default, the most recently changed rows first.
fn order(table: &TableInfo, order: Option<&str>) -> Result<String, String> {
    let key = ident("_key");
    let Some(order) = order else {
        return Ok(format!("{} DESC, {key}", ident("_version")));
    };
    let (name, direction) = match order.rsplit_once('.') {
        Some((name, "desc")) => (name, "DESC"),
        Some((name, "asc")) => (name, "ASC"),
        _ => (order, "ASC"),
    };
    if name != "_version" && table.column(name).is_none() {
        return Err(format!(
            "table `{}` has no column `{name}` to order by",
            table.name
        ));
    }
    Ok(format!("{} {direction}, {key}", ident(name)))
}

pub(crate) fn ident(name: &str) -> String {
    format!("\"{}\"", name.replace('"', "\"\""))
}

fn literal(text: &str) -> String {
    format!("'{}'", text.replace('\'', "''"))
}

#[cfg(test)]
mod tests {
    use super::*;

    fn balances() -> TableInfo {
        let column = |name: &str, ty| ColumnInfo {
            name: name.into(),
            ty: format!("{ty}"),
            nullable: false,
            column_type: ty,
        };
        TableInfo {
            name: "balances".into(),
            kind: "reduce",
            key: vec!["user".into()],
            columns: vec![
                column("user", ColumnType::Address),
                column("balance", ColumnType::U128),
                column("memo", ColumnType::Bytes),
            ],
        }
    }

    #[test]
    fn selects_rows_as_the_feed_renders_them() {
        let query = Query {
            filters: vec![("user".into(), "0x1".into())],
            order: Some("balance.desc".into()),
            limit: 10,
            offset: 20,
        };
        let (rows, count) = select("vault", &balances(), &query).unwrap();
        assert_eq!(
            rows.text,
            r#"SELECT jsonb_build_object('_version', "_version"::text, 'user', "user", 'balance', "balance"::text, 'memo', '0x' || encode("memo", 'hex')) FROM "vault"."balances" WHERE "user" = $1 ORDER BY "balance" DESC, "_key" LIMIT $2::bigint OFFSET $3::bigint"#
        );
        assert_eq!(
            rows.binds,
            [
                "0x0000000000000000000000000000000000000000000000000000000000000001",
                "10",
                "20"
            ]
        );
        assert_eq!(
            count.text,
            r#"SELECT count(*) FROM "vault"."balances" WHERE "user" = $1"#
        );
    }

    #[test]
    fn newest_changes_come_first_by_default() {
        let (rows, _) = select("vault", &balances(), &Query::default()).unwrap();
        assert!(rows.text.contains(r#"ORDER BY "_version" DESC, "_key""#));
    }

    #[test]
    fn unknown_columns_and_bad_addresses_are_refused() {
        let query = |filters: Vec<(String, String)>, order: Option<&str>| Query {
            filters,
            order: order.map(Into::into),
            ..Query::default()
        };
        let table = balances();
        assert!(select("v", &table, &query(vec![("nope".into(), "1".into())], None)).is_err());
        assert!(
            select(
                "v",
                &table,
                &query(vec![("user".into(), "0xzz".into())], None)
            )
            .is_err()
        );
        assert!(select("v", &table, &query(vec![], Some("nope.desc"))).is_err());
    }

    #[test]
    fn wide_tables_split_the_json_object() {
        let mut table = balances();
        for i in 0..60 {
            table.columns.push(ColumnInfo {
                name: format!("c{i}"),
                ty: "bool".into(),
                nullable: false,
                column_type: ColumnType::Bool,
            });
        }
        let (rows, _) = select("v", &table, &Query::default()).unwrap();
        assert_eq!(rows.text.matches("jsonb_build_object(").count(), 2);
    }
}
