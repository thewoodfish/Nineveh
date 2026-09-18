// The page's shared pieces. The page is dark throughout, so these have one look: white
// for what matters, white at low opacity for what supports it, blue for what's alive.

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
      "bg-blue-600 text-white shadow-card hover:bg-blue-500 hover:shadow-glow focus-visible:ring-blue-400",
    quiet:
      "border border-white/15 bg-white/[0.06] text-white/80 backdrop-blur hover:border-white/25 hover:bg-white/10 hover:text-white focus-visible:ring-white/30",
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
export function Eyebrow({ children, center = false }: { children: ReactNode; center?: boolean }) {
  return (
    <div
      className={`flex items-center gap-2.5 text-xs font-semibold tracking-[0.14em] text-blue-300 uppercase ${
        center ? "justify-center" : ""
      }`}
    >
      <span className="h-px w-6 bg-blue-400/60" />
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
      {rule && <hr className="border-0 border-t border-white/10" />}
      <div className="py-20 sm:py-28">{children}</div>
    </section>
  );
}

export function Heading({ children, center = false }: { children: ReactNode; center?: boolean }) {
  return (
    <h2
      className={`mt-5 max-w-3xl text-3xl font-semibold tracking-tight text-balance text-white sm:text-[2.7rem] sm:leading-[1.08] ${
        center ? "mx-auto" : ""
      }`}
    >
      {children}
    </h2>
  );
}

export function Lede({ children, center = false }: { children: ReactNode; center?: boolean }) {
  return (
    <p
      className={`mt-5 max-w-2xl text-lg leading-relaxed text-pretty text-white/55 ${
        center ? "mx-auto" : ""
      }`}
    >
      {children}
    </p>
  );
}

/** A pane of code, coloured by a few plain rules. */
export function Code({ title, lines }: { title: string; lines: string[] }) {
  return (
    <div className="overflow-hidden rounded-2xl bg-white/[0.045] ring-1 ring-white/10 backdrop-blur">
      <div className="flex items-center gap-2 border-b border-white/10 px-4 py-2.5">
        <span className="size-2 rounded-full bg-white/20" />
        <span className="font-mono text-[11px] font-medium text-white/40">{title}</span>
      </div>
      <pre className="overflow-x-auto px-4 py-4 font-mono text-[12.5px] leading-[1.8] text-white/75">
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
    return <span className="text-white/30">{line || " "}</span>;
  }
  const parts: ReactNode[] = [];
  const pattern = /("[^"]*")|(\b[a-z_][a-z0-9_]*:)/gi;
  let at = 0;
  for (const match of line.matchAll(pattern)) {
    const index = match.index ?? 0;
    if (index > at) parts.push(line.slice(at, index));
    parts.push(
      <span key={index} className={match[1] ? "text-emerald-300/90" : "text-blue-300"}>
        {match[0]}
      </span>,
    );
    at = index + match[0].length;
  }
  parts.push(line.slice(at));
  return line === "" ? " " : parts;
}
