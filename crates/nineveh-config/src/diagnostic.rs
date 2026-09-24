//! Located, human-readable problems with a project config.
//!
//! Non-Rust teams write these configs, so every error points at the exact YAML text it's
//! about and, where it can, suggests the fix. Rendering follows rustc's layout because
//! it's familiar and survives copy-paste into issues and chat.

use std::fmt::{self, Write as _};

/// A region of a config source, in bytes.
///
/// A project can be written across two files — `nineveh.yaml` and the `reducers:` file
/// it names (ADR 0025) — so a span says which one it's in. File 0 is the YAML; a
/// frontend that reads another file numbers it from 1 and passes the sources to
/// [`Diagnostics::render_files`] in the same order.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub struct Span {
    pub offset: usize,
    pub len: usize,
    pub file: u16,
}

impl Span {
    #[must_use]
    pub const fn new(offset: usize, len: usize) -> Self {
        Self {
            offset,
            len,
            file: 0,
        }
    }

    /// The same region, in another of the project's files.
    #[must_use]
    pub const fn in_file(self, file: u16) -> Self {
        Self { file, ..self }
    }

    pub(crate) fn from_location(location: &serde_saphyr::Location) -> Option<Self> {
        let span = location.span();
        let offset = usize::try_from(span.byte_offset()?).ok()?;
        let len = span
            .byte_len()
            .and_then(|l| usize::try_from(l).ok())
            .unwrap_or(0);
        Some(Self::new(offset, len))
    }
}

/// One problem with a config.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Diagnostic {
    pub message: String,
    /// The YAML the problem is about, if it has a place in the file.
    pub span: Option<Span>,
    /// A suggested fix.
    pub help: Option<String>,
}

impl Diagnostic {
    #[must_use]
    pub fn new(message: impl Into<String>, span: Option<Span>) -> Self {
        Self {
            message: message.into(),
            span,
            help: None,
        }
    }

    #[must_use]
    pub fn help(mut self, help: impl Into<String>) -> Self {
        self.help = Some(help.into());
        self
    }

    /// Add `help` unless a suggestion was already made.
    #[must_use]
    pub fn help_if_none(self, help: impl Into<String>) -> Self {
        if self.help.is_some() {
            self
        } else {
            self.help(help)
        }
    }

    /// Suggest the closest of `candidates` to `name`, if one is close enough.
    #[must_use]
    pub fn did_you_mean<'a>(
        self,
        name: &str,
        candidates: impl IntoIterator<Item = &'a str>,
    ) -> Self {
        match closest(name, candidates) {
            Some(best) => self.help(format!("did you mean `{best}`?")),
            None => self,
        }
    }
}

/// Every problem found in a config. Never empty.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Diagnostics(Vec<Diagnostic>);

impl Diagnostics {
    #[must_use]
    pub fn from_vec(diagnostics: Vec<Diagnostic>) -> Option<Self> {
        (!diagnostics.is_empty()).then_some(Self(diagnostics))
    }

    #[must_use]
    pub fn single(diagnostic: Diagnostic) -> Self {
        Self(vec![diagnostic])
    }

    #[must_use]
    pub fn as_slice(&self) -> &[Diagnostic] {
        &self.0
    }

    /// Render every diagnostic against the source it came from, rustc style:
    ///
    /// ```text
    /// error: unknown source `deposit`
    ///   --> nineveh.yaml:14:15
    ///    |
    /// 14 |       - { on: deposit, set: { balance: "balance + amount" } }
    ///    |               ^^^^^^^
    ///    = help: did you mean `deposits`?
    /// ```
    #[must_use]
    pub fn render(&self, file_name: &str, source: &str) -> String {
        self.render_files(&[(file_name, source)])
    }

    /// Render every diagnostic against the file its span names, for a project written
    /// across more than one: `nineveh.yaml` first, then each `reducers:` file.
    #[must_use]
    pub fn render_files(&self, files: &[(&str, &str)]) -> String {
        let mut out = String::new();
        for (i, d) in self.0.iter().enumerate() {
            if i > 0 {
                out.push('\n');
            }
            let (name, source) = d
                .span
                .and_then(|s| files.get(usize::from(s.file)))
                .or_else(|| files.first())
                .copied()
                .unwrap_or(("nineveh.yaml", ""));
            render_one(&mut out, d, name, source);
        }
        out
    }
}

impl fmt::Display for Diagnostics {
    /// Messages only, one per line. Use [`Diagnostics::render`] to show source context.
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        for (i, d) in self.0.iter().enumerate() {
            if i > 0 {
                f.write_str("\n")?;
            }
            write!(f, "error: {}", d.message)?;
        }
        Ok(())
    }
}

impl std::error::Error for Diagnostics {}

