//! Lexing and parsing: expression text to an untyped tree.
//!
//! A hand-written Pratt parser (ADR 0007): the grammar is small, and error quality
//! matters more than generator convenience. Precedence, loosest first:
//!
//! ```text
//! if c then a else b
//! ||
//! &&
//! == != < <= > >=      (non-associative: `a < b < c` is an error)
//! + -
//! * / %
//! ! -                  (prefix)
//! .field  f(args)      (postfix)
//! ```

use nineveh_core::{Address, U256};

use crate::error::{ExprError, Span};
use crate::num::parse_digits;
use crate::types::IntType;

/// Nesting deeper than this is rejected rather than risking the stack. Configs run on
/// hosted infrastructure, so expression text is untrusted.
const MAX_DEPTH: usize = 64;

#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) struct Expr {
    pub(crate) kind: ExprKind,
    pub(crate) span: Span,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) enum ExprKind {
    Int {
        magnitude: U256,
        negative: bool,
        suffix: Option<IntType>,
    },
    Bool(bool),
    Str(String),
    Address(Address),
    Null,
    Name(String),
    Field(Box<Expr>, String, Span),
    Unary(UnOp, Box<Expr>),
    Binary(BinOp, Box<Expr>, Box<Expr>),
    If(Box<Expr>, Box<Expr>, Box<Expr>),
    Call(String, Span, Vec<Expr>),
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
    pub(crate) fn symbol(self) -> &'static str {
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

    fn binding_power(self) -> u8 {
        match self {
            Self::Or => 1,
            Self::And => 2,
            Self::Eq | Self::Ne | Self::Lt | Self::Le | Self::Gt | Self::Ge => 3,
            Self::Add | Self::Sub => 4,
            Self::Mul | Self::Div | Self::Rem => 5,
        }
    }

    pub(crate) fn is_comparison(self) -> bool {
        self.binding_power() == 3
    }
}

const PREFIX_POWER: u8 = 6;

// --- lexer -----------------------------------------------------------------------

#[derive(Debug, Clone, PartialEq, Eq)]
enum Tok {
    Int(U256, Option<IntType>),
    Str(String),
    Address(Address),
    Ident(String),
    If,
    Then,
    Else,
    True,
    False,
    Null,
    Op(BinOp),
    Bang,
    LParen,
    RParen,
    Comma,
    Dot,
    Eof,
}

fn describe(tok: &Tok) -> String {
    match tok {
        Tok::Int(..) => "a number".into(),
        Tok::Str(_) => "a string".into(),
        Tok::Address(_) => "an address".into(),
        Tok::Ident(name) => format!("`{name}`"),
        Tok::If => "`if`".into(),
        Tok::Then => "`then`".into(),
        Tok::Else => "`else`".into(),
        Tok::True => "`true`".into(),
        Tok::False => "`false`".into(),
        Tok::Null => "`null`".into(),
        Tok::Op(op) => format!("`{}`", op.symbol()),
        Tok::Bang => "`!`".into(),
        Tok::LParen => "`(`".into(),
        Tok::RParen => "`)`".into(),
        Tok::Comma => "`,`".into(),
        Tok::Dot => "`.`".into(),
        Tok::Eof => "the end of the expression".into(),
    }
}

