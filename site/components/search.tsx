"use client";

// Search over the documentation.
//
// The whole index is built at build time and shipped with the page: a few dozen
// kilobytes of headings and the opening of the prose under each. That is small enough
// to match in the browser on every keystroke, which is what makes this feel instant and
// what means the docs need no service behind them to be searchable.

import { useCallback, useEffect, useMemo, useRef, useState } from "react";

import type { Hit } from "@/lib/docs";

/** The bar in the header. Opens the dialog; never searches on its own. */
export function SearchTrigger() {
  const [open, setOpen] = useState(false);
  const [mac, setMac] = useState(false);

  useEffect(() => {
    setMac(/Mac|iPhone|iPad/.test(navigator.platform));
    const onKey = (e: KeyboardEvent) => {
      if (e.key === "k" && (e.metaKey || e.ctrlKey)) {
        e.preventDefault();
        setOpen(true);
      }
    };
    window.addEventListener("keydown", onKey);
    return () => window.removeEventListener("keydown", onKey);
  }, []);

  return (
    <>
      <button
        type="button"
        onClick={() => setOpen(true)}
        className="flex w-full items-center gap-2.5 rounded-lg border border-ink-line bg-ink-wash px-3 py-1.5 text-[13.5px] text-ink-faint transition-colors hover:border-ink-line-strong hover:text-ink-soft"
      >
        <svg viewBox="0 0 16 16" className="size-[15px] shrink-0" aria-hidden>
          <g fill="none" stroke="currentColor" strokeWidth="1.5" strokeLinecap="round">
            <circle cx="7" cy="7" r="4.5" />
            <path d="M10.5 10.5 14 14" />
          </g>
        </svg>
        <span className="min-w-0 flex-1 text-left">Search the docs</span>
        {/* Only shown once the platform is known, so it can't say ⌘K on Windows. */}
        <kbd className="hidden shrink-0 rounded border border-ink-line px-1.5 font-sans text-[11px] text-ink-faint sm:block">
          {mac ? "⌘K" : "Ctrl K"}
        </kbd>
      </button>
      {open && <SearchDialog onClose={() => setOpen(false)} />}
    </>
  );
}

