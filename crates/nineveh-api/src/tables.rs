//! A project's state tables as the API serves them, and the SQL that reads them.
//!
//! Rows come back as JSON built in SQL, in exactly the shape the change feed carries
//! (ADR 0008): integers up to 32 bits as numbers, wider ones as decimal strings,
//! addresses at full width, bytes as `0x` hex, structured values as JSON. So a client
//! can merge feed changes into rows it listed.
//!
//! Identifiers come only from the validated config and are always quoted; every value
//! is bound.

use std::collections::HashMap;

use nineveh_config::{ColumnType, Project, TableKind};
use nineveh_core::Address;
use serde::Serialize;
use sqlx::PgPool;

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
    /// How many rows it holds, only when `?counts=true` asked and the table could be
    /// read. Absent otherwise, because a shape is what this endpoint is for.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub rows: Option<Rows>,
}

/// How many rows a table holds, and whether that is a count or an estimate.
#[derive(Debug, Clone, Copy, Serialize)]
pub struct Rows {
    pub count: i64,
    /// True when the rows were counted, false when the planner's estimate was taken.
    pub exact: bool,
}

/// The most rows [`count_rows`] will read before giving up and estimating.
///
/// Counting is a sequential scan, so an unbounded `count(*)` over every table is a way
/// to make a dashboard take a minute on a busy project. Reading this many is quick, and
/// past it a reader wants a magnitude anyway — nobody reads "8,412,905" as anything but
/// "about eight million".
const COUNT_LIMIT: i64 = 50_000;

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
                rows: None,
            })
            .collect()
    }

    fn column(&self, name: &str) -> Option<&ColumnInfo> {
        self.columns.iter().find(|c| c.name == name)
    }
}

/// The planner's row estimate for every table in `schema`, by table name.
///
/// Free: it reads what `ANALYZE` already recorded rather than the tables themselves. It
/// is also allowed to be absent or stale — `-1` until a table has ever been analyzed,
/// which is exactly the state a project is in just after its first build — so it decides
/// only whether a table is small enough to count, never what gets reported on its own.
pub(crate) async fn estimates(
    pool: &PgPool,
    schema: &str,
) -> Result<HashMap<String, i64>, sqlx::Error> {
    let rows: Vec<(String, i64)> = sqlx::query_as(
        "SELECT c.relname, c.reltuples::bigint \
         FROM pg_class c JOIN pg_namespace n ON n.oid = c.relnamespace \
         WHERE n.nspname = $1 AND c.relkind = 'r'",
    )
    .bind(schema)
    .fetch_all(pool)
    .await?;
    Ok(rows.into_iter().collect())
}

/// How many rows `table` holds, counted when that is cheap and estimated when it isn't.
///
/// The count is capped by reading at most [`COUNT_LIMIT`] rows, so the work this does is
/// bounded whatever the table's size and whatever the planner believes. Hitting the cap
/// means the table is at least that big, and then the estimate is the better answer —
/// unless it is missing too, in which case the cap is reported as an approximation,
/// which understates a huge table until the next `ANALYZE` and says so by being
/// approximate.
pub(crate) async fn count_rows(
    pool: &PgPool,
    schema: &str,
    table: &str,
    estimate: Option<i64>,
) -> Result<Rows, sqlx::Error> {
    if let Some(rows) = without_counting(estimate) {
        return Ok(rows);
    }
    let counted: i64 = sqlx::query_scalar(&format!(
        "SELECT count(*) FROM (SELECT 1 FROM {}.{} LIMIT $1::bigint) t",
        ident(schema),
        ident(table)
    ))
    .bind(COUNT_LIMIT)
    .fetch_one(pool)
    .await?;
    Ok(counted_rows(estimate, counted))
}

/// The answer when the estimate alone settles it: the table is known to be past the cap,
/// so counting could only confirm what reading it would cost.
fn without_counting(estimate: Option<i64>) -> Option<Rows> {
    estimate.filter(|&e| e >= COUNT_LIMIT).map(|count| Rows {
        count,
        exact: false,
    })
}

/// The answer once a count capped at [`COUNT_LIMIT`] has come back.
///
/// Under the cap the count read the whole table, so it is the truth. At the cap the
/// table is bigger than the count says and the only question is by how much: the
/// estimate if there is one, and the cap itself if there isn't — understating the table
/// until the next `ANALYZE`, which is why that answer is marked inexact.
fn counted_rows(estimate: Option<i64>, counted: i64) -> Rows {
    if counted < COUNT_LIMIT {
        return Rows {
            count: counted,
            exact: true,
        };
    }
    Rows {
        count: estimate.unwrap_or(counted).max(counted),
        exact: false,
    }
}