fn lex(text: &str) -> Result<Vec<(Tok, Span)>, ExprError> {
    let bytes = text.as_bytes();
    let mut tokens = Vec::new();
    let mut i = 0;
    while i < bytes.len() {
        let start = i;
        let c = bytes[i];
        if c.is_ascii_whitespace() {
            i += 1;
            continue;
        }
        let two = bytes.get(i..i + 2);
        let (tok, len) = match c {
            b'0'..=b'9' => number(text, i)?,
            b'a'..=b'z' | b'A'..=b'Z' | b'_' => word(text, i)?,
            b'@' => {
                let end = scan(bytes, i + 1, |b| b.is_ascii_alphanumeric());
                let address = text[i + 1..end].parse().map_err(|_| {
                    ExprError::new("invalid address", Span::new(start, end))
                        .help("write addresses as `@0x` followed by hex digits, like `@0x1`")
                })?;
                (Tok::Address(address), end - i)
            }
            b'"' | b'\'' => {
                let (value, end) = string(text, i)?;
                (Tok::Str(value), end - i)
            }
            _ => match two {
                Some(b"==") => (Tok::Op(BinOp::Eq), 2),
                Some(b"!=") => (Tok::Op(BinOp::Ne), 2),
                Some(b"<=") => (Tok::Op(BinOp::Le), 2),
                Some(b">=") => (Tok::Op(BinOp::Ge), 2),
                Some(b"&&") => (Tok::Op(BinOp::And), 2),
                Some(b"||") => (Tok::Op(BinOp::Or), 2),
                _ => match c {
                    b'<' => (Tok::Op(BinOp::Lt), 1),
                    b'>' => (Tok::Op(BinOp::Gt), 1),
                    b'+' => (Tok::Op(BinOp::Add), 1),
                    b'-' => (Tok::Op(BinOp::Sub), 1),
                    b'*' => (Tok::Op(BinOp::Mul), 1),
                    b'/' => (Tok::Op(BinOp::Div), 1),
                    b'%' => (Tok::Op(BinOp::Rem), 1),
                    b'!' => (Tok::Bang, 1),
                    b'(' => (Tok::LParen, 1),
                    b')' => (Tok::RParen, 1),
                    b',' => (Tok::Comma, 1),
                    b'.' => (Tok::Dot, 1),
                    b'=' => {
                        return Err(ExprError::new(
                            "`=` isn't an operator",
                            Span::new(start, start + 1),
                        )
                        .help("compare with `==`"));
                    }
                    _ => {
                        let ch = text[i..].chars().next().unwrap_or('?');
                        return Err(ExprError::new(
                            format!("unexpected character `{ch}`"),
                            Span::new(start, start + ch.len_utf8()),
                        ));
                    }
                },
            },
        };
        tokens.push((tok, Span::new(start, start + len)));
        i += len;
    }
    tokens.push((Tok::Eof, Span::new(text.len(), text.len())));
    Ok(tokens)
}

/// A number literal at `i`: digits with optional `_` separators and a type suffix.
fn number(text: &str, i: usize) -> Result<(Tok, usize), ExprError> {
    let bytes = text.as_bytes();
    let digits_end = scan(bytes, i, |b| b.is_ascii_digit() || b == b'_');
    if bytes.get(digits_end) == Some(&b'x') && &text[i..digits_end] == "0" {
        return Err(
            ExprError::new("hex numbers aren't supported", Span::new(i, digits_end + 1))
                .help("addresses are written with `@`, like `@0x1`"),
        );
    }
    let suffix_end = scan(bytes, digits_end, |b| b.is_ascii_alphanumeric());
    let suffix = &text[digits_end..suffix_end];
    let suffix = if suffix.is_empty() {
        None
    } else {
        Some(IntType::parse(suffix).ok_or_else(|| {
            ExprError::new(
                format!("unknown integer suffix `{suffix}`"),
                Span::new(digits_end, suffix_end),
            )
            .help("use a Move integer type, like `1u128` or `-1i64`")
        })?)
    };
    let magnitude = parse_digits(&text[i..digits_end])
        .ok_or_else(|| ExprError::new("number is too large", Span::new(i, digits_end)))?;
    Ok((Tok::Int(magnitude, suffix), suffix_end - i))
}

