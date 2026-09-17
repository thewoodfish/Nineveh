"use client";

// An input for one reducer expression: highlighted as you type, with the names in
// scope offered as you type them, and insertable from outside (the chips above a
// rule). The server is the authority on whether an expression holds up — this only
// helps you write it, so it flags a name it doesn't know but never blocks anything.

import { useCallback, useEffect, useMemo, useRef, useState } from "react";

/** Something an expression can refer to, as offered in the list. */
export type Name = {
  /** What the list shows. */
  label: string;
  /** What typing it puts in the expression, if that differs (`sellers[`). */
  insert?: string;
  /** The type or a word about where it comes from. */
  detail?: string;
  kind: "field" | "column" | "builtin" | "function" | "table";
};

/** What the chips above a rule use to type into the expression you're editing. */
export type Insert = { insert: (text: string) => void };

const KEYWORDS = ["if", "then", "else", "true", "false", "null"];

const TOKENS =
  /\s+|[A-Za-z_][A-Za-z0-9_]*|@0x[0-9a-fA-F]*|\d[\d_]*(?:[iu]\d+)?|"(?:[^"\\]|\\.)*"?|'(?:[^'\\]|\\.)*'?|==|!=|<=|>=|&&|\|\||./g;

const CLASSES: Record<string, string> = {
  keyword: "text-violet-600 dark:text-violet-400",
  number: "text-amber-700 dark:text-amber-400",
  text: "text-emerald-700 dark:text-emerald-400",
  name: "text-sky-700 dark:text-sky-300",
  call: "text-indigo-600 dark:text-indigo-300",
  unknown: "text-red-600 underline decoration-wavy decoration-red-400 dark:text-red-400",
  plain: "text-zinc-500 dark:text-zinc-400",
  word: "text-zinc-800 dark:text-zinc-200",
};