fn render_one(out: &mut String, d: &Diagnostic, file_name: &str, source: &str) {
    let _ = writeln!(out, "error: {}", d.message);
    let Some(span) = d.span.filter(|s| source.is_char_boundary(s.offset)) else {
        let _ = writeln!(out, " --> {file_name}");
        if let Some(help) = &d.help {
            let _ = writeln!(out, "  = help: {help}");
        }
        return;
    };

    let line_start = source[..span.offset].rfind('\n').map_or(0, |i| i + 1);
    let line_end = source[span.offset..]
        .find('\n')
        .map_or(source.len(), |i| span.offset + i);
    let line_text = source[line_start..line_end].trim_end_matches('\r');
    let line_no = source[..span.offset].matches('\n').count() + 1;
    let column = source[line_start..span.offset].chars().count() + 1;

    // Underline the span, clipped to its first line; at least one caret.
    let mut span_end = (span.offset + span.len).min(line_start + line_text.len());
    while !source.is_char_boundary(span_end) {
        span_end -= 1;
    }
    let carets = source
        .get(span.offset..span_end)
        .map_or(1, |s| s.chars().count())
        .max(1);

    let gutter = " ".repeat(line_no.to_string().len());
    let _ = writeln!(out, "{gutter}--> {file_name}:{line_no}:{column}");
    let _ = writeln!(out, "{gutter} |");
    let _ = writeln!(out, "{line_no} | {line_text}");
    let _ = writeln!(
        out,
        "{gutter} | {}{}",
        " ".repeat(column - 1),
        "^".repeat(carets)
    );
    if let Some(help) = &d.help {
        let _ = writeln!(out, "{gutter} = help: {help}");
    }
}

/// The candidate closest to `name` by edit distance, if it's plausibly a typo.
fn closest<'a>(name: &str, candidates: impl IntoIterator<Item = &'a str>) -> Option<&'a str> {
    let limit = (name.chars().count() / 3).max(1);
    candidates
        .into_iter()
        .map(|c| (edit_distance(name, c), c))
        .filter(|&(d, _)| d <= limit)
        .min_by_key(|&(d, _)| d)
        .map(|(_, c)| c)
}

/// Edit distance counting insertions, deletions, substitutions and swaps of adjacent
/// characters (optimal string alignment), so `mni` is one edit from `min`.
fn edit_distance(a: &str, b: &str) -> usize {
    let a: Vec<char> = a.chars().collect();
    let b: Vec<char> = b.chars().collect();
    let width = b.len() + 1;
    let mut d = vec![0; (a.len() + 1) * width];
    for i in 0..=a.len() {
        d[i * width] = i;
    }
    for (j, cell) in d.iter_mut().take(width).enumerate() {
        *cell = j;
    }
    for i in 1..=a.len() {
        for j in 1..=b.len() {
            let cost = usize::from(a[i - 1] != b[j - 1]);
            let mut best = (d[(i - 1) * width + j] + 1)
                .min(d[i * width + j - 1] + 1)
                .min(d[(i - 1) * width + j - 1] + cost);
            if i > 1 && j > 1 && a[i - 1] == b[j - 2] && a[i - 2] == b[j - 1] {
                best = best.min(d[(i - 2) * width + j - 2] + 1);
            }
            d[i * width + j] = best;
        }
    }
    d[a.len() * width + b.len()]
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn renders_the_line_and_underlines_the_span() {
        let source = "name: vault\nstate:\n  balances:\n    mirror: deposit\n";
        let offset = source.find("deposit").unwrap();
        let d = Diagnostic::new("unknown source `deposit`", Some(Span::new(offset, 7)))
            .did_you_mean("deposit", ["deposits", "vaults"]);
        let rendered = Diagnostics::single(d).render("nineveh.yaml", source);
        assert_eq!(
            rendered,
            "error: unknown source `deposit`\n\
             \x20--> nineveh.yaml:4:13\n\
             \x20 |\n\
             4 |     mirror: deposit\n\
             \x20 |             ^^^^^^^\n\
             \x20 = help: did you mean `deposits`?\n"
        );
    }

    #[test]
    fn suggestions_only_for_plausible_typos() {
        assert_eq!(closest("colums", ["columns", "key"]), Some("columns"));
        assert_eq!(closest("xyz", ["columns", "key"]), None);
        assert_eq!(edit_distance("kitten", "sitting"), 3);
        assert_eq!(closest("mian", ["main", "mainnet"]), Some("main"));
        assert_eq!(edit_distance("mni", "min"), 1);
    }

    #[test]
    fn spans_past_the_end_render_without_a_snippet() {
        let d = Diagnostic::new("oops", Some(Span::new(999, 1)));
        let rendered = Diagnostics::single(d).render("nineveh.yaml", "a: 1\n");
        assert_eq!(rendered, "error: oops\n --> nineveh.yaml\n");
    }
}