/// What a request asks of a table.
#[derive(Debug, Default)]
pub(crate) struct Query {
    pub(crate) filters: Vec<Filter>,
    pub(crate) order: Option<String>,
    pub(crate) limit: i64,
    pub(crate) offset: i64,
}

/// One comparison against one column: `?balance.gte=100`.
#[derive(Debug)]
pub(crate) struct Filter {
    pub(crate) column: String,
    pub(crate) op: Op,
    pub(crate) value: String,
}

/// How a filter compares.
///
/// A closed set, deliberately. Each one is a single Postgres operator against a single
/// column, so what a request can ask stays something the planner can serve from an
/// index on that column. Anything that needs more than this — a join, an aggregate, a
/// condition spanning columns — is a reducer's job, computed once as state is written
/// rather than per request.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) enum Op {
    Eq,
    Neq,
    Gt,
    Gte,
    Lt,
    Lte,
}

/// The operators, as a request spells them, for an error that names them all.
pub(crate) const OPERATORS: &str = "eq, neq, gt, gte, lt, lte";

impl Op {
    /// The operator a `column.suffix` parameter names, if it names one.
    pub(crate) fn parse(suffix: &str) -> Option<Self> {
        match suffix {
            "eq" => Some(Self::Eq),
            "neq" => Some(Self::Neq),
            "gt" => Some(Self::Gt),
            "gte" => Some(Self::Gte),
            "lt" => Some(Self::Lt),
            "lte" => Some(Self::Lte),
            _ => None,
        }
    }

    fn sql(self) -> &'static str {
        match self {
            Self::Eq => "=",
            Self::Neq => "<>",
            Self::Gt => ">",
            Self::Gte => ">=",
            Self::Lt => "<",
            Self::Lte => "<=",
        }
    }

    /// Whether this only asks whether two values are the same, rather than which is
    /// greater — the distinction that decides what a boolean column can be asked.
    fn is_equality(self) -> bool {
        matches!(self, Self::Eq | Self::Neq)
    }
}

pub(crate) const DEFAULT_LIMIT: i64 = 50;
pub(crate) const MAX_LIMIT: i64 = 1000;

/// How many columns an `order` may name.
///
/// A second one is what a leaderboard needs to break a tie, and a third is the tie-break
/// for that. Past this it is a sort nobody asked a product question about, and every
/// column named is one the planner may have to sort on rather than read in order.
pub(crate) const MAX_ORDER: usize = 4;

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

