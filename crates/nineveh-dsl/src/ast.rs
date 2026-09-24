//! The DSL's syntax tree, as written.
//!
//! Nothing here is resolved: a name is still just a name, and an expression is still
//! shaped the way the developer typed it. Resolution against the project's sources,
//! tables and columns happens in [`crate::scatter`], which is where the spans kept
//! here are spent.

use nineveh_config::{ColumnType, Span};

#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) struct Name {
    pub(crate) text: String,
    pub(crate) span: Span,
}

/// A whole `.nineveh.ts` file.
#[derive(Debug, Clone, PartialEq, Eq, Default)]
pub(crate) struct Program {
    pub(crate) tables: Vec<TableDecl>,
    pub(crate) handlers: Vec<Handler>,
}

/// `export const balances = table({ key: { … }, columns: { … } })`
#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) struct TableDecl {
    pub(crate) name: Name,
    pub(crate) key: Vec<ColumnDecl>,
    pub(crate) columns: Vec<ColumnDecl>,
}

/// `balance: u128.default(0)`, `memo: string.nullable()`.
#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) struct ColumnDecl {
    pub(crate) name: Name,
    pub(crate) ty: ColumnType,
    pub(crate) ty_span: Span,
    pub(crate) nullable: bool,
    /// The literal as written; converted against `ty` when the table is built.
    pub(crate) default: Option<Literal>,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) enum Literal {
    Int { digits: String, negative: bool },
    Str(String),
    Bool(bool),
}

/// `on(deposits, (d) => { … })`, or `on(vaults.deleted, …)`.
#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) struct Handler {
    pub(crate) source: Name,
    pub(crate) deleted: bool,
    /// The handler's parameter: a local alias for the record.
    pub(crate) param: Name,
    pub(crate) body: Vec<Stmt>,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) enum Stmt {
    /// `const x = <expr>`: a named value, substituted at its uses.
    Let {
        name: Name,
        value: Expr,
    },
    /// `const b = balances.row(<expr>, …)`: names the row a rule writes.
    Row {
        name: Name,
        table: Name,
        keys: Vec<Expr>,
    },
    /// `b.balance += <expr>`
    Assign {
        row: RowRef,
        column: Name,
        op: AssignOp,
        value: Expr,
    },
    /// `b.delete()`
    Delete {
        row: RowRef,
        span: Span,
    },
    If {
        cond: Expr,
        then: Vec<Stmt>,
        otherwise: Vec<Stmt>,
    },
    Return {
        span: Span,
    },
}

/// Which row a write is about: one named by a `const`, or written out in place.
#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) enum RowRef {
    Bound(Name),
    Inline {
        table: Name,
        keys: Vec<Expr>,
        span: Span,
    },
}

impl RowRef {
    pub(crate) fn span(&self) -> Span {
        match self {
            Self::Bound(n) => n.span,
            Self::Inline { span, .. } => *span,
        }
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) enum AssignOp {
    Set,
    Add,
    Sub,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) struct Expr {
    pub(crate) kind: ExprKind,
    pub(crate) span: Span,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) enum ExprKind {
    Int(String),
    Str(String),
    Bool(bool),
    Null,
    Name(String),
    /// `d.amount`, `b.balance`, `position.size`.
    Field(Box<Expr>, Name),
    /// `markets.get(m)`: a row of another table, which is always an option.
    Get {
        table: Name,
        keys: Vec<Expr>,
    },
    /// `?.column` on the result of a `get`.
    OptField(Box<Expr>, Name),
    /// `a ?? b`
    Coalesce(Box<Expr>, Box<Expr>),
    /// `c ? a : b`
    Ternary(Box<Expr>, Box<Expr>, Box<Expr>),
    Unary(UnOp, Box<Expr>),
    Binary(BinOp, Box<Expr>, Box<Expr>),
    /// `u128(x)`, `min(a, b)`, `address("0x1")`.
    Call(Name, Vec<Expr>),
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) enum UnOp {
    Not,
    Neg,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) enum BinOp {
    Or,
    And,
    Eq,
    Ne,
    Lt,
    Le,
    Gt,
    Ge,
    Add,
    Sub,
    Mul,
    Div,
    Rem,
}

impl BinOp {
    pub(crate) fn text(self) -> &'static str {
        match self {
            Self::Or => "||",
            Self::And => "&&",
            Self::Eq => "==",
            Self::Ne => "!=",
            Self::Lt => "<",
            Self::Le => "<=",
            Self::Gt => ">",
            Self::Ge => ">=",
            Self::Add => "+",
            Self::Sub => "-",
            Self::Mul => "*",
            Self::Div => "/",
            Self::Rem => "%",
        }
    }

    /// The operator that means the opposite, for folding `!(a == b)` into `a != b`.
    pub(crate) fn negated(self) -> Option<Self> {
        Some(match self {
            Self::Eq => Self::Ne,
            Self::Ne => Self::Eq,
            Self::Lt => Self::Ge,
            Self::Le => Self::Gt,
            Self::Gt => Self::Le,
            Self::Ge => Self::Lt,
            _ => return None,
        })
    }

    /// Binding power, loosest first, matching `nineveh-expr` (ADR 0007). Level 1 is
    /// reserved for `??`, which JavaScript refuses to mix with `&&`/`||` unparenthesised.
    pub(crate) fn power(self) -> u8 {
        match self {
            Self::Or => 2,
            Self::And => 3,
            Self::Eq | Self::Ne | Self::Lt | Self::Le | Self::Gt | Self::Ge => 4,
            Self::Add | Self::Sub => 5,
            Self::Mul | Self::Div | Self::Rem => 6,
        }
    }
}