/// A keyword or identifier at `i`.
fn word(text: &str, i: usize) -> Result<(Tok, usize), ExprError> {
    let end = scan(text.as_bytes(), i, |b| {
        b.is_ascii_alphanumeric() || b == b'_'
    });
    let word = &text[i..end];
    let tok = match word {
        "if" => Tok::If,
        "then" => Tok::Then,
        "else" => Tok::Else,
        "true" => Tok::True,
        "false" => Tok::False,
        "null" => Tok::Null,
        "and" | "or" | "not" => {
            let symbol = match word {
                "and" => "&&",
                "or" => "||",
                _ => "!",
            };
            return Err(
                ExprError::new(format!("`{word}` isn't an operator"), Span::new(i, end))
                    .help(format!("write `{symbol}`")),
            );
        }
        _ => Tok::Ident(word.to_owned()),
    };
    Ok((tok, end - i))
}

fn scan(bytes: &[u8], from: usize, accept: impl Fn(u8) -> bool) -> usize {
    bytes[from..]
        .iter()
        .position(|&b| !accept(b))
        .map_or(bytes.len(), |n| from + n)
}

/// A quoted string starting at `start`; returns its value and the index past the
/// closing quote.
fn string(text: &str, start: usize) -> Result<(String, usize), ExprError> {
    let quote = text[start..].chars().next().unwrap_or('"');
    let mut value = String::new();
    let mut chars = text[start + 1..].char_indices();
    while let Some((offset, c)) = chars.next() {
        let at = start + 1 + offset;
        match c {
            '\\' => {
                let Some((_, escaped)) = chars.next() else {
                    break;
                };
                value.push(match escaped {
                    'n' => '\n',
                    't' => '\t',
                    '\\' | '\'' | '"' => escaped,
                    other => {
                        return Err(ExprError::new(
                            format!("unknown escape `\\{other}`"),
                            Span::new(at, at + 1 + other.len_utf8()),
                        ));
                    }
                });
            }
            c if c == quote => return Ok((value, at + 1)),
            c => value.push(c),
        }
    }
    Err(ExprError::new(
        "unterminated string",
        Span::new(start, text.len()),
    ))
}

// --- parser ----------------------------------------------------------------------

/// Parse expression text into an untyped tree.
pub(crate) fn parse(text: &str) -> Result<Expr, ExprError> {
    let tokens = lex(text)?;
    let mut parser = Parser { tokens, pos: 0 };
    let expr = parser.expr(0, 0)?;
    match parser.peek() {
        Tok::Eof => Ok(expr),
        tok => Err(ExprError::new(
            format!("expected an operator or the end, found {}", describe(tok)),
            parser.span(),
        )),
    }
}

struct Parser {
    tokens: Vec<(Tok, Span)>,
    pos: usize,
}

impl Parser {
    fn peek(&self) -> &Tok {
        self.tokens.get(self.pos).map_or(&Tok::Eof, |(t, _)| t)
    }

    fn span(&self) -> Span {
        self.tokens
            .get(self.pos)
            .or_else(|| self.tokens.last())
            .map_or(Span::new(0, 0), |(_, s)| *s)
    }

    fn bump(&mut self) -> (Tok, Span) {
        let token = self
            .tokens
            .get(self.pos)
            .cloned()
            .unwrap_or((Tok::Eof, self.span()));
        self.pos += 1;
        token
    }

    fn expect(&mut self, tok: &Tok, what: &str) -> Result<Span, ExprError> {
        if self.peek() == tok {
            Ok(self.bump().1)
        } else {
            Err(ExprError::new(
                format!("expected {what}, found {}", describe(self.peek())),
                self.span(),
            ))
        }
    }

    fn expr(&mut self, min_power: u8, depth: usize) -> Result<Expr, ExprError> {
        if depth > MAX_DEPTH {
            return Err(ExprError::new(
                "expression is nested too deeply",
                self.span(),
            ));
        }
        let mut lhs = self.prefix(depth)?;
        while let Tok::Op(op) = *self.peek() {
            let power = op.binding_power();
            if power <= min_power {
                break;
            }
            self.bump();
            let rhs = self.expr(power, depth + 1)?;
            if op.is_comparison()
                && let Tok::Op(next) = *self.peek()
                && next.is_comparison()
            {
                return Err(
                    ExprError::new("comparisons can't be chained", self.span()).help(format!(
                        "write `a {} b && b {} c`",
                        op.symbol(),
                        next.symbol()
                    )),
                );
            }
            let span = lhs.span.to(rhs.span);
            lhs = Expr {
                kind: ExprKind::Binary(op, Box::new(lhs), Box::new(rhs)),
                span,
            };
        }
        Ok(lhs)
    }

