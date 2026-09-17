//! The expression language end to end: typechecking, evaluation, and property tests of
//! the arithmetic against Rust's own checked integer operations as a reference.

#![allow(
    clippy::unwrap_used,
    clippy::panic,
    reason = "test-only crate: helpers panic on unexpected results"
)]

use nineveh_core::{Address, I256, Identifier, StructTag, TypeTag, U256, Value};
use nineveh_expr::{
    Cell, ColumnVar, Env, EvalErrorKind, Inputs, IntType, NoTables, Structs, TableColumn, TableVar,
    Tables, Tx, Type, compile,
};
use proptest::prelude::*;

/// A `Position` struct with an `Object<Market>` field, for field access and flattening.
struct Layouts;

impl Structs for Layouts {
    fn fields(&self, tag: &StructTag) -> Option<Vec<(Identifier, TypeTag)>> {
        (tag.name.name.as_str() == "Position").then(|| {
            vec![
                (id("size"), TypeTag::U64),
                (id("market"), ty("0x1::object::Object<0xabc::m::Market>")),
            ]
        })
    }
}

fn id(s: &str) -> Identifier {
    s.parse().unwrap()
}

fn ty(s: &str) -> TypeTag {
    s.parse().unwrap()
}

fn columns() -> Vec<ColumnVar> {
    let col = |name: &str, ty: Type, readable| ColumnVar {
        name: name.into(),
        ty,
        readable,
    };
    vec![
        col("user", Type::Address, true),
        col("balance", Type::Int(IntType::U128), true),
        col("count", Type::Int(IntType::U64), true),
        col("note", Type::Option(Box::new(Type::String)), true),
        col("first_seen", Type::Int(IntType::U64), false),
        col("amount", Type::Int(IntType::U64), true),
    ]
}

fn record() -> Vec<(Identifier, TypeTag)> {
    vec![
        (id("amount"), TypeTag::U64),
        (id("big"), TypeTag::U128),
        (id("pnl"), TypeTag::I64),
        (id("owner"), TypeTag::Address),
        (id("memo"), ty("0x1::option::Option<0x1::string::String>")),
        (id("position"), ty("0xabc::m::Position")),
        (id("market"), ty("0x1::object::Object<0xabc::m::Market>")),
    ]
}

fn row_values() -> Vec<Value> {
    vec![
        Value::Address(Address::special(7)),
        Value::U128(100),
        Value::U64(3),
        Value::Option(None),
        Value::U64(0),
        Value::U64(9),
    ]
}

fn record_values() -> Vec<Value> {
    let object = |a: u8| Value::Struct(vec![(id("inner"), Value::Address(Address::special(a)))]);
    vec![
        Value::U64(40),
        Value::U128(1_000),
        Value::I64(-25),
        Value::Address(Address::special(7)),
        Value::Option(Some(Box::new(Value::String("hi".into())))),
        Value::Struct(vec![
            (id("size"), Value::U64(5)),
            (id("market"), object(0xb)),
        ]),
        object(0xc),
    ]
}

fn run(text: &str, target: &Type) -> Result<Value, String> {
    let (columns, record) = (columns(), record());
    let env = Env {
        columns: &columns,
        record: &record,
        source: "deposits",
        tables: &[],
        structs: &Layouts,
    };
    let compiled = compile(text, &env, target).map_err(|e| format!("compile: {}", e.message))?;
    assert_eq!(compiled.ty(), target, "{text}");
    let (row, rec) = (row_values(), record_values());
    let inputs = Inputs {
        row: &row,
        record: &rec,
        tx: Tx {
            version: 42,
            timestamp_micros: 1_700_000_000_000_000,
        },
        tables: &NoTables,
    };
    compiled.eval(&inputs).map_err(|e| format!("eval: {e}"))
}

fn ok(text: &str, target: &Type) -> Value {
    run(text, target).unwrap_or_else(|e| panic!("{text}: {e}"))
}

fn compile_error(text: &str, target: &Type) -> String {
    match run(text, target) {
        Err(e) if e.starts_with("compile: ") => e["compile: ".len()..].to_owned(),
        other => panic!("{text}: expected a compile error, got {other:?}"),
    }
}

const U64: Type = Type::Int(IntType::U64);
const U128: Type = Type::Int(IntType::U128);
const I64: Type = Type::Int(IntType::I64);
const BOOL: Type = Type::Bool;

