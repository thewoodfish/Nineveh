//! Tokens to a [`Program`].
//!
//! Recursive descent, with a Pratt loop for expressions — the same choice, for the
//! same reason, as `nineveh-expr` (ADR 0007): the grammar is small and error quality
//! is the product. Parsing stops at the first problem; a file that doesn't parse has
//! no shape to keep checking against.

use nineveh_config::{ColumnType, Diagnostic, Span};

use crate::ast::{
    AssignOp, BinOp, ColumnDecl, Expr, ExprKind, Handler, Literal, Name, Program, RowRef, Stmt,
    TableDecl, UnOp,
};
use crate::lex::{Tok, Token, lex};

/// Nesting deeper than this is rejected rather than risking the stack. Hosted projects
/// compile configs from the control plane, so the text is untrusted.
const MAX_DEPTH: usize = 64;

pub(crate) fn parse(source: &str) -> Result<Program, Diagnostic> {
    let lexed = lex(source).map_err(|e| Diagnostic::new(e.message, Some(e.span)))?;
    let mut p = Parser {
        tokens: lexed.tokens,
        at: 0,
        depth: 0,
    };
    p.program()
}

struct Parser {
    tokens: Vec<Token>,
    at: usize,
    depth: usize,
}

impl Parser {
    fn peek(&self) -> &Tok {
        &self.tokens[self.at.min(self.tokens.len() - 1)].tok
    }

    fn peek_at(&self, ahead: usize) -> &Tok {
        &self.tokens[(self.at + ahead).min(self.tokens.len() - 1)].tok
    }

    fn span(&self) -> Span {
        self.tokens[self.at.min(self.tokens.len() - 1)].span
    }

    fn bump(&mut self) -> Token {
        let t = self.tokens[self.at.min(self.tokens.len() - 1)].clone();
        if self.at < self.tokens.len() - 1 {
            self.at += 1;
        }
        t
    }

    fn eat(&mut self, sym: &str) -> bool {
        if matches!(self.peek(), Tok::Sym(s) if *s == sym) {
            self.bump();
            true
        } else {
            false
        }
    }

    fn eat_word(&mut self, word: &str) -> bool {
        if matches!(self.peek(), Tok::Word(w) if w == word) {
            self.bump();
            true
        } else {
            false
        }
    }

    fn at_word(&self, word: &str) -> bool {
        matches!(self.peek(), Tok::Word(w) if w == word)
    }

    /// Whether the statement just read is over: a `;`, a closing `}`, end of file, or
    /// a line break before whatever comes next.
    fn statement_ended(&self) -> bool {
        let next = &self.tokens[self.at.min(self.tokens.len() - 1)];
        next.newline_before || matches!(next.tok, Tok::Sym(";" | "}") | Tok::Eof)
    }

    fn expect(&mut self, sym: &str) -> Result<Span, Diagnostic> {
        if matches!(self.peek(), Tok::Sym(s) if *s == sym) {
            Ok(self.bump().span)
        } else {
            Err(self.unexpected(&format!("`{sym}`")))
        }
    }

    fn unexpected(&self, wanted: &str) -> Diagnostic {
        Diagnostic::new(
            format!("expected {wanted}, found {}", self.peek().describe()),
            Some(self.span()),
        )
    }

    fn name(&mut self) -> Result<Name, Diagnostic> {
        match self.peek().clone() {
            Tok::Word(text) => {
                let span = self.bump().span;
                Ok(Name { text, span })
            }
            _ => Err(self.unexpected("a name")),
        }
    }

    /// A name that will become a Postgres identifier and a GraphQL field.
    fn declared_name(&mut self) -> Result<Name, Diagnostic> {
        let name = self.name()?;
        if !nineveh_config::Named::is_valid(&name.text) {
            return Err(Diagnostic::new(
                format!("`{}` isn't a usable name here", name.text),
                Some(name.span),
            )
            .help("names are lower case, start with a letter, and hold letters, digits and `_`"));
        }
        Ok(name)
    }

    fn deeper(&mut self) -> Result<(), Diagnostic> {
        self.depth += 1;
        if self.depth > MAX_DEPTH {
            return Err(Diagnostic::new(
                "this is nested too deeply to compile".to_owned(),
                Some(self.span()),
            ));
        }
        Ok(())
    }

    // ---- top level ----

