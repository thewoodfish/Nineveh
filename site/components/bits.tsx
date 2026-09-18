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
export function Eyebrow({ children, dark = false }: { children: ReactNode; dark?: boolean }) {
  return (
    <div
      className={`flex items-center gap-2.5 text-xs font-semibold tracking-[0.14em] uppercase ${
        dark ? "text-blue-300" : "text-blue-600"
      }`}
    >
      <span className={`h-px w-6 ${dark ? "bg-blue-400/60" : "bg-blue-300"}`} />
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

/**
 * A dark panel, inset and rounded so it sits *on* the page rather than cutting across
 * it. The page keeps its light rhythm; this is where it goes quiet and technical.
 */
export function Panel({ children }: { children: ReactNode }) {
  return (
    <div className="relative overflow-hidden rounded-3xl bg-ink-900 px-6 py-14 shadow-hero sm:px-12 sm:py-20">
      <div
        className="absolute inset-0"
        style={{
          background:
            "radial-gradient(40rem 26rem at 12% -10%, oklch(0.552 0.221 261 / 0.45), transparent 68%), radial-gradient(34rem 22rem at 96% 108%, oklch(0.63 0.196 259 / 0.3), transparent 66%)",
        }}
        aria-hidden
      />
      {/* The same weave as the page, carried inside so the panel belongs to it. */}
      <div
        className="absolute inset-0 opacity-[0.35]"
        style={{
          backgroundImage:
            "linear-gradient(to right, oklch(1 0 0 / 0.05) 1px, transparent 1px), linear-gradient(to bottom, oklch(1 0 0 / 0.05) 1px, transparent 1px)",
          backgroundSize: "64px 64px",
          maskImage: "radial-gradient(38rem 24rem at 50% 30%, black, transparent 75%)",
        }}
        aria-hidden
      />
      <div className="relative">{children}</div>
    </div>
  );
}

export function Heading({ children, dark = false }: { children: ReactNode; dark?: boolean }) {
  return (
    <h2
      className={`max-w-3xl text-3xl font-semibold tracking-tight text-balance sm:text-[2.6rem] sm:leading-[1.1] ${
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
      className={`mt-5 max-w-2xl text-lg leading-relaxed text-pretty ${
        dark ? "text-white/60" : "text-ink-500"
      }`}
    >
      {children}
    </p>
  );
}

/** A pane of code, coloured by a few plain rules, on either ground. */
export function Code({
  title,
  lines,
  dark = false,
}: {
  title: string;
  lines: string[];
  dark?: boolean;
}) {
  return (
    <div
      className={`overflow-hidden rounded-2xl ${
        dark
          ? "bg-white/[0.045] ring-1 ring-white/10 backdrop-blur"
          : "border border-ink-200/80 bg-white shadow-card"
      }`}
    >
      <div
        className={`flex items-center gap-2 px-4 py-2.5 ${
          dark ? "border-b border-white/10" : "border-b border-ink-200/70 bg-ink-50/60"
        }`}
      >
        <span className={`size-2 rounded-full ${dark ? "bg-white/20" : "bg-ink-200"}`} />
        <span
          className={`font-mono text-[11px] font-medium ${dark ? "text-white/40" : "text-ink-400"}`}
        >
          {title}
        </span>
      </div>
      <pre
        className={`overflow-x-auto px-4 py-4 font-mono text-[12.5px] leading-[1.8] ${
          dark ? "text-white/75" : "text-ink-700"
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
function paint(line: string, dark = false) {
  const comment = dark ? "text-white/30" : "text-ink-400";
  const key = dark ? "text-blue-300" : "font-medium text-blue-700";
  const string = dark ? "text-emerald-300/90" : "text-emerald-700";
  if (line.trimStart().startsWith("#")) {
    return <span className={comment}>{line || " "}</span>;
  }
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
  return line === "" ? " " : parts;
}