#[test]
fn reducer_staples() {
    assert_eq!(
        ok("balance + u128(deposits.amount)", &U128),
        Value::U128(140)
    );
    assert_eq!(ok("count + 1", &U64), Value::U64(4));
    assert_eq!(ok("max(row.amount, deposits.amount)", &U64), Value::U64(40));
    assert_eq!(ok("min(deposits.amount, 10)", &U64), Value::U64(10));
    assert_eq!(ok("if pnl < 0 then 0 else 1", &U64), Value::U64(0));
    assert_eq!(ok("abs(pnl)", &I64), Value::I64(25));
    assert_eq!(ok("-pnl", &I64), Value::I64(25));
    assert_eq!(ok("owner == user && big > 999", &BOOL), Value::Bool(true));
    assert_eq!(ok("tx.version * 2", &U64), Value::U64(84));
    assert_eq!(ok("7 / 2 + 7 % 2", &U64), Value::U64(4));
}

#[test]
fn options_and_nullable_columns() {
    let nullable = Type::Option(Box::new(Type::String));
    assert_eq!(
        ok("memo", &nullable),
        Value::Option(Some(Box::new(Value::String("hi".into()))))
    );
    // A plain value is accepted where a nullable one is expected.
    assert_eq!(
        ok("'x'", &nullable),
        Value::Option(Some(Box::new(Value::String("x".into()))))
    );
    assert_eq!(ok("null", &nullable), Value::Option(None));
    assert_eq!(
        ok("unwrap_or(note, 'none')", &Type::String),
        Value::String("none".into())
    );
    assert_eq!(
        ok("is_some(memo) && is_none(note)", &BOOL),
        Value::Bool(true)
    );
    assert_eq!(ok("note == null", &BOOL), Value::Bool(true));
}

#[test]
fn objects_read_as_addresses() {
    assert_eq!(
        ok("market", &Type::Address),
        Value::Address(Address::special(0xc))
    );
    assert_eq!(
        ok("position.market", &Type::Address),
        Value::Address(Address::special(0xb))
    );
    assert_eq!(ok("position.size + deposits.amount", &U64), Value::U64(45));
    assert_eq!(ok("market == @0xc", &BOOL), Value::Bool(true));
}

#[test]
fn anything_goes_into_json() {
    assert!(matches!(ok("position", &Type::Json), Value::Struct(_)));
}

#[test]
fn runtime_errors_are_located_and_deterministic() {
    // `amount` here is ambiguous, so qualify it.
    let cases = [
        (
            "deposits.amount - 41",
            U64,
            "`-` overflowed: the result doesn't fit in u64",
            0..20,
        ),
        ("count / (count - 3)", U64, "division by zero", 0..19),
        (
            "u8(big)",
            Type::Int(IntType::U8),
            "the value doesn't fit in u8",
            0..7,
        ),
        (
            "abs(-9223372036854775808i64)",
            I64,
            "`abs` overflowed: the result doesn't fit in i64",
            0..28,
        ),
    ];
    for (text, target, message, span) in cases {
        let (columns, record) = (columns(), record());
        let env = Env {
            columns: &columns,
            record: &record,
            source: "deposits",
            tables: &[],
            structs: &Layouts,
        };
        let compiled = compile(text, &env, &target).unwrap();
        let (row, rec) = (row_values(), record_values());
        let inputs = Inputs {
            row: &row,
            record: &rec,
            tx: Tx::default(),
            tables: &NoTables,
        };
        let err = compiled.eval(&inputs).unwrap_err();
        assert_eq!(err.to_string(), message, "{text}");
        assert_eq!(err.span.start..err.span.end, span, "{text}");
        assert!(!err.is_retryable());
        // Same inputs, same failure.
        assert_eq!(compiled.eval(&inputs).unwrap_err(), err);
    }
}