    fn program(&mut self) -> Result<Program, Diagnostic> {
        let mut program = Program::default();
        loop {
            self.eat(";");
            match self.peek().clone() {
                Tok::Eof => break,
                // `import { on, table } from "nineveh"` — accepted so the file is real
                // TypeScript for the editor, and ignored: there is nothing to import.
                Tok::Word(w) if w == "import" => self.skip_import()?,
                Tok::Word(w) if w == "export" || w == "const" => {
                    program.tables.push(self.table_decl()?);
                }
                Tok::Word(w) if w == "on" => program.handlers.push(self.handler()?),
                _ => {
                    return Err(self.unexpected("`on`, `const` or `export const`"));
                }
            }
        }
        Ok(program)
    }

    fn skip_import(&mut self) -> Result<(), Diagnostic> {
        self.bump();
        while !matches!(self.peek(), Tok::Eof) {
            if matches!(self.peek(), Tok::Str(_)) {
                self.bump();
                self.eat(";");
                return Ok(());
            }
            self.bump();
        }
        Err(self.unexpected("a module name"))
    }

    /// `export const balances = table({ key: { … }, columns: { … } })`
    fn table_decl(&mut self) -> Result<TableDecl, Diagnostic> {
        self.eat_word("export");
        if !self.eat_word("const") {
            return Err(self.unexpected("`const`"));
        }
        let name = self.declared_name()?;
        self.expect("=")?;
        if !self.eat_word("table") {
            return Err(self.unexpected("`table(…)`"));
        }
        self.expect("(")?;
        self.expect("{")?;

        let mut key = Vec::new();
        let mut columns = Vec::new();
        let mut seen_key = false;
        let mut seen_columns = false;
        while !self.eat("}") {
            let field = self.name()?;
            self.expect(":")?;
            match field.text.as_str() {
                "key" => {
                    seen_key = true;
                    key = self.column_block()?;
                }
                "columns" => {
                    seen_columns = true;
                    columns = self.column_block()?;
                }
                other => {
                    return Err(Diagnostic::new(
                        format!("a table has no `{other}`"),
                        Some(field.span),
                    )
                    .help("a table takes `key` and `columns`"));
                }
            }
            if !self.eat(",") && !matches!(self.peek(), Tok::Sym("}")) {
                return Err(self.unexpected("`,` or `}`"));
            }
        }
        self.expect(")")?;
        self.eat(";");

        if !seen_key {
            return Err(Diagnostic::new(
                format!("table `{}` has no `key`", name.text),
                Some(name.span),
            )
            .help("every reduce table names the columns that identify a row"));
        }
        if !seen_columns {
            return Err(Diagnostic::new(
                format!("table `{}` has no `columns`", name.text),
                Some(name.span),
            )
            .help("write `columns: {}` if the key is the whole row"));
        }
        Ok(TableDecl { name, key, columns })
    }

    /// `{ user: address, balance: u128.default(0), memo: string.nullable() }`
    fn column_block(&mut self) -> Result<Vec<ColumnDecl>, Diagnostic> {
        self.expect("{")?;
        let mut out: Vec<ColumnDecl> = Vec::new();
        while !self.eat("}") {
            let name = self.declared_name()?;
            self.expect(":")?;
            let ty_name = self.name()?;
            let Some(ty) = column_type(&ty_name.text) else {
                return Err(Diagnostic::new(
                    format!("`{}` isn't a column type", ty_name.text),
                    Some(ty_name.span),
                )
                .did_you_mean(&ty_name.text, ColumnType::ALL.iter().map(|t| t.as_str())));
            };
            let mut column = ColumnDecl {
                name,
                ty,
                ty_span: ty_name.span,
                nullable: false,
                default: None,
            };
            // `.default(…)` and `.nullable()`, in any order.
            while self.eat(".") {
                let modifier = self.name()?;
                match modifier.text.as_str() {
                    "default" => {
                        self.expect("(")?;
                        column.default = Some(self.literal()?);
                        self.expect(")")?;
                    }
                    "nullable" => {
                        self.expect("(")?;
                        self.expect(")")?;
                        column.nullable = true;
                    }
                    other => {
                        return Err(Diagnostic::new(
                            format!("a column has no `{other}`"),
                            Some(modifier.span),
                        )
                        .help("a column takes `.default(…)` and `.nullable()`"));
                    }
                }
            }
            if out.iter().any(|c| c.name.text == column.name.text) {
                return Err(Diagnostic::new(
                    format!("`{}` is declared twice", column.name.text),
                    Some(column.name.span),
                ));
            }
            out.push(column);
            if !self.eat(",") && !matches!(self.peek(), Tok::Sym("}")) {
                return Err(self.unexpected("`,` or `}`"));
            }
        }
        Ok(out)
    }