    fn prefix(&mut self, depth: usize) -> Result<Expr, ExprError> {
        let (tok, span) = self.bump();
        let expr = match tok {
            Tok::Int(magnitude, suffix) => Expr {
                kind: ExprKind::Int {
                    magnitude,
                    negative: false,
                    suffix,
                },
                span,
            },
            Tok::Str(s) => Expr {
                kind: ExprKind::Str(s),
                span,
            },
            Tok::Address(a) => Expr {
                kind: ExprKind::Address(a),
                span,
            },
            Tok::True | Tok::False => Expr {
                kind: ExprKind::Bool(tok == Tok::True),
                span,
            },
            Tok::Null => Expr {
                kind: ExprKind::Null,
                span,
            },
            Tok::Op(BinOp::Sub) => {
                // `-5` is a negative literal, so `-128i8` is in range.
                if let Tok::Int(magnitude, suffix) = *self.peek() {
                    let (_, int_span) = self.bump();
                    Expr {
                        kind: ExprKind::Int {
                            magnitude,
                            negative: true,
                            suffix,
                        },
                        span: span.to(int_span),
                    }
                } else {
                    let operand = self.expr(PREFIX_POWER, depth + 1)?;
                    Expr {
                        span: span.to(operand.span),
                        kind: ExprKind::Unary(UnOp::Neg, Box::new(operand)),
                    }
                }
            }
            Tok::Bang => {
                let operand = self.expr(PREFIX_POWER, depth + 1)?;
                Expr {
                    span: span.to(operand.span),
                    kind: ExprKind::Unary(UnOp::Not, Box::new(operand)),
                }
            }
            Tok::LParen => {
                let inner = self.expr(0, depth + 1)?;
                let close = self.expect(&Tok::RParen, "`)`")?;
                Expr {
                    kind: inner.kind,
                    span: span.to(close),
                }
            }
            Tok::If => self.if_expr(span, depth)?,
            Tok::Ident(name) => {
                if *self.peek() == Tok::LParen {
                    self.call(name, span, depth)?
                } else {
                    Expr {
                        kind: ExprKind::Name(name),
                        span,
                    }
                }
            }
            other => {
                return Err(ExprError::new(
                    format!("expected a value, found {}", describe(&other)),
                    span,
                ));
            }
        };
        self.postfix(expr)
    }

    fn if_expr(&mut self, span: Span, depth: usize) -> Result<Expr, ExprError> {
        let cond = self.expr(0, depth + 1)?;
        self.expect(&Tok::Then, "`then`")?;
        let then = self.expr(0, depth + 1)?;
        self.expect(&Tok::Else, "`else` (every `if` needs one)")?;
        let otherwise = self.expr(0, depth + 1)?;
        Ok(Expr {
            span: span.to(otherwise.span),
            kind: ExprKind::If(Box::new(cond), Box::new(then), Box::new(otherwise)),
        })
    }

    fn call(&mut self, name: String, span: Span, depth: usize) -> Result<Expr, ExprError> {
        self.bump();
        let mut args = Vec::new();
        if *self.peek() != Tok::RParen {
            loop {
                args.push(self.expr(0, depth + 1)?);
                if *self.peek() == Tok::Comma {
                    self.bump();
                } else {
                    break;
                }
            }
        }
        let close = self.expect(&Tok::RParen, "`,` or `)`")?;
        Ok(Expr {
            kind: ExprKind::Call(name, span, args),
            span: span.to(close),
        })
    }