#[test]
fn type_errors_explain_themselves() {
    let cases = [
        (
            "amount",
            U64,
            "`amount` is both a column and a field of the record",
        ),
        ("balance + big", U128, ""),
        (
            "balance + amount",
            U128,
            "`amount` is both a column and a field of the record",
        ),
        (
            "balance + deposits.amount",
            U128,
            "expected u128, found u64",
        ),
        (
            "first_seen + 1",
            U64,
            "`first_seen` has no default, so a new row has no value to read",
        ),
        ("-count", U64, "can't negate an unsigned u64"),
        (
            "256",
            Type::Int(IntType::U8),
            "this number doesn't fit in u8",
        ),
        ("count > 1 > 0", BOOL, "comparisons can't be chained"),
        (
            "owner + 1",
            Type::Address,
            "`+` needs integers, found address",
        ),
        ("null", U64, "`null` needs a nullable context"),
        ("sqrt(count)", U64, "unknown function `sqrt`"),
        ("mni(count, 1)", U64, "unknown function `mni`"),
        ("balanse", U128, "unknown name `balanse`"),
        ("tx.sender", Type::Address, "`tx` has no field `sender`"),
        (
            "position.nope",
            U64,
            "`Position` has no field `nope` every value has",
        ),
        ("position == position", BOOL, ""),
    ];
    for (text, target, message) in cases {
        if message.is_empty() {
            ok(text, &target);
            continue;
        }
        let got = compile_error(text, &target);
        assert!(
            got.contains(message),
            "{text}: got {got:?}, want {message:?}"
        );
    }
}

#[test]
fn integer_conversions_are_explicit() {
    let (columns, record) = (columns(), record());
    let env = Env {
        columns: &columns,
        record: &record,
        source: "deposits",
        tables: &[],
        structs: &Layouts,
    };
    let e = compile("deposits.amount", &env, &U128).unwrap_err();
    assert_eq!(e.message, "expected u128, found u64");
    assert_eq!(
        e.help.as_deref(),
        Some("integers never convert implicitly; write `u128(...)` to convert")
    );
    let e = compile("mni(count, 1)", &env, &U64).unwrap_err();
    assert_eq!(e.help.as_deref(), Some("did you mean `min`?"));
    assert_eq!((e.span.start, e.span.end), (0, 3));
}

#[test]
fn literals_take_their_type_from_context() {
    assert_eq!(ok("1", &U128), Value::U128(1));
    assert_eq!(ok("1 + balance", &U128), Value::U128(101));
    assert_eq!(ok("if count > 0 then 5 else 0", &U128), Value::U128(5));
    assert_eq!(ok("-128i8", &Type::Int(IntType::I8)), Value::I8(-128));
    assert_eq!(
        ok(
            "115792089237316195423570985008687907853269984665640564039457584007913129639935",
            &Type::Int(IntType::U256)
        ),
        Value::U256(U256::MAX)
    );
    assert_eq!(
        ok(
            "-57896044618658097711785492504343953926634992332820282019728792003956564819968",
            &Type::Int(IntType::I256)
        ),
        Value::I256(I256::MIN)
    );
}

// --- arithmetic against a reference ---------------------------------------------

/// Evaluate `a <op> b` at type `ty` with both operands as literals.
fn arith(op: &str, a: &str, b: &str, ty: IntType) -> Option<String> {
    let text = format!("({a}{t}) {op} ({b}{t})", t = ty.as_str());
    let target = Type::Int(ty);
    let empty_cols: Vec<ColumnVar> = Vec::new();
    let empty_rec: Vec<(Identifier, TypeTag)> = Vec::new();
    let env = Env {
        columns: &empty_cols,
        record: &empty_rec,
        source: "s",
        tables: &[],
        structs: &Layouts,
    };
    let compiled = compile(&text, &env, &target).unwrap_or_else(|e| panic!("{text}: {e}"));
    let inputs = Inputs {
        row: &[],
        record: &[],
        tx: Tx::default(),
        tables: &NoTables,
    };
    match compiled.eval(&inputs) {
        Ok(value) => Some(serde_json_free(&value)),
        Err(e) => {
            assert!(matches!(
                e.kind,
                EvalErrorKind::Overflow { .. } | EvalErrorKind::DivideByZero
            ));
            None
        }
    }
}

fn serde_json_free(value: &Value) -> String {
    match value {
        Value::U8(v) => v.to_string(),
        Value::U16(v) => v.to_string(),
        Value::U32(v) => v.to_string(),
        Value::U64(v) => v.to_string(),
        Value::U128(v) => v.to_string(),
        Value::U256(v) => v.to_string(),
        Value::I8(v) => v.to_string(),
        Value::I16(v) => v.to_string(),
        Value::I32(v) => v.to_string(),
        Value::I64(v) => v.to_string(),
        Value::I128(v) => v.to_string(),
        Value::I256(v) => v.to_string(),
        other => panic!("not an integer: {other:?}"),
    }
}