    fn literal(&mut self) -> Result<Literal, Diagnostic> {
        let negative = self.eat("-");
        match self.peek().clone() {
            Tok::Int(digits) => {
                self.bump();
                Ok(Literal::Int { digits, negative })
            }
            Tok::Str(s) if !negative => {
                self.bump();
                Ok(Literal::Str(s))
            }
            Tok::Word(w) if !negative && (w == "true" || w == "false") => {
                self.bump();
                Ok(Literal::Bool(w == "true"))
            }
            _ => Err(self.unexpected("a number, string or boolean")),
        }
    }

    /// `on(deposits, (d) => { … })`, `on(vaults.deleted, (v) => { … })`
    fn handler(&mut self) -> Result<Handler, Diagnostic> {
        self.bump(); // `on`
        self.expect("(")?;
        let source = self.name()?;
        let deleted = if self.eat(".") {
            let what = self.name()?;
            if what.text != "deleted" {
                return Err(Diagnostic::new(
                    format!("a source has no `{}`", what.text),
                    Some(what.span),
                )
                .help("write `<source>.deleted` for a source's deletes"));
            }
            true
        } else {
            false
        };
        self.expect(",")?;
        self.expect("(")?;
        let param = self.name()?;
        // An optional type annotation, so the file can be checked by `tsc`.
        if self.eat(":") {
            self.name()?;
        }
        self.expect(")")?;
        self.expect("=>")?;
        let body = self.block()?;
        self.expect(")")?;
        self.eat(";");
        Ok(Handler {
            source,
            deleted,
            param,
            body,
        })
    }

    // ---- statements ----

    fn block(&mut self) -> Result<Vec<Stmt>, Diagnostic> {
        self.deeper()?;
        self.expect("{")?;
        let mut out = Vec::new();
        while !self.eat("}") {
            if matches!(self.peek(), Tok::Eof) {
                return Err(self.unexpected("`}`"));
            }
            out.push(self.stmt()?);
            self.eat(";");
        }
        self.depth -= 1;
        Ok(out)
    }

    /// A braced block, or the single statement a guard clause is usually written as:
    /// `if (w.amount == 0) return`.
    fn body(&mut self) -> Result<Vec<Stmt>, Diagnostic> {
        if matches!(self.peek(), Tok::Sym("{")) {
            return self.block();
        }
        let stmt = self.stmt()?;
        self.eat(";");
        Ok(vec![stmt])
    }

    fn stmt(&mut self) -> Result<Stmt, Diagnostic> {
        if self.at_word("const") {
            return self.const_stmt();
        }
        if self.at_word("let") || self.at_word("var") {
            let span = self.span();
            let word = self.name()?.text;
            return Err(Diagnostic::new(
                format!("`{word}` isn't part of this language"),
                Some(span),
            )
            .help("use `const`: a rule computes values, it doesn't keep any"));
        }
        if self.at_word("if") {
            return self.if_stmt();
        }
        if self.at_word("return") {
            let span = self.bump().span;
            // `return` never takes a value, but without a semicolon the next statement
            // would look like one. A line break ends it, as it does in JavaScript.
            if !self.statement_ended() {
                return Err(Diagnostic::new(
                    "`return` doesn't take a value here".to_owned(),
                    Some(self.span()),
                )
                .help("a handler doesn't return anything; `return` just stops it"));
            }
            return Ok(Stmt::Return { span });
        }
        self.write_stmt()
    }

    fn const_stmt(&mut self) -> Result<Stmt, Diagnostic> {
        self.bump(); // `const`
        let name = self.name()?;
        if self.eat(":") {
            self.name()?;
        }
        self.expect("=")?;
        // `const b = balances.row(…)` names a row; anything else names a value.
        if let (Tok::Word(table), Tok::Sym("."), Tok::Word(method), Tok::Sym("(")) = (
            self.peek().clone(),
            self.peek_at(1).clone(),
            self.peek_at(2).clone(),
            self.peek_at(3).clone(),
        ) && method == "row"
        {
            let table_span = self.span();
            self.bump();
            self.bump();
            self.bump();
            let keys = self.args()?;
            return Ok(Stmt::Row {
                name,
                table: Name {
                    text: table,
                    span: table_span,
                },
                keys,
            });
        }
        let value = self.expr()?;
        Ok(Stmt::Let { name, value })
    }