/** Colour each token of `text`, given the names that are in scope. */
function highlight(text: string, known: Set<string>): { text: string; kind: string }[] {
  const out: { text: string; kind: string }[] = [];
  const matches = text.match(TOKENS) ?? [];
  let previous = "";
  for (let i = 0; i < matches.length; i++) {
    const token = matches[i] ?? "";
    const next = (matches[i + 1] ?? "").trim() === "" ? (matches[i + 2] ?? "") : (matches[i + 1] ?? "");
    let kind = "plain";
    if (/^\s+$/.test(token)) kind = "plain";
    else if (KEYWORDS.includes(token)) kind = "keyword";
    else if (/^["']/.test(token)) kind = "text";
    else if (/^@0x/.test(token) || /^\d/.test(token)) kind = token.startsWith("@") ? "text" : "number";
    else if (/^[A-Za-z_]/.test(token)) {
      if (next === "(") kind = "call";
      // A qualifier (`tx.version`, `row.count`, `value.size`) and the field after it
      // are both names the server resolves, not ones this knows.
      else if (previous === "." || next === ".") kind = "word";
      else if (known.has(token)) kind = "name";
      else kind = "unknown";
    }
    out.push({ text: token, kind });
    if (token.trim() !== "") previous = token;
  }
  return out;
}

/** The word being typed just before `caret`, which the list completes. */
function tokenAt(text: string, caret: number): Token {
  let start = caret;
  while (start > 0 && /[A-Za-z0-9_]/.test(text[start - 1] ?? "")) start -= 1;
  return { word: text.slice(start, caret), start, end: caret };
}

/** The word under the caret, as the input itself reports it. */
type Token = { word: string; start: number; end: number };

const NOTHING: Token = { word: "", start: 0, end: 0 };

export function ExpressionInput({
  value,
  onChange,
  names,
  placeholder,
  onActive,
  invalid,
}: {
  value: string;
  onChange: (value: string) => void;
  names: Name[];
  placeholder?: string;
  /** Called with a handle while this input has the focus, and `null` when it loses it. */
  onActive?: (handle: Insert | null) => void;
  invalid?: boolean;
}) {
  const input = useRef<HTMLInputElement>(null);
  const mirror = useRef<HTMLPreElement>(null);
  const [open, setOpen] = useState(false);
  const [picked, setPicked] = useState(0);
  // Kept from the input's own events rather than derived later, so the list always
  // completes the word that's actually under the caret.
  const [token, setToken] = useState<Token>(NOTHING);

  const known = useMemo(() => new Set(names.map((n) => n.label.replace(/[[(].*$/, ""))), [names]);
  const painted = useMemo(() => highlight(value, known), [value, known]);

  const matches = useMemo(() => {
    if (!open || token.word === "") return [];
    const word = token.word.toLowerCase();
    return names
      .filter((n) => n.label.toLowerCase().includes(word))
      .sort((a, b) => {
        const starts = (n: Name) => (n.label.toLowerCase().startsWith(word) ? 0 : 1);
        return starts(a) - starts(b) || a.label.length - b.label.length;
      })
      .slice(0, 8);
  }, [names, open, token.word]);

  const track = (element: HTMLInputElement) =>
    setToken(tokenAt(element.value, element.selectionStart ?? element.value.length));

  useEffect(() => setPicked(0), [token.word]);

  /**
   * Put `text` in over `range`, or at the caret when there's no range. Completion
   * passes the token the list was built from rather than letting this re-read the
   * caret, so accepting always replaces the word that was being offered.
   */
  const put = useCallback(
    (text: string, range?: { start: number; end: number }) => {
      const element = input.current;
      const caret = element?.selectionStart ?? value.length;
      const from = range?.start ?? caret;
      const until = range?.end ?? caret;
      onChange(`${value.slice(0, from)}${text}${value.slice(until)}`);
      const to = from + text.length;
      setToken({ word: "", start: to, end: to });
      requestAnimationFrame(() => {
        element?.focus();
        element?.setSelectionRange(to, to);
      });
    },
    [onChange, value],
  );

  const handle = useMemo<Insert>(() => ({ insert: (text) => put(text) }), [put]);

  const accept = (name: Name) => {
    put(name.insert ?? name.label, token);
    setOpen(false);
  };

  return (
    <div className="relative flex-1">
      <pre
        ref={mirror}
        aria-hidden
        className="pointer-events-none absolute inset-0 overflow-hidden rounded-md border border-transparent px-2.5 py-1.5 font-mono text-sm whitespace-pre"
      >
        {painted.map((part, i) => (
          <span key={i} className={CLASSES[part.kind]}>
            {part.text}
          </span>
        ))}
      </pre>
      <input
        ref={input}
        value={value}
        spellCheck={false}
        placeholder={placeholder}
        onChange={(e) => {
          onChange(e.target.value);
          track(e.target);
          setOpen(true);
        }}
        onFocus={(e) => {
          onActive?.(handle);
          track(e.target);
        }}
        onBlur={() => {
          // Late enough for a click on the list to land first.
          setTimeout(() => setOpen(false), 120);
          onActive?.(null);
        }}
        onScroll={(e) => {
          if (mirror.current) mirror.current.scrollLeft = e.currentTarget.scrollLeft;
        }}
        onKeyUp={(e) => track(e.currentTarget)}
        onClick={(e) => track(e.currentTarget)}
        onKeyDown={(e) => {
          if (matches.length === 0) {
            if (e.key === "Escape") setOpen(false);
            return;
          }
          if (e.key === "ArrowDown" || e.key === "ArrowUp") {
            e.preventDefault();
            const step = e.key === "ArrowDown" ? 1 : matches.length - 1;
            setPicked((p) => (p + step) % matches.length);
          } else if (e.key === "Enter" || e.key === "Tab") {
            const match = matches[picked];
            if (match) {
              e.preventDefault();
              accept(match);
            }
          } else if (e.key === "Escape") {
            e.preventDefault();
            setOpen(false);
          }
        }}
        className={`w-full rounded-md border bg-white px-2.5 py-1.5 font-mono text-sm text-transparent caret-zinc-900 focus:outline-none dark:bg-zinc-900 dark:caret-zinc-100 ${
          invalid
            ? "border-red-400 focus:border-red-500"
            : "border-zinc-200 focus:border-lapis-400 dark:border-zinc-700"
        }`}
      />
      {matches.length > 0 && (
        <ul className="absolute top-full left-0 z-20 mt-1 max-h-56 w-72 overflow-auto rounded-md border border-zinc-200 bg-white py-1 shadow-lg dark:border-zinc-700 dark:bg-zinc-900">
          {matches.map((name, i) => (
            <li key={name.label}>
              <button
                type="button"
                onMouseDown={(e) => e.preventDefault()}
                onClick={() => accept(name)}
                onMouseEnter={() => setPicked(i)}
                className={`flex w-full items-baseline justify-between gap-3 px-2.5 py-1 text-left font-mono text-xs ${
                  i === picked ? "bg-lapis-50 dark:bg-zinc-800" : ""
                }`}
              >
                <span className="text-zinc-800 dark:text-zinc-200">{name.label}</span>
                <span className="shrink-0 text-zinc-400">{name.detail}</span>
              </button>
            </li>
          ))}
        </ul>
      )}
    </div>
  );
}