/// `WHERE column <op> value AND …`, one comparison per filter.
fn conditions(table: &TableInfo, filters: &[Filter]) -> Result<(String, Vec<String>), String> {
    let mut clauses = Vec::new();
    let mut binds = Vec::new();
    for Filter {
        column: name,
        op,
        value,
    } in filters
    {
        let column = table
            .column(name)
            .ok_or_else(|| format!("table `{}` has no column `{name}`", table.name))?;
        let n = binds.len() + 1;
        let (placeholder, value) = match column.column_type {
            ColumnType::Json => {
                return Err(format!("column `{name}` is JSON and can't be filtered on"));
            }
            // Two booleans are equal or they are not; which of them is greater is not
            // a question about the data.
            ColumnType::Bool if !op.is_equality() => {
                return Err(format!(
                    "column `{name}` is a boolean, so it takes eq and neq, not an ordering"
                ));
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
        clauses.push(format!("{} {} {placeholder}", ident(name), op.sql()));
        binds.push(value);
    }
    let text = if clauses.is_empty() {
        String::new()
    } else {
        format!(" WHERE {}", clauses.join(" AND "))
    };
    Ok((text, binds))
}

/// `ORDER BY`: columns or `_version`, each optionally `.desc`, then `_key` so pages
/// are stable. By default, the most recently changed rows first.
///
/// Several columns, because one is often not enough: a leaderboard ordered on revenue
/// has ties, and which row comes first within a tie decides what page two shows. `_key`
/// stays last and settles whatever the named columns left open, so paging through a
/// table neither repeats a row nor skips one.
fn order(table: &TableInfo, order: Option<&str>) -> Result<String, String> {
    let key = ident("_key");
    let Some(order) = order else {
        return Ok(format!("{} DESC, {key}", ident("_version")));
    };
    let terms: Vec<&str> = order.split(',').map(str::trim).collect();
    if terms.len() > MAX_ORDER {
        return Err(format!(
            "`order` takes at most {MAX_ORDER} columns, and this names {}",
            terms.len()
        ));
    }
    let mut by = Vec::with_capacity(terms.len() + 1);
    for term in terms {
        if term.is_empty() {
            return Err("`order` has an empty column name in it".to_owned());
        }
        let (name, direction) = match term.rsplit_once('.') {
            Some((name, "desc")) => (name, "DESC"),
            Some((name, "asc")) => (name, "ASC"),
            _ => (term, "ASC"),
        };
        if name != "_version" && table.column(name).is_none() {
            return Err(format!(
                "table `{}` has no column `{name}` to order by",
                table.name
            ));
        }
        by.push(format!("{} {direction}", ident(name)));
    }
    by.push(key);
    Ok(by.join(", "))
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

    #[test]
    fn a_table_is_counted_until_counting_would_mean_reading_it() {
        // Nothing analyzed yet — which is where a project sits right after its first
        // build — so the table gets counted however big the planner thinks it is.
        assert!(without_counting(None).is_none());
        assert!(without_counting(Some(-1)).is_none());
        assert!(without_counting(Some(COUNT_LIMIT - 1)).is_none());

        // Known to be past the cap: reading it could only confirm that.
        let big = without_counting(Some(4_000_000)).expect("estimate settles it");
        assert_eq!(big.count, 4_000_000);
        assert!(!big.exact);
    }

    #[test]
    fn a_count_under_the_cap_is_the_truth_and_one_at_it_is_not() {
        // The whole table fitted inside the cap, so this is not an estimate.
        let small = counted_rows(None, 1_204);
        assert_eq!(small.count, 1_204);
        assert!(small.exact);

        // Nothing is exact once the count stopped early. A stale estimate that knows
        // better wins; with none, the cap is all there is, and it understates.
        let stale = counted_rows(Some(8_000_000), COUNT_LIMIT);
        assert_eq!(stale.count, 8_000_000);
        assert!(!stale.exact);

        let blind = counted_rows(None, COUNT_LIMIT);
        assert_eq!(blind.count, COUNT_LIMIT);
        assert!(!blind.exact);

        // An estimate that is somehow smaller than what was actually read is not the
        // better answer just because it exists.
        assert_eq!(counted_rows(Some(12), COUNT_LIMIT).count, COUNT_LIMIT);
    }

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
            rows: None,
        }
    }

    /// `?column=value`, the equality a bare parameter means.
    fn eq(column: &str, value: &str) -> Filter {
        Filter {
            column: column.into(),
            op: Op::Eq,
            value: value.into(),
        }
    }

    fn filter(column: &str, op: Op, value: &str) -> Filter {
        Filter {
            column: column.into(),
            op,
            value: value.into(),
        }
    }

    #[test]
    fn selects_rows_as_the_feed_renders_them() {
        let query = Query {
            filters: vec![eq("user", "0x1")],
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
        let query = |filters: Vec<Filter>, order: Option<&str>| Query {
            filters,
            order: order.map(Into::into),
            ..Query::default()
        };
        let table = balances();
        assert!(select("v", &table, &query(vec![eq("nope", "1")], None)).is_err());
        assert!(select("v", &table, &query(vec![eq("user", "0xzz")], None)).is_err());
        assert!(select("v", &table, &query(vec![], Some("nope.desc"))).is_err());
    }

    #[test]
    fn a_range_is_two_comparisons_on_one_column() {
        let query = Query {
            filters: vec![
                filter("balance", Op::Gte, "100"),
                filter("balance", Op::Lt, "500"),
            ],
            ..Query::default()
        };
        let (rows, count) = select("v", &balances(), &query).unwrap();
        assert!(
            rows.text
                .contains(r#"WHERE "balance" >= $1::numeric AND "balance" < $2::numeric"#),
            "{}",
            rows.text
        );
        // The last two are limit and offset, which `Query::default()` leaves at zero —
        // the handler is what applies DEFAULT_LIMIT.
        assert_eq!(rows.binds, ["100", "500", "0", "0"]);
        // The count is of the same rows, so it carries the same conditions and binds.
        assert!(count.text.contains(r#""balance" >= $1::numeric"#));
        assert_eq!(count.binds, ["100", "500"]);
    }

    #[test]
    fn every_operator_has_its_postgres_spelling() {
        let sql = |op| {
            let query = Query {
                filters: vec![filter("balance", op, "1")],
                ..Query::default()
            };
            select("v", &balances(), &query).unwrap().0.text
        };
        for (op, spelling) in [
            (Op::Eq, "="),
            (Op::Neq, "<>"),
            (Op::Gt, ">"),
            (Op::Gte, ">="),
            (Op::Lt, "<"),
            (Op::Lte, "<="),
        ] {
            assert!(
                sql(op).contains(&format!(r#""balance" {spelling} $1::numeric"#)),
                "{op:?} should compare with {spelling}"
            );
        }
    }

    #[test]
    fn operators_are_only_the_ones_named() {
        for (text, op) in [
            ("eq", Op::Eq),
            ("neq", Op::Neq),
            ("gt", Op::Gt),
            ("gte", Op::Gte),
            ("lt", Op::Lt),
            ("lte", Op::Lte),
        ] {
            assert_eq!(Op::parse(text), Some(op));
            // The error that names the set has to actually name it.
            assert!(
                OPERATORS.contains(text),
                "`{text}` is missing from OPERATORS"
            );
        }
        // Not a query language: no pattern match, no set membership, no null test.
        for text in ["like", "ilike", "in", "is", "ne", "gte_", "GT", ""] {
            assert_eq!(Op::parse(text), None, "`{text}` should not be an operator");
        }
    }

    #[test]
    fn an_address_filter_is_widened_whatever_the_operator() {
        let query = Query {
            filters: vec![filter("user", Op::Gt, "0x1")],
            ..Query::default()
        };
        let (rows, _) = select("v", &balances(), &query).unwrap();
        // Addresses are stored at full width, so a comparison against `0x1` has to be
        // against the full-width form or it compares as a shorter string and is wrong.
        assert_eq!(
            rows.binds[0],
            "0x0000000000000000000000000000000000000000000000000000000000000001"
        );
    }

    #[test]
    fn a_boolean_is_equal_or_not_and_nothing_else() {
        let mut table = balances();
        table.columns.push(ColumnInfo {
            name: "frozen".into(),
            ty: "bool".into(),
            nullable: false,
            column_type: ColumnType::Bool,
        });
        let query = |op| Query {
            filters: vec![filter("frozen", op, "true")],
            ..Query::default()
        };

        assert!(select("v", &table, &query(Op::Eq)).is_ok());
        assert!(select("v", &table, &query(Op::Neq)).is_ok());
        // Which of two booleans is greater is not a question about the data, and
        // Postgres would happily answer it.
        let refused = select("v", &table, &query(Op::Gt)).unwrap_err();
        assert!(refused.contains("eq and neq"), "{refused}");
        assert!(select("v", &table, &query(Op::Lte)).is_err());
    }

    #[test]
    fn json_is_still_not_filterable_by_any_operator() {
        let mut table = balances();
        table.columns.push(ColumnInfo {
            name: "meta".into(),
            ty: "json".into(),
            nullable: false,
            column_type: ColumnType::Json,
        });
        for op in [Op::Eq, Op::Neq, Op::Gt, Op::Lte] {
            let query = Query {
                filters: vec![filter("meta", op, "{}")],
                ..Query::default()
            };
            assert!(select("v", &table, &query).is_err());
        }
    }

    #[test]
    fn a_tie_is_broken_by_the_next_column_and_then_by_the_key() {
        let query = Query {
            order: Some("balance.desc,user.asc".into()),
            ..Query::default()
        };
        let (rows, _) = select("v", &balances(), &query).unwrap();
        assert!(
            rows.text
                .contains(r#"ORDER BY "balance" DESC, "user" ASC, "_key""#),
            "{}",
            rows.text
        );
    }

    #[test]
    fn order_is_bounded_and_every_column_in_it_is_checked() {
        let order = |text: &str| {
            select(
                "v",
                &balances(),
                &Query {
                    order: Some(text.into()),
                    ..Query::default()
                },
            )
        };
        // Whitespace around a comma is a person writing a list, not an error.
        assert!(order("balance.desc, user").is_ok());
        assert!(order("_version.desc,balance").is_ok());
        // A column that does not exist is refused wherever it sits in the list.
        assert!(order("balance,nope").is_err());
        assert!(order("nope,balance").is_err());
        assert!(order("balance,,user").is_err());
        assert!(order(",balance").is_err());
        // Past the cap it is a sort nobody asked a product question about.
        assert!(order("balance,user,memo,_version").is_ok());
        let refused = order("balance,user,memo,_version,balance").unwrap_err();
        assert!(refused.contains("at most 4"), "{refused}");
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