/// Literal text for a value; negatives are written `-N`, which the parser folds.
fn lit<T: ToString>(v: &T) -> String {
    v.to_string()
}

macro_rules! reference {
    ($name:ident, $t:ty, $ty:expr) => {
        proptest! {
            #[test]
            fn $name(a: $t, b: $t) {
                let (sa, sb) = (lit(&a), lit(&b));
                prop_assert_eq!(arith("+", &sa, &sb, $ty), a.checked_add(b).map(|v| v.to_string()));
                prop_assert_eq!(arith("-", &sa, &sb, $ty), a.checked_sub(b).map(|v| v.to_string()));
                prop_assert_eq!(arith("*", &sa, &sb, $ty), a.checked_mul(b).map(|v| v.to_string()));
                prop_assert_eq!(arith("/", &sa, &sb, $ty), a.checked_div(b).map(|v| v.to_string()));
                prop_assert_eq!(arith("%", &sa, &sb, $ty), a.checked_rem(b).map(|v| v.to_string()));
            }
        }
    };
}

reference!(u8_matches_rust, u8, IntType::U8);
reference!(u32_matches_rust, u32, IntType::U32);
reference!(u64_matches_rust, u64, IntType::U64);
reference!(u128_matches_rust, u128, IntType::U128);
reference!(i8_matches_rust, i8, IntType::I8);
reference!(i32_matches_rust, i32, IntType::I32);
reference!(i64_matches_rust, i64, IntType::I64);
reference!(i128_matches_rust, i128, IntType::I128);

proptest! {
    /// u256 against u128 operands, where the exact result is known from u128 math.
    #[test]
    fn u256_matches_wide_products(a: u128, b: u128) {
        let expected = U256::from(a) * U256::from(b);
        prop_assert_eq!(arith("*", &lit(&a), &lit(&b), IntType::U256), Some(expected.to_string()));
        prop_assert_eq!(
            arith("+", &lit(&a), &lit(&b), IntType::U256),
            Some((U256::from(a) + U256::from(b)).to_string())
        );
    }

    /// Casts are exact or fail: a round trip through a wider type never changes a value.
    #[test]
    fn casts_round_trip(a: i64) {
        let text = format!("i64(i256({a}i64))");
        let empty_cols: Vec<ColumnVar> = Vec::new();
        let empty_rec: Vec<(Identifier, TypeTag)> = Vec::new();
        let env = Env { columns: &empty_cols, record: &empty_rec, source: "s", tables: &[], structs: &Layouts };
        let compiled = compile(&text, &env, &I64).unwrap();
        let inputs = Inputs { row: &[], record: &[], tx: Tx::default(), tables: &NoTables };
        prop_assert_eq!(compiled.eval(&inputs).unwrap(), Value::I64(a));
        let to_u64 = compile(&format!("u64({a}i64)"), &env, &U64).unwrap().eval(&inputs);
        prop_assert_eq!(to_u64.is_ok(), a >= 0);
    }
}

// --- reading other tables --------------------------------------------------------

/// Two tables a rule can read: `holders`, keyed by address with plain columns, and
/// `vaults`, a mirror whose stored row holds the whole struct.
fn tables() -> Vec<TableVar> {
    let column = |name: &str, ty: Type, cell| TableColumn {
        name: name.into(),
        ty,
        cell,
    };
    let holders = TableVar {
        name: "holders".into(),
        index: 0,
        key: vec![column("user", Type::Address, Cell::At(0))],
        columns: vec![
            column("user", Type::Address, Cell::At(0)),
            column("balance", Type::Int(IntType::U128), Cell::At(1)),
            column("note", Type::String, Cell::At(2)),
        ],
        readable: true,
    };
    let vaults = TableVar {
        name: "vaults".into(),
        index: 1,
        key: vec![column("address", Type::Address, Cell::At(0))],
        columns: vec![
            column("address", Type::Address, Cell::At(0)),
            column("size", Type::Int(IntType::U64), Cell::Field(1, id("size"))),
            column("market", Type::Address, Cell::Field(1, id("market"))),
        ],
        readable: true,
    };
    let history = TableVar {
        name: "history".into(),
        index: 2,
        key: vec![column("version", Type::Int(IntType::U64), Cell::At(0))],
        columns: vec![column("version", Type::Int(IntType::U64), Cell::At(0))],
        readable: false,
    };
    vec![holders, vaults, history]
}

