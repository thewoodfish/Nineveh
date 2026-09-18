// The page's shared pieces. Everything light, everything on the same canvas: the
// sections are told apart by rhythm and rules, not by slabs of colour.

import type { ReactNode } from "react";

/** A stepped ziggurat: Nineveh's skyline, and state built up layer on layer. */
export function Logo({ className = "size-5" }: { className?: string }) {
  return (
    <svg viewBox="0 0 20 20" className={className} aria-hidden>
      <path fill="currentColor" d="M8 3h4v3H8zM5 7h10v4H5zM2 12h16v5H2z" />
    </svg>
  );
}

export function Button({
  href,
  tone = "primary",
  size = "md",
  children,
}: {
  href: string;
  tone?: "primary" | "quiet";
  size?: "md" | "lg";
  children: ReactNode;
}) {
  const tones = {
    primary:
      "bg-blue-600 text-white shadow-card hover:bg-blue-500 hover:shadow-hero focus-visible:ring-blue-300",
    quiet:
      "border border-ink-200 bg-white/80 text-ink-800 shadow-soft backdrop-blur hover:border-blue-200 hover:text-ink-900 focus-visible:ring-blue-200",
  };
  const sizes = { md: "px-4 py-2.5 text-sm", lg: "px-5 py-3 text-sm" };
  return (
    <a
      href={href}
      className={`inline-flex items-center justify-center gap-2 rounded-xl font-medium transition-all outline-none focus-visible:ring-2 ${sizes[size]} ${tones[tone]}`}
    >
      {children}
    </a>
  );
}

/** The line above a heading: what this stretch of the page is about. */
export function Eyebrow({ children }: { children: ReactNode }) {
  return (
    <div className="flex items-center gap-2.5 text-xs font-semibold tracking-[0.14em] text-blue-600 uppercase">
      <span className="h-px w-6 bg-blue-300" />
      {children}
    </div>
  );
}

/**
 * One stretch of the page. Sections share the canvas, so the only thing between them
 * is space — and, where the argument turns, a hairline.
 */
export function Section({
  id,
  rule = true,
  children,
}: {
  id?: string;
  rule?: boolean;
  children: ReactNode;
}) {
  return (
    <section id={id} className="mx-auto w-full max-w-6xl px-6">
      {rule && <hr className="border-0 border-t border-ink-200/70" />}
      <div className="py-20 sm:py-28">{children}</div>
    </section>
  );
}

export function Heading({ children }: { children: ReactNode }) {
  return (
    <h2 className="max-w-3xl text-3xl font-semibold tracking-tight text-balance text-ink-900 sm:text-[2.6rem] sm:leading-[1.1]">
      {children}
    </h2>
  );
}

export function Lede({ children }: { children: ReactNode }) {
  return <p className="mt-5 max-w-2xl text-lg leading-relaxed text-pretty text-ink-500">{children}</p>;
}

/** A pane of code, light like the rest of the page, coloured by a few plain rules. */
export function Code({ title, lines }: { title: string; lines: string[] }) {
  return (
    <div className="overflow-hidden rounded-2xl border border-ink-200/80 bg-white shadow-card">
      <div className="flex items-center gap-2 border-b border-ink-200/70 bg-ink-50/60 px-4 py-2.5">
        <span className="size-2 rounded-full bg-ink-200" />
        <span className="font-mono text-[11px] font-medium text-ink-400">{title}</span>
      </div>
      <pre className="overflow-x-auto px-4 py-4 font-mono text-[12.5px] leading-[1.8] text-ink-700">
        {lines.map((line, i) => (
          <div key={i}>{paint(line)}</div>
        ))}
      </pre>
    </div>
  );
}

/** Keys, strings and comments, told apart. */
function paint(line: string) {
  if (line.trimStart().startsWith("#")) {
    return <span className="text-ink-400">{line || " "}</span>;
  }
  const parts: ReactNode[] = [];
  const pattern = /("[^"]*")|(\b[a-z_][a-z0-9_]*:)/gi;
  let at = 0;
  for (const match of line.matchAll(pattern)) {
    const index = match.index ?? 0;
    if (index > at) parts.push(line.slice(at, index));
    parts.push(
      <span key={index} className={match[1] ? "text-emerald-700" : "font-medium text-blue-700"}>
        {match[0]}
      </span>,
    );
    at = index + match[0].length;
  }
  parts.push(line.slice(at));
  return line === "" ? " " : parts;
}