function SearchDialog({ onClose }: { onClose: () => void }) {
  const [query, setQuery] = useState("");
  const [index, setIndex] = useState<Hit[] | null>(null);
  const [selected, setSelected] = useState(0);
  const input = useRef<HTMLInputElement>(null);
  const list = useRef<HTMLUListElement>(null);

  // Fetched when the dialog first opens rather than shipped with every page: nobody
  // who doesn't search should pay for the index.
  useEffect(() => {
    let alive = true;
    void fetch("/search.json")
      .then((r) => r.json() as Promise<Hit[]>)
      .then((hits) => alive && setIndex(hits))
      .catch(() => alive && setIndex([]));
    input.current?.focus();
    return () => {
      alive = false;
    };
  }, []);

  const results = useMemo(() => (index ? search(index, query) : []), [index, query]);

  useEffect(() => setSelected(0), [query]);

  const go = useCallback(
    (hit: Hit | undefined) => {
      if (hit) window.location.href = hit.href;
    },
    [],
  );

  const onKey = (e: React.KeyboardEvent) => {
    if (e.key === "Escape") return onClose();
    if (e.key === "ArrowDown" || e.key === "ArrowUp") {
      e.preventDefault();
      const next = e.key === "ArrowDown" ? selected + 1 : selected - 1;
      const bounded = Math.max(0, Math.min(results.length - 1, next));
      setSelected(bounded);
      list.current?.children[bounded]?.scrollIntoView({ block: "nearest" });
      return;
    }
    if (e.key === "Enter") {
      e.preventDefault();
      go(results[selected]);
    }
  };

  return (
    // eslint-disable-next-line jsx-a11y/no-noninteractive-element-interactions
    <div
      role="dialog"
      aria-modal="true"
      aria-label="Search the documentation"
      className="fixed inset-0 z-50 flex items-start justify-center px-4 pt-[12vh]"
      onKeyDown={onKey}
    >
      <button
        type="button"
        aria-label="Close search"
        onClick={onClose}
        className="fixed inset-0 cursor-default bg-scrim backdrop-blur-[2px]"
      />
      <div className="relative flex max-h-[70vh] w-full max-w-xl flex-col overflow-hidden rounded-xl border border-ink-line bg-panel shadow-[var(--lift)]">
        <div className="flex items-center gap-3 border-b border-ink-line px-4">
          <svg viewBox="0 0 16 16" className="size-4 shrink-0 text-ink-faint" aria-hidden>
            <g fill="none" stroke="currentColor" strokeWidth="1.5" strokeLinecap="round">
              <circle cx="7" cy="7" r="4.5" />
              <path d="M10.5 10.5 14 14" />
            </g>
          </svg>
          <input
            ref={input}
            value={query}
            onChange={(e) => setQuery(e.target.value)}
            placeholder="Search the docs"
            aria-label="Search the docs"
            className="min-w-0 flex-1 bg-transparent py-3.5 text-[15px] text-ink outline-none placeholder:text-ink-faint"
          />
          <kbd className="shrink-0 rounded border border-ink-line px-1.5 py-0.5 font-sans text-[11px] text-ink-faint">
            esc
          </kbd>
        </div>

        {query && results.length === 0 && index !== null && (
          <p className="px-4 py-8 text-center text-sm text-ink-faint">
            Nothing matches <span className="text-ink-soft">{query}</span>.
          </p>
        )}

        <ul ref={list} className="min-h-0 flex-1 overflow-y-auto p-2">
          {results.map((hit, i) => (
            <li key={hit.href}>
              <a
                href={hit.href}
                onMouseEnter={() => setSelected(i)}
                aria-current={i === selected ? "true" : undefined}
                className={`block rounded-lg px-3 py-2.5 ${
                  i === selected ? "bg-accent-wash" : ""
                }`}
              >
                <span className="flex items-baseline gap-2">
                  <span
                    className={`text-sm font-medium ${i === selected ? "text-accent" : "text-ink-strong"}`}
                  >
                    {hit.section ?? hit.page}
                  </span>
                  <span className="truncate text-[11.5px] text-ink-faint">
                    {hit.section ? `${hit.group} · ${hit.page}` : hit.group}
                  </span>
                </span>
                {/* `line-clamp` sets its own display, so it cannot carry `block` too:
                    two lines of context, not the whole paragraph. */}
                {hit.text && (
                  <span className="mt-0.5 line-clamp-2 text-[12.5px] leading-snug text-ink-soft">
                    {hit.text}
                  </span>
                )}
              </a>
            </li>
          ))}
        </ul>

        {!query && (
          <p className="border-t border-ink-line px-4 py-2.5 text-[12px] text-ink-faint">
            Type to search headings and their text. ↑↓ to move, ↵ to open.
          </p>
        )}
      </div>
    </div>
  );
}

/**
 * Rank the index against `query`.
 *
 * Every word has to appear somewhere in the entry, so a two-word query narrows instead
 * of widening — the thing a reader expects and the thing a plain substring match gets
 * wrong. What a word matched decides the order: a heading outranks the prose under it,
 * and a whole-word match outranks the middle of a longer one.
 */
function search(index: Hit[], query: string): Hit[] {
  const words = query.toLowerCase().split(/\s+/).filter(Boolean);
  if (words.length === 0) return index.filter((h) => !h.section).slice(0, 8);

  const scored: { hit: Hit; score: number }[] = [];
  for (const hit of index) {
    const label = (hit.section ?? hit.page).toLowerCase();
    const body = hit.text.toLowerCase();
    const page = hit.page.toLowerCase();
    let score = 0;
    let all = true;
    for (const word of words) {
      const inLabel = label.indexOf(word);
      const inBody = body.indexOf(word);
      const inPage = page.indexOf(word);
      if (inLabel === -1 && inBody === -1 && inPage === -1) {
        all = false;
        break;
      }
      if (inLabel === 0) score += 12;
      else if (inLabel > 0) score += new RegExp(`\\b${escape(word)}`).test(label) ? 8 : 5;
      else if (inPage !== -1) score += 3;
      if (inBody !== -1) score += 1;
    }
    // A page is a better answer than one of its own sections when both match equally.
    if (all) scored.push({ hit, score: score + (hit.section ? 0 : 1) });
  }
  return scored
    .sort((a, b) => b.score - a.score)
    .slice(0, 12)
    .map((s) => s.hit);
}

function escape(word: string): string {
  return word.replace(/[.*+?^${}()|[\]\\]/g, "\\$&");
}
