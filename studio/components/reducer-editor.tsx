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
import {
  EditorView,
  highlightActiveLine,
  hoverTooltip,
  keymap,
  lineNumbers,
} from "@codemirror/view";
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
  // Sized with the editor, not the page: left to inherit, a message is set in body text
  // and towers over the line it is about.
  ".cm-diagnostic": {
    fontFamily: "inherit",
    fontSize: "11.5px",
    lineHeight: "1.5",
    padding: "7px 9px",
    maxWidth: "22rem",
  },
  ".cm-nv-hover": { maxWidth: "22rem", padding: "7px 9px" },
  ".cm-nv-hover-signature": {
    fontFamily: "var(--font-mono, ui-monospace, monospace)",
    fontSize: "12px",
    color: "var(--color-on-surface)",
  },
  ".cm-nv-hover-detail": {
    marginTop: "3px",
    fontSize: "11.5px",
    lineHeight: "1.5",
    color: "var(--color-on-surface-variant)",
  },
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

/** What the word at `from` is a member of, or null when it stands on its own. */
function ownerAt(doc: string, from: number): string | null {
  const before = doc.slice(0, from);
  if (!/\.\s*$/.test(before)) return null;
  const head = before.replace(/\s*\.\s*$/, "");
  // `balances.row(d.user).balance` — what owns `balance` is a row, not a name.
  const call = /([A-Za-z_]\w*)\s*\.\s*row\s*\([^()]*\)$/.exec(head);
  if (call) return `${call[1]}.row`;
  return /([A-Za-z_]\w*)$/.exec(head)?.[1] ?? null;
}

/** The word under `pos`, or null if the pointer is on punctuation or space. */
function wordAround(doc: string, pos: number): { from: number; to: number } | null {
  let from = pos;
  let to = pos;
  while (from > 0 && /\w/.test(doc[from - 1] ?? "")) from -= 1;
  while (to < doc.length && /\w/.test(doc[to] ?? "")) to += 1;
  return to > from ? { from, to } : null;
}

/**
 * The names bound to a row of `table`: `const b = balances.row(d.user)` makes `b` one.
 *
 * Table names are validated identifiers, so there is nothing in one to escape.
 */
function rowLocals(doc: string, table: string): Set<string> {
  const bound = new Set<string>();
  const pattern = new RegExp(`\\bconst\\s+([A-Za-z_]\\w*)\\s*=\\s*${table}\\s*\\.\\s*row\\s*\\(`, "g");
  for (const [, name] of doc.matchAll(pattern)) if (name) bound.add(name);
  return bound;
}

/** A column or field's type as the hover states it. */
function stated(type: string, nullable: boolean): string {
  return nullable ? `${type} | null` : type;
}

/**
 * What a name means here: its type on the first line, what it is on the second.
 *
 * Nothing is invented. Every answer comes from the project — the source's fields and the
 * table's columns, the same two things completion offers — so a name this can't account
 * for gets no tooltip rather than a guess. A misspelling is the linter's to report, and
 * saying it twice in two ways would only be noise.
 */
function explain(
  doc: string,
  from: number,
  to: number,
  scope: Scope,
): { signature: string; detail: string } | null {
  const word = doc.slice(from, to);
  if (!word) return null;
  const owner = ownerAt(doc, from);
  // From the word's end, not its start, so `r` in `on(sold, (r) => …)` is inside the
  // handler that names it: the place someone is most likely to ask what it is.
  const handler = handlerAt(doc, to);
  const key = (name: string) => scope.table.key.includes(name);
  const column = (name: string) => scope.table.columns.find((c) => c.name === name) ?? null;

  if (owner) {
    if (owner === "tx") {
      if (word === "version") {
        return {
          signature: "tx.version: u64",
          detail: "The transaction this record arrived in.",
        };
      }
      if (word === "timestamp") {
        return {
          signature: "tx.timestamp: u64",
          detail: "When that transaction committed, in microseconds.",
        };
      }
      return null;
    }

    // A field of the record this handler was handed.
    if (handler && owner === handler.param) {
      const source = scope.sources.find((s) => s.name === handler.source);
      const field = source?.fields.find((f) => f.name === word);
      if (!source || !field) return null;
      return {
        signature: `${word}: ${stated(field.type, field.nullable)}`,
        detail: `A field of ${source.name}, the ${source.kind} source this handler fires on.`,
      };
    }

    // A column of a row of this table, named or reached straight through `.row(…)`.
    if (rowLocals(doc, scope.table.name).has(owner) || owner === `${scope.table.name}.row`) {
      const found = column(word);
      if (!found) return null;
      return {
        signature: `${word}: ${stated(found.type, found.nullable)}`,
        detail: key(word)
          ? `A key column of ${scope.table.name}: it identifies the row, so a rule doesn't set it.`
          : `A column of ${scope.table.name}.`,
      };
    }

    if (owner === scope.table.name && word === "row") {
      return {
        signature: `row(${scope.table.key.join(", ")})`,
        detail: `The row of ${scope.table.name} with that key, created with its defaults if it isn't there yet.`,
      };
    }
    return null;
  }

  if (word === scope.table.name) {
    const columns = scope.table.columns.length;
    return {
      signature: `${scope.table.name} — key ${scope.table.key.join(", ")}`,
      detail: `This table: ${columns} column${columns === 1 ? "" : "s"}, written only by the handlers here.`,
    };
  }

  const source = scope.sources.find((s) => s.name === word);
  if (source) {
    return {
      signature: `${word}: ${source.kind} source`,
      // Plain text in a plain-text card: backticks around a name would be shown, not read.
      detail: `Follows ${source.follows}${source.deletes ? `, and ${word}.deleted fires on its deletes` : ""}.`,
    };
  }

  if (handler && word === handler.param) {
    const from_ = scope.sources.find((s) => s.name === handler.source);
    if (!from_) return null;
    return {
      signature: `${word}: ${handler.source} record`,
      detail: `The ${from_.kind} record that arrived, with ${from_.fields.length} field${from_.fields.length === 1 ? "" : "s"} to read.`,
    };
  }

  if (rowLocals(doc, scope.table.name).has(word)) {
    return {
      signature: `${word}: row of ${scope.table.name}`,
      detail: "Set its columns and the row is written when the handler returns.",
    };
  }

  const found = column(word);
  if (found) {
    return {
      signature: `${word}: ${stated(found.type, found.nullable)}`,
      detail: key(word)
        ? `A key column of ${scope.table.name}.`
        : `A column of ${scope.table.name}.`,
    };
  }

  if (scope.functions.includes(word)) {
    return { signature: `${word}(…)`, detail: "An expression function." };
  }
  return null;
}

/** The hover card, built from whatever `explain` can account for. */
function hovers(scope: () => Scope) {
  return hoverTooltip((view, pos) => {
    const doc = view.state.doc.toString();
    const word = wordAround(doc, pos);
    if (!word) return null;
    const found = explain(doc, word.from, word.to, scope());
    if (!found) return null;
    return {
      pos: word.from,
      end: word.to,
      above: true,
      create: () => {
        const dom = document.createElement("div");
        dom.className = "cm-nv-hover";
        const signature = document.createElement("div");
        signature.className = "cm-nv-hover-signature";
        signature.textContent = found.signature;
        const detail = document.createElement("div");
        detail.className = "cm-nv-hover-detail";
        detail.textContent = found.detail;
        dom.append(signature, detail);
        return { dom };
      },
    };
  });
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
          hovers(() => latest.current.scope),
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
