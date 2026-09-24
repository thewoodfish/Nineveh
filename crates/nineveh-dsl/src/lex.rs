//! Source text to tokens.
//!
//! The DSL is written in a `.nineveh.ts` file, so the lexer accepts what TypeScript
//! accepts for the forms the grammar uses: `//` and `/* */` comments, single- and
//! double-quoted strings, and `_` separators in numbers. Semicolons are optional —
//! statement ends are unambiguous without them, and a JS developer writes both ways.

use nineveh_config::Span;

#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) enum Tok {
    /// A bare word: a keyword, a name, or a type like `u128`.
    Word(String),
    Int(String),
    Str(String),
    /// Punctuation and operators, interned as static text so matching is by `==`.
    Sym(&'static str),
    Eof,
}

impl Tok {
    /// How the token is written, for "expected X, found Y".
    pub(crate) fn describe(&self) -> String {
        match self {
            Self::Word(w) => format!("`{w}`"),
            Self::Int(i) => format!("`{i}`"),
            Self::Str(_) => "a string".to_owned(),
            Self::Sym(s) => format!("`{s}`"),
            Self::Eof => "end of file".to_owned(),
        }
    }
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) struct Token {
    pub(crate) tok: Tok,
    pub(crate) span: Span,
    /// Whether a line break came before this token. Semicolons are optional, so a
    /// newline is what ends a statement — the same rule JavaScript uses.
    pub(crate) newline_before: bool,
}

/// Every operator and punctuator, longest first so `>=` never lexes as `>` then `=`.
const SYMBOLS: &[&str] = &[
    "?.", "??", "===", "!==", "==", "!=", "<=", ">=", "&&", "||", "+=", "-=", "=>", "(", ")", "{",
    "}", "[", "]", ",", ".", ":", ";", "?", "=", "+", "-", "*", "/", "%", "<", ">", "!",
];

pub(crate) struct Lexed {
    pub(crate) tokens: Vec<Token>,
}

/// An unrecognised character or an unterminated string, located.
pub(crate) struct LexError {
    pub(crate) message: String,
    pub(crate) span: Span,
}

pub(crate) fn lex(source: &str) -> Result<Lexed, LexError> {
    let bytes = source.as_bytes();
    let mut tokens = Vec::new();
    let mut i = 0;
    let mut newline = false;
    while i < bytes.len() {
        let b = bytes[i];
        // Whitespace.
        if b.is_ascii_whitespace() {
            newline |= b == b'\n';
            i += 1;
            continue;
        }
        // Comments.
        if b == b'/' && bytes.get(i + 1) == Some(&b'/') {
            i = source[i..].find('\n').map_or(bytes.len(), |n| i + n);
            continue;
        }

        if b == b'/' && bytes.get(i + 1) == Some(&b'*') {
            let end = source[i + 2..]
                .find("*/")
                .map_or_else(|| bytes.len(), |n| i + 2 + n + 2);
            i = end;
            continue;
        }
        let start = i;
        // Words: identifiers, keywords, type names.
        if b.is_ascii_alphabetic() || b == b'_' || b == b'$' {
            while i < bytes.len()
                && (bytes[i].is_ascii_alphanumeric() || bytes[i] == b'_' || bytes[i] == b'$')
            {
                i += 1;
            }
            push(
                &mut tokens,
                Tok::Word(source[start..i].to_owned()),
                start,
                i,
                &mut newline,
            );
            continue;
        }
        // Numbers: decimal or `0x` hex, with `_` separators.
        if b.is_ascii_digit() {
            i = scan_number(bytes, i);
            push(
                &mut tokens,
                Tok::Int(source[start..i].to_owned()),
                start,
                i,
                &mut newline,
            );
            continue;
        }
        // Strings. No escapes: the values here are addresses, type names and short
        // labels, and a backslash in one is far more likely a mistake than intent.
        if b == b'"' || b == b'\'' {
            let end = scan_string(bytes, i)?;
            let text = source[start + 1..end - 1].to_owned();
            i = end;
            push(&mut tokens, Tok::Str(text), start, i, &mut newline);
            continue;
        }
        // Operators and punctuation.
        if let Some(sym) = SYMBOLS.iter().find(|s| source[i..].starts_with(**s)) {
            i += sym.len();
            push(&mut tokens, Tok::Sym(sym), start, i, &mut newline);
            continue;
        }
        let ch = source[i..].chars().next().unwrap_or('?');
        return Err(LexError {
            message: format!("`{ch}` doesn't mean anything here"),
            span: crate::span(start, ch.len_utf8()),
        });
    }
    push(
        &mut tokens,
        Tok::Eof,
        source.len(),
        source.len(),
        &mut newline,
    );
    Ok(Lexed { tokens })
}

/// Where the number starting at `from` ends.
fn scan_number(bytes: &[u8], from: usize) -> usize {
    let mut i = from;
    let hex = bytes[i] == b'0' && matches!(bytes.get(i + 1), Some(b'x' | b'X'));
    if hex {
        i += 2;
    }
    while i < bytes.len()
        && (bytes[i] == b'_'
            || if hex {
                bytes[i].is_ascii_hexdigit()
            } else {
                bytes[i].is_ascii_digit()
            })
    {
        i += 1;
    }
    i
}

/// Where the quoted string starting at `from` ends, past its closing quote.
fn scan_string(bytes: &[u8], from: usize) -> Result<usize, LexError> {
    let quote = bytes[from];
    let mut i = from + 1;
    while i < bytes.len() && bytes[i] != quote && bytes[i] != b'\n' {
        i += 1;
    }
    if i >= bytes.len() || bytes[i] != quote {
        return Err(LexError {
            message: "this string isn't closed".to_owned(),
            span: Span::new(from, 1),
        });
    }
    Ok(i + 1)
}

fn push(tokens: &mut Vec<Token>, tok: Tok, start: usize, end: usize, newline: &mut bool) {
    tokens.push(Token {
        tok,
        span: crate::span(start, end - start),
        newline_before: *newline,
    });
    *newline = false;
}