    fn if_stmt(&mut self) -> Result<Stmt, Diagnostic> {
        self.bump(); // `if`
        self.expect("(")?;
        let cond = self.expr()?;
        self.expect(")")?;
        let then = self.body()?;
        let otherwise = if self.eat_word("else") {
            if self.at_word("if") {
                vec![self.if_stmt()?]
            } else {
                self.body()?
            }
        } else {
            Vec::new()
        };
        Ok(Stmt::If {
            cond,
            then,
            otherwise,
        })
    }

    /// `b.balance += x`, `balances.row(k).balance = x`, `b.delete()`.
    fn write_stmt(&mut self) -> Result<Stmt, Diagnostic> {
        let head = self.name()?;
        let start = head.span;
        let row = if matches!(self.peek(), Tok::Sym("."))
            && matches!(self.peek_at(1), Tok::Word(w) if w == "row")
        {
            self.bump();
            self.bump();
            let keys = self.args()?;
            let end = self.tokens[self.at.saturating_sub(1)].span;
            RowRef::Inline {
                table: head,
                keys,
                span: Span::new(start.offset, end.offset + end.len - start.offset),
            }
        } else {
            RowRef::Bound(head)
        };
        self.expect(".")?;
        let field = self.name()?;
        if field.text == "delete" && matches!(self.peek(), Tok::Sym("(")) {
            self.bump();
            let end = self.expect(")")?;
            return Ok(Stmt::Delete {
                span: Span::new(start.offset, end.offset + end.len - start.offset),
                row,
            });
        }
        let op = if self.eat("=") {
            AssignOp::Set
        } else if self.eat("+=") {
            AssignOp::Add
        } else if self.eat("-=") {
            AssignOp::Sub
        } else {
            return Err(self.unexpected("`=`, `+=`, `-=` or `.delete()`"));
        };
        let value = self.expr()?;
        Ok(Stmt::Assign {
            row,
            column: field,
            op,
            value,
        })
    }

    // ---- expressions ----

    fn args(&mut self) -> Result<Vec<Expr>, Diagnostic> {
        self.expect("(")?;
        let mut out = Vec::new();
        while !self.eat(")") {
            out.push(self.expr()?);
            if !self.eat(",") && !matches!(self.peek(), Tok::Sym(")")) {
                return Err(self.unexpected("`,` or `)`"));
            }
        }
        Ok(out)
    }

    fn expr(&mut self) -> Result<Expr, Diagnostic> {
        self.deeper()?;
        let e = self.ternary()?;
        self.depth -= 1;
        Ok(e)
    }

    fn ternary(&mut self) -> Result<Expr, Diagnostic> {
        let cond = self.binary(0)?;
        if self.eat("?") {
            let then = self.expr()?;
            self.expect(":")?;
            let otherwise = self.ternary()?;
            let span = join(cond.span, otherwise.span);
            return Ok(Expr {
                kind: ExprKind::Ternary(Box::new(cond), Box::new(then), Box::new(otherwise)),
                span,
            });
        }
        Ok(cond)
    }

    fn binary(&mut self, min_power: u8) -> Result<Expr, Diagnostic> {
        let mut left = self.unary()?;
        loop {
            // `??` sits below every other operator, as it does in JavaScript.
            if min_power <= 1 && matches!(self.peek(), Tok::Sym("??")) {
                self.bump();
                let right = self.binary(2)?;
                let span = join(left.span, right.span);
                left = Expr {
                    kind: ExprKind::Coalesce(Box::new(left), Box::new(right)),
                    span,
                };
                continue;
            }
            let Some(op) = self.peek_binop() else { break };
            if op.power() < min_power.max(2) {
                break;
            }
            self.bump();
            let right = self.binary(op.power() + 1)?;
            // Comparisons don't chain, matching `nineveh-expr`.
            if op.power() == BinOp::Eq.power()
                && let Some(next) = self.peek_binop()
                && next.power() == op.power()
            {
                return Err(Diagnostic::new(
                    format!("`{}` doesn't chain", next.text()),
                    Some(self.span()),
                )
                .help("write the two comparisons separately, joined with `&&`"));
            }
            let span = join(left.span, right.span);
            left = Expr {
                kind: ExprKind::Binary(op, Box::new(left), Box::new(right)),
                span,
            };
        }
        Ok(left)
    }

    fn peek_binop(&self) -> Option<BinOp> {
        let Tok::Sym(s) = self.peek() else {
            return None;
        };
        Some(match *s {
            "||" => BinOp::Or,
            "&&" => BinOp::And,
            "==" | "===" => BinOp::Eq,
            "!=" | "!==" => BinOp::Ne,
            "<" => BinOp::Lt,
            "<=" => BinOp::Le,
            ">" => BinOp::Gt,
            ">=" => BinOp::Ge,
            "+" => BinOp::Add,
            "-" => BinOp::Sub,
            "*" => BinOp::Mul,
            "/" => BinOp::Div,
            "%" => BinOp::Rem,
            _ => return None,
        })
    }

