//! The reducers in the published docs compile.
//!
//! Documentation that doesn't work is worse than none: a reader trusts it, spends an
//! afternoon, and concludes the tool is broken. Every complete example on these pages is
//! extracted from the markdown and put through the real compiler, so an example can't
//! rot while the page still claims it works.
//!
//! `first-backend.md` is here as well as `reducers.md` because a reader types its two
//! blocks into Studio and expects them to save, and one of them is what Studio itself
//! generates: if that stops compiling, the template that writes it is broken too.
//!
//! Fragments — a few lines showing one idea — are skipped: they aren't meant to stand
//! alone. An example counts as complete when it declares a table or handles a source.

#![allow(clippy::panic, reason = "a failing example is reported with its text")]

use std::fs;
use std::path::Path;

use nineveh_dsl::{Context, SourceInfo, TableInfo, compile};

/// Everything the page's examples refer to but don't declare themselves.
fn ctx() -> Context {
    let event = |name: &str| SourceInfo {
        name: name.to_owned(),
        has_deletes: false,
    };
    Context {
        sources: vec![
            event("deposits"),
            event("withdrawals"),
            event("trades"),
            event("prices"),
            event("opened"),
            event("closed"),
            event("sold"),
            SourceInfo {
                name: "vaults".into(),
                has_deletes: true,
            },
        ],
        tables: vec![
            // Tables an example reads or writes without declaring: they stand for
            // ones the reader already has.
            TableInfo {
                name: "markets".into(),
                key_arity: None,
                is_log: false,
            },
        ],
    }
}

/// The fenced `ts` blocks of a markdown file, in order.
fn examples(markdown: &str) -> Vec<String> {
    let mut out = Vec::new();
    let mut block: Option<String> = None;
    for line in markdown.lines() {
        match (&mut block, line.trim_end()) {
            (None, "```ts") => block = Some(String::new()),
            (Some(_), "```") => out.push(block.take().unwrap_or_default()),
            (Some(text), line) => {
                text.push_str(line);
                text.push('\n');
            }
            _ => {}
        }
    }
    out
}

/// Every page whose `ts` blocks are reducers, and how many of them to expect.
const PAGES: &[(&str, usize)] = &[("docs/reducers.md", 6), ("docs/first-backend.md", 2)];

#[test]
fn every_complete_example_compiles() {
    for (page, least) in PAGES {
        let path = Path::new(env!("CARGO_MANIFEST_DIR"))
            .join("../..")
            .join(page);
        let markdown =
            fs::read_to_string(&path).unwrap_or_else(|e| panic!("{}: {e}", path.display()));

        let mut checked = 0;
        for (i, example) in examples(&markdown).into_iter().enumerate() {
            // A fragment shows one line of a larger thing; only whole ones are compiled.
            let complete = example.contains("table({") || example.contains("on(");
            // The one with `…` in it is deliberately a sketch of the shape.
            if !complete || example.contains('…') {
                continue;
            }
            // An example that only handles records leans on tables the reader declared
            // earlier on the page; give it the ones the page declares.
            let source = if example.contains("table({") {
                example.clone()
            } else {
                format!("{}\n{example}", preamble(&markdown))
            };
            if let Err(d) = compile(&source, &ctx()) {
                panic!(
                    "{page} example {i} doesn't compile:\n\n{example}\n{}",
                    d.render(page, &source)
                );
            }
            checked += 1;
        }
        assert!(
            checked >= *least,
            "expected at least {least} examples from {page} to be compiled, got {checked}"
        );
    }
}

/// Every table the page declares, so a handler-only example has something to write.
///
/// A page teaches by showing the same table more than once, so declarations are kept
/// by name: the first one wins, and the rest would only be a duplicate.
fn preamble(markdown: &str) -> String {
    let mut seen: Vec<String> = Vec::new();
    let mut out = String::new();
    for example in examples(markdown) {
        if !example.contains("table({") || example.contains('…') {
            continue;
        }
        // Keep the declaration, drop its handlers: those are compiled on their own turn.
        let declaration = example.split("\non(").next().unwrap_or_default();
        let Some(name) = declaration
            .split_once("export const ")
            .and_then(|(_, rest)| rest.split_whitespace().next())
        else {
            continue;
        };
        if seen.iter().any(|s| s == name) {
            continue;
        }
        seen.push(name.to_owned());
        out.push_str(declaration);
        out.push('\n');
    }
    out
}
