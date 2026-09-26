"use client";

/**
 * The box reducers are written in.
 *
 * This is the main thing a developer does in Studio, and a `<textarea>` gave it nothing:
 * no colour, no completion, and errors printed underneath rather than marked where they
 * are. CodeMirror gives all three, and the ingredients were already here — the compiler
 * reports its problems with byte spans (ADR 0011), and the control plane already knows
 * every source's fields and every column of the table.
 *
 * The language is JavaScript's grammar on purpose: the DSL is JS-shaped (ADR 0025), so
 * the same parser that highlights one highlights the other, and nothing about the syntax
 * has to be re-described here.
 */

import { autocompletion, closeBrackets, closeBracketsKeymap } from "@codemirror/autocomplete";
import type { Completion, CompletionContext, CompletionResult } from "@codemirror/autocomplete";
import { defaultKeymap, history, historyKeymap, indentWithTab } from "@codemirror/commands";
import { javascript } from "@codemirror/lang-javascript";
import {
  HighlightStyle,
  bracketMatching,
  indentOnInput,
  syntaxHighlighting,
} from "@codemirror/language";
import { type Diagnostic, lintGutter, setDiagnostics } from "@codemirror/lint";
import { Compartment, EditorState } from "@codemirror/state";
import { EditorView, highlightActiveLine, keymap, lineNumbers } from "@codemirror/view";
import { tags } from "@lezer/highlight";
import { useEffect, useMemo, useRef } from "react";

import type { Problem, SourceInfo, Table } from "@/lib/api";

/** The reducers file's index among a project's sources (ADR 0025). */
const REDUCERS_FILE = 1;

/**
 * Colours from the app's own tokens, so the editor is the same in both themes without
 * knowing which one is on.
 */
const highlight = HighlightStyle.define([
  { tag: [tags.keyword, tags.modifier], color: "var(--color-syntax-keyword)" },
  { tag: [tags.function(tags.variableName), tags.function(tags.propertyName)], color: "var(--color-syntax-call)" },
  { tag: [tags.number, tags.bool], color: "var(--color-tertiary)" },
  { tag: [tags.string, tags.special(tags.string)], color: "var(--color-tertiary)" },
  { tag: [tags.comment], color: "var(--color-on-surface-variant)", fontStyle: "italic" },
  { tag: [tags.propertyName], color: "var(--color-on-surface)" },
  { tag: [tags.variableName, tags.definition(tags.variableName)], color: "var(--color-on-surface)" },
  { tag: [tags.operator, tags.punctuation], color: "var(--color-on-surface-variant)" },
]);

const theme = EditorView.theme({
  "&": { backgroundColor: "transparent", color: "var(--color-on-surface)" },
  "&.cm-focused": { outline: "none" },
  ".cm-scroller": {
    fontFamily: "var(--font-mono, ui-monospace, monospace)",
    fontSize: "13px",
    lineHeight: "1.65",
  },
  ".cm-content": { padding: "12px 0" },
  ".cm-gutters": {
    backgroundColor: "transparent",
    border: "none",
    color: "var(--color-on-surface-variant)",
    opacity: "0.55",
  },
  ".cm-activeLine": { backgroundColor: "color-mix(in oklab, var(--color-on-surface) 4%, transparent)" },
  ".cm-lintRange-error": {
    // A wavy underline in the app's error colour, rather than CodeMirror's own red.
    backgroundImage: "none",
    textDecoration: "underline wavy var(--color-error)",
    textDecorationSkipInk: "none",
  },
  ".cm-tooltip": {
    backgroundColor: "var(--color-menu)",
    border: "1px solid var(--color-outline-variant)",
    borderRadius: "6px",
    boxShadow: "0 4px 16px rgb(0 0 0 / 0.14)",
  },
  ".cm-tooltip-autocomplete ul li[aria-selected]": {
    backgroundColor: "var(--color-secondary-container)",
    color: "var(--color-on-secondary-container)",
  },
  ".cm-diagnostic": { fontFamily: "inherit", padding: "6px 8px" },
});

/** What names mean something at the cursor. */
export type Scope = {
  table: Table;
  sources: SourceInfo[];
  /** Every function the expression language offers (`docs/expressions.md`). */
  functions: string[];
};

const KEYWORDS: Completion[] = [
  { label: "table", type: "function", detail: "declare this table" },
  { label: "on", type: "function", detail: "when a record arrives" },
  { label: "row", type: "method", detail: "the row for a key" },
  { label: "delete", type: "method", detail: "remove the row" },
  { label: "const", type: "keyword" },
  { label: "if", type: "keyword" },
  { label: "else", type: "keyword" },
  { label: "return", type: "keyword" },
  { label: "tx.version", type: "variable", detail: "u64" },
  { label: "tx.timestamp", type: "variable", detail: "u64 — microseconds" },
];

/**
 * The handler the cursor is inside, as far back as the nearest `on(<source>, (<name>) =>`.
 *
 * A regex rather than a walk of the syntax tree: it only has to answer "what is `r` here",
 * and the shape it looks for is the one shape the DSL's handlers have.
 */
