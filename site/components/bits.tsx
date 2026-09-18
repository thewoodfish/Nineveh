// The page's small shared pieces: the mark, buttons, section framing, and the code
// blocks that do half the talking.

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
  children,
}: {
  href: string;
  tone?: "primary" | "ghost" | "light";
  children: ReactNode;
}) {
  const tones = {
    primary:
      "bg-blue-600 text-white shadow-lifted hover:bg-blue-500 focus-visible:ring-blue-300",
    ghost:
      "border border-white/15 bg-white/[0.04] text-white hover:bg-white/10 focus-visible:ring-white/40",
    light:
      "border border-ink-200 bg-white text-ink-900 shadow-card hover:bg-ink-50 focus-visible:ring-blue-300",
  };
  return (
    <a
      href={href}
      className={`inline-flex items-center justify-center gap-2 rounded-xl px-5 py-3 text-sm font-medium transition-colors outline-none focus-visible:ring-2 ${tones[tone]}`}
    >
      {children}
    </a>
  );
}

/** The line above a section's heading: what this part of the page is about. */
export function Eyebrow({ children }: { children: ReactNode }) {
  return (
    <div className="text-xs font-semibold tracking-[0.12em] text-blue-600 uppercase">
      {children}
    </div>
  );
}

export function Section({
  id,
  dark = false,
  children,
}: {
  id?: string;
  dark?: boolean;
  children: ReactNode;
}) {
  return (
    <section id={id} className={dark ? "on-dark" : "bg-white"}>
      <div className="mx-auto max-w-6xl px-6 py-20 sm:py-28">{children}</div>
    </section>
  );
}

export function Heading({ children, dark = false }: { children: ReactNode; dark?: boolean }) {
  return (
    <h2
      className={`max-w-3xl text-3xl font-semibold tracking-tight text-balance sm:text-4xl ${
        dark ? "text-white" : "text-ink-900"
      }`}
    >
      {children}
    </h2>
  );
}

export function Lede({ children, dark = false }: { children: ReactNode; dark?: boolean }) {
  return (
    <p
      className={`mt-4 max-w-2xl text-lg leading-relaxed text-pretty ${
        dark ? "text-white/60" : "text-ink-500"
      }`}
    >
      {children}
    </p>
  );
}

/** Code, coloured by hand: a few rules read better than a highlighter here. */
export function Code({
  title,
  lines,
  dark = true,
}: {
  title: string;
  lines: string[];
  dark?: boolean;
}) {
  return (
    <div
      className={`overflow-hidden rounded-2xl ${
        dark ? "bg-ink-950 ring-1 ring-white/10" : "bg-white shadow-lifted ring-1 ring-ink-200"
      }`}
    >
      <div
        className={`flex items-center gap-2 px-4 py-2.5 text-[11px] font-medium ${
          dark ? "border-b border-white/10 text-white/40" : "border-b border-ink-200 text-ink-400"
        }`}
      >
        <span className="font-mono">{title}</span>
      </div>
      <pre
        className={`overflow-x-auto px-4 py-4 font-mono text-[12.5px] leading-[1.75] ${
          dark ? "text-white/80" : "text-ink-700"
        }`}
      >
        {lines.map((line, i) => (
          <div key={i}>{paint(line, dark)}</div>
        ))}
      </pre>
    </div>
  );
}

/** Keys, strings and comments, told apart. */
function paint(line: string, dark: boolean) {
  const muted = dark ? "text-white/30" : "text-ink-400";
  const key = dark ? "text-blue-300" : "text-blue-700";
  const string = dark ? "text-emerald-300/90" : "text-emerald-700";
  if (line.trimStart().startsWith("#")) return <span className={muted}>{line || " "}</span>;
  const parts: ReactNode[] = [];
  const pattern = /("[^"]*")|(\b[a-z_][a-z0-9_]*:)/gi;
  let at = 0;
  for (const match of line.matchAll(pattern)) {
    const index = match.index ?? 0;
    if (index > at) parts.push(line.slice(at, index));
    parts.push(
      <span key={index} className={match[1] ? string : key}>
        {match[0]}
      </span>,
    );
    at = index + match[0].length;
  }
  parts.push(line.slice(at));
  return parts.length === 1 && line === "" ? " " : parts;
}