    fn postfix(&mut self, mut expr: Expr) -> Result<Expr, ExprError> {
        while *self.peek() == Tok::Dot {
            self.bump();
            let (tok, span) = self.bump();
            let Tok::Ident(field) = tok else {
                return Err(ExprError::new(
                    format!("expected a field name after `.`, found {}", describe(&tok)),
                    span,
                ));
            };
            let whole = expr.span.to(span);
            expr = Expr {
                kind: ExprKind::Field(Box::new(expr), field, span),
                span: whole,
            };
        }
        Ok(expr)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn shape(e: &Expr) -> String {
        match &e.kind {
            ExprKind::Int {
                magnitude,
                negative,
                suffix,
            } => format!(
                "{}{magnitude}{}",
                if *negative { "-" } else { "" },
                suffix.map_or("", IntType::as_str)
            ),
            ExprKind::Bool(b) => b.to_string(),
            ExprKind::Str(s) => format!("{s:?}"),
            ExprKind::Address(a) => format!("@{}", a.to_standard_string()),
            ExprKind::Null => "null".into(),
            ExprKind::Name(n) => n.clone(),
            ExprKind::Field(e, f, _) => format!("{}.{f}", shape(e)),
            ExprKind::Unary(UnOp::Not, e) => format!("!{}", shape(e)),
            ExprKind::Unary(UnOp::Neg, e) => format!("-{}", shape(e)),
            ExprKind::Binary(op, a, b) => format!("({} {} {})", shape(a), op.symbol(), shape(b)),
            ExprKind::If(c, a, b) => {
                format!("(if {} then {} else {})", shape(c), shape(a), shape(b))
            }
            ExprKind::Call(f, _, args) => {
                format!(
                    "{f}({})",
                    args.iter().map(shape).collect::<Vec<_>>().join(", ")
                )
            }
        }
    }

    fn p(text: &str) -> String {
        shape(&parse(text).unwrap())
    }

    #[test]
    fn precedence_and_associativity() {
        assert_eq!(p("a + b * c - d"), "((a + (b * c)) - d)");
        assert_eq!(p("a || b && c == d"), "(a || (b && (c == d)))");
        assert_eq!(p("!a && -b < c"), "(!a && (-b < c))");
        assert_eq!(p("(a + b) * c"), "((a + b) * c)");
        assert_eq!(p("if a then b + 1 else c"), "(if a then (b + 1) else c)");
        assert_eq!(p("min(a, b.c.d) + 1_000u128"), "(min(a, b.c.d) + 1000u128)");
    }

    #[test]
    fn literals() {
        assert_eq!(p("-128i8"), "-128i8");
        assert_eq!(p("@0x1 == owner"), "(@0x1 == owner)");
        assert_eq!(p(r#"'it\'s' == "x""#), r#"("it's" == "x")"#);
        assert_eq!(p("null"), "null");
    }

    #[test]
    fn errors_point_at_the_problem() {
        let cases = [
            ("a < b < c", "comparisons can't be chained", 6),
            ("a = b", "`=` isn't an operator", 2),
            ("a and b", "`and` isn't an operator", 2),
            ("1u7", "unknown integer suffix `u7`", 1),
            ("0x1", "hex numbers aren't supported", 0),
            (
                "if a then b",
                "expected `else` (every `if` needs one), found the end of the expression",
                11,
            ),
            ("f(a b)", "expected `,` or `)`, found `b`", 4),
            (
                "a +",
                "expected a value, found the end of the expression",
                3,
            ),
            ("'open", "unterminated string", 0),
            ("a # b", "unexpected character `#`", 2),
        ];
        for (text, message, at) in cases {
            let e = parse(text).unwrap_err();
            assert_eq!((e.message.as_str(), e.span.start), (message, at), "{text}");
        }
    }

    #[test]
    fn hostile_nesting_is_rejected() {
        let deep = format!("{}1{}", "(".repeat(10_000), ")".repeat(10_000));
        assert_eq!(
            parse(&deep).unwrap_err().message,
            "expression is nested too deeply"
        );
        let unary = "!".repeat(10_000) + "a";
        assert!(parse(&unary).is_err());
    }
}