function handlerAt(doc: string, pos: number): { source: string; param: string } | null {
  const before = doc.slice(0, pos);
  let found: { source: string; param: string } | null = null;
  for (const m of before.matchAll(/\bon\(\s*([A-Za-z_]\w*)(?:\.\w+)?\s*,\s*\(?\s*([A-Za-z_]\w*)/g)) {
    found = { source: m[1] ?? "", param: m[2] ?? "" };
  }
  return found;
}

function completions(scope: Scope) {
  return (context: CompletionContext): CompletionResult | null => {
    const doc = context.state.doc.toString();

    // After a dot, the only sensible answers are that thing's own members.
    const dotted = context.matchBefore(/([A-Za-z_]\w*)\.\w*$/);
    if (dotted) {
      const owner = /^([A-Za-z_]\w*)\./.exec(dotted.text)?.[1] ?? "";
      const from = dotted.from + owner.length + 1;
      const handler = handlerAt(doc, context.pos);

      // The record this handler was handed: its source's fields, with their types.
      if (handler && owner === handler.param) {
        const source = scope.sources.find((s) => s.name === handler.source);
        if (source) {
          return {
            from,
            options: source.fields.map((f) => ({
              label: f.name,
              type: "property",
              detail: f.type + (f.nullable ? "?" : ""),
            })),
          };
        }
      }

      // A row of this table: the columns a rule may set.
      const columns = scope.table.columns
        .filter((c) => !scope.table.key.includes(c.name))
        .map((c) => ({ label: c.name, type: "property", detail: c.type }));
      if (owner === scope.table.name) {
        return { from, options: [{ label: "row", type: "method", detail: "row(key)" }] };
      }
      return { from, options: columns };
    }

    const word = context.matchBefore(/[\w.]*/);
    if (!word || (word.from === word.to && !context.explicit)) return null;

    const options: Completion[] = [
      ...KEYWORDS,
      { label: scope.table.name, type: "class", detail: "this table" },
      ...scope.sources.map((s) => ({
        label: s.name,
        type: "variable",
        detail: `${s.kind} source`,
      })),
      ...scope.table.columns.map((c) => ({
        label: c.name,
        type: "property",
        detail: c.type,
      })),
      ...scope.functions.map((f) => ({ label: f, type: "function" })),
    ];
    return { from: word.from, options };
  };
}

/**
 * The compiler's problems as marks on the text.
 *
 * Spans are offsets into the whole reducers file and the editor holds one table's slice
 * of it, so each one shifts by where that slice starts. A problem in the config is not
 * this box's to show, and one that lands outside the slice is dropped rather than
 * clamped onto an innocent line.
 */
function marks(problems: Problem[], offset: number, length: number): Diagnostic[] {
  return problems.flatMap((problem) => {
    const at = problem.at;
    if (!at || at.file !== REDUCERS_FILE) return [];
    const from = at.offset - offset;
    const to = from + Math.max(at.len, 1);
    if (from < 0 || to > length) return [];
    return [
      {
        from,
        to,
        severity: "error" as const,
        message: problem.help ? `${problem.message}\n\n${problem.help}` : problem.message,
      },
    ];
  });
}

export function ReducerEditor({
  value,
  onChange,
  scope,
  problems,
  offset,
  readOnly = false,
}: {
  value: string;
  onChange: (next: string) => void;
  scope: Scope;
  /** The problems the last check found, anywhere in the project. */
  problems: Problem[];
  /** Where this editor's text starts inside the whole reducers file. */
  offset: number;
  readOnly?: boolean;
}) {
  const host = useRef<HTMLDivElement>(null);
  const view = useRef<EditorView | null>(null);
  const latest = useRef({ onChange, scope, problems, offset });
  latest.current = { onChange, scope, problems, offset };

  // Recreated only when the table changes: a new EditorState would throw away the
  // cursor, the selection and the undo history on every keystroke otherwise.
  const editable = useMemo(() => new Compartment(), []);

  useEffect(() => {
    if (!host.current || view.current) return;
    const created: EditorView = new EditorView({
      parent: host.current,
      state: EditorState.create({
        doc: value,
        extensions: [
          lineNumbers(),
          history(),
          indentOnInput(),
          bracketMatching(),
          closeBrackets(),
          highlightActiveLine(),
          javascript({ typescript: true }),
          syntaxHighlighting(highlight),
          autocompletion({ override: [(c) => completions(latest.current.scope)(c)] }),
          lintGutter(),
          keymap.of([...closeBracketsKeymap, ...defaultKeymap, ...historyKeymap, indentWithTab]),
          theme,
          EditorView.lineWrapping,
          EditorView.updateListener.of((update) => {
            if (update.docChanged) latest.current.onChange(update.state.doc.toString());
          }),
          editable.of(EditorView.editable.of(!readOnly)),
        ],
      }),
    });
    view.current = created;
    return () => {
      created.destroy();
      view.current = null;
    };
    // eslint-disable-next-line react-hooks/exhaustive-deps
  }, []);

  // Text set from outside — a revert, or a save coming back — without disturbing a cursor
  // that is already where the text says it should be.
  useEffect(() => {
    const v = view.current;
    if (!v || v.state.doc.toString() === value) return;
    v.dispatch({ changes: { from: 0, to: v.state.doc.length, insert: value } });
  }, [value]);

  useEffect(() => {
    view.current?.dispatch({ effects: editable.reconfigure(EditorView.editable.of(!readOnly)) });
  }, [readOnly, editable]);

  // A new check came back, so its problems become the marks.
  //
  // `setDiagnostics` rather than `linter()`: a linter is a source the editor *asks*, on
  // its own schedule and on document changes, and these arrive on someone else's — the
  // answer to a check sent 400ms ago, when the text has not changed since. Asked for on
  // the editor's schedule, the marks were always one check behind. Pushed, they are the
  // answer to the text on screen.
  useEffect(() => {
    const v = view.current;
    if (!v) return;
    v.dispatch(setDiagnostics(v.state, marks(problems, offset, v.state.doc.length)));
  }, [problems, offset]);

  return <div ref={host} className="reducer-editor" />;
}