    fn unary(&mut self) -> Result<Expr, Diagnostic> {
        for (sym, op) in [("!", UnOp::Not), ("-", UnOp::Neg)] {
            if matches!(self.peek(), Tok::Sym(s) if *s == sym) {
                let start = self.bump().span;
                let inner = self.unary()?;
                let span = join(start, inner.span);
                return Ok(Expr {
                    kind: ExprKind::Unary(op, Box::new(inner)),
                    span,
                });
            }
        }
        self.postfix()
    }

    fn postfix(&mut self) -> Result<Expr, Diagnostic> {
        let mut e = self.primary()?;
        loop {
            if matches!(self.peek(), Tok::Sym("?.")) {
                self.bump();
                let field = self.name()?;
                let span = join(e.span, field.span);
                e = Expr {
                    kind: ExprKind::OptField(Box::new(e), field),
                    span,
                };
                continue;
            }
            if matches!(self.peek(), Tok::Sym(".")) {
                self.bump();
                let field = self.name()?;
                // `t.get(k)` reads another table's row; `t.row(k)` names one to write.
                if matches!(self.peek(), Tok::Sym("(")) {
                    if field.text == "row" {
                        return Err(Diagnostic::new(
                            "a row isn't a value".to_owned(),
                            Some(join(e.span, field.span)),
                        )
                        .help("`.row(…)` names a row to write; to read one, use `.get(…)`"));
                    }
                    if field.text == "get" {
                        let ExprKind::Name(table) = &e.kind else {
                            return Err(Diagnostic::new(
                                "only a table has rows to get".to_owned(),
                                Some(e.span),
                            ));
                        };
                        let table = Name {
                            text: table.clone(),
                            span: e.span,
                        };
                        let keys = self.args()?;
                        let end = self.tokens[self.at.saturating_sub(1)].span;
                        e = Expr {
                            kind: ExprKind::Get { table, keys },
                            span: join(e.span, end),
                        };
                        continue;
                    }
                    return Err(Diagnostic::new(
                        format!("`{}` isn't something you can call", field.text),
                        Some(field.span),
                    ));
                }
                let span = join(e.span, field.span);
                e = Expr {
                    kind: ExprKind::Field(Box::new(e), field),
                    span,
                };
                continue;
            }
            // `u128(x)`, `min(a, b)`: a call on a bare name.
            if matches!(self.peek(), Tok::Sym("(")) {
                let ExprKind::Name(callee) = &e.kind else {
                    return Err(Diagnostic::new(
                        "this isn't something you can call".to_owned(),
                        Some(e.span),
                    ));
                };
                let callee = Name {
                    text: callee.clone(),
                    span: e.span,
                };
                let args = self.args()?;
                let end = self.tokens[self.at.saturating_sub(1)].span;
                e = Expr {
                    kind: ExprKind::Call(callee, args),
                    span: join(e.span, end),
                };
                continue;
            }
            break;
        }
        Ok(e)
    }

    fn primary(&mut self) -> Result<Expr, Diagnostic> {
        let span = self.span();
        match self.peek().clone() {
            Tok::Int(digits) => {
                self.bump();
                Ok(Expr {
                    kind: ExprKind::Int(digits),
                    span,
                })
            }
            Tok::Str(s) => {
                self.bump();
                Ok(Expr {
                    kind: ExprKind::Str(s),
                    span,
                })
            }
            Tok::Word(w) => {
                self.bump();
                let kind = match w.as_str() {
                    "true" => ExprKind::Bool(true),
                    "false" => ExprKind::Bool(false),
                    "null" | "undefined" => ExprKind::Null,
                    _ => ExprKind::Name(w),
                };
                Ok(Expr { kind, span })
            }
            Tok::Sym("(") => {
                self.bump();
                let inner = self.expr()?;
                self.expect(")")?;
                Ok(inner)
            }
            _ => Err(self.unexpected("a value")),
        }
    }
}

fn join(a: Span, b: Span) -> Span {
    let start = a.offset.min(b.offset);
    let end = (a.offset + a.len).max(b.offset + b.len);
    Span::new(start, end - start)
}

fn column_type(name: &str) -> Option<ColumnType> {
    ColumnType::ALL.into_iter().find(|t| t.as_str() == name)
}