/// `holders` has one row, for address 7; `vaults` has one, holding a `Position`.
struct Rows;

impl Tables for Rows {
    fn row(&self, table: u32, key: &[Value]) -> Option<Vec<Value>> {
        let seven = [Value::Address(Address::special(7))];
        match (table, key == seven) {
            (0, true) => Some(vec![
                Value::Address(Address::special(7)),
                Value::U128(250),
                Value::Option(None),
            ]),
            (1, true) => Some(vec![
                Value::Address(Address::special(7)),
                Value::Struct(vec![
                    (id("size"), Value::U64(5)),
                    (
                        id("market"),
                        Value::Struct(vec![(id("inner"), Value::Address(Address::special(0xb)))]),
                    ),
                ]),
            ]),
            _ => None,
        }
    }
}

fn read(text: &str, target: &Type) -> Result<Value, String> {
    let (columns, record, tables) = (columns(), record(), tables());
    let env = Env {
        columns: &columns,
        record: &record,
        source: "deposits",
        tables: &tables,
        structs: &Layouts,
    };
    let compiled = compile(text, &env, target).map_err(|e| {
        format!(
            "compile: {}{}",
            e.message,
            e.help.map(|h| format!(" [{h}]")).unwrap_or_default()
        )
    })?;
    let (row, rec) = (row_values(), record_values());
    compiled
        .eval(&Inputs {
            row: &row,
            record: &rec,
            tx: Tx::default(),
            tables: &Rows,
        })
        .map_err(|e| format!("eval: {e}"))
}

#[test]
fn a_rule_reads_other_tables_by_key() {
    let some = |v: Value| Value::Option(Some(Box::new(v)));
    // `owner` is address 7, the row both tables hold.
    assert_eq!(
        read("holders[owner].balance", &Type::Option(Box::new(U128))),
        Ok(some(Value::U128(250)))
    );
    // A row that isn't there reads as null, so `unwrap_or` gives a total answer.
    assert_eq!(
        read("unwrap_or(holders[@0x9].balance, 0)", &U128),
        Ok(Value::U128(0))
    );
    assert_eq!(
        read("is_some(holders[@0x9].user)", &BOOL),
        Ok(Value::Bool(false))
    );
    assert_eq!(
        read("is_some(holders[owner].user)", &BOOL),
        Ok(Value::Bool(true))
    );
    // A null column reads as null, like a missing row.
    assert_eq!(
        read("is_none(holders[owner].note)", &BOOL),
        Ok(Value::Bool(true))
    );
    // A mirror's columns are fields of the struct its row holds, and an `Object<T>`
    // reads as its address, as the API serves it.
    assert_eq!(
        read("unwrap_or(vaults[owner].size, 0) + deposits.amount", &U64),
        Ok(Value::U64(45))
    );
    assert_eq!(
        read("unwrap_or(vaults[owner].market, @0x0) == @0xb", &BOOL),
        Ok(Value::Bool(true))
    );
    // Lookups compose: a key can be anything, including another lookup.
    assert_eq!(
        read(
            "unwrap_or(holders[unwrap_or(vaults[owner].address, @0x0)].balance, 0)",
            &U128
        ),
        Ok(Value::U128(250))
    );
}

#[test]
fn table_reads_say_what_is_wrong() {
    let cases = [
        (
            "holders[owner].blance",
            "compile: `holders` has no column `blance` [did you mean `balance`?]",
        ),
        (
            "holders[owner, 1].balance",
            "compile: `holders` is keyed by 1 column, got 2 [write `holders[user]`]",
        ),
        ("holders[1].balance", "compile: expected address, found u64"),
        (
            "holders.balance",
            "compile: unknown name `holders` [`holders` is a table; read a row of it, like \
             `holders[user].x`]",
        ),
        (
            "unwrap_or(holders[owner], 0)",
            "compile: a row of `holders` isn't a value on its own [read one of its columns, like \
             `holders[...].x`]",
        ),
        (
            "unwrap_or(history[1].version, 0)",
            "compile: `history` is a log table, so it has no rows to look up [logs are \
             append-only history; look up a state or mirror table]",
        ),
        (
            "holder[owner].balance",
            "compile: unknown table `holder` [did you mean `holders`?]",
        ),
    ];
    for (text, message) in cases {
        assert_eq!(read(text, &U128).unwrap_err(), message, "{text}");
    }
}
