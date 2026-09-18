"use client";

import { useState, type ReactNode } from "react";

import { API_URL, type ColumnType } from "@/lib/api";
import { formatInteger, shortHex } from "@/lib/format";

const PHASES: Record<string, { label: string; dot: string; pulse?: boolean }> = {
  running: { label: "Running", dot: "bg-emerald-500", pulse: true },
  serving: { label: "Serving", dot: "bg-emerald-500" },
  starting: { label: "Starting", dot: "bg-sky-500", pulse: true },
  retrying: { label: "Retrying", dot: "bg-amber-500", pulse: true },
  halted: { label: "Halted", dot: "bg-red-500" },
  failed: { label: "Failed", dot: "bg-red-500" },
  stopped: { label: "Stopped", dot: "bg-white/30" },
  offline: { label: "Offline", dot: "bg-white/20" },
};

export function PhaseDot({ phase, label = false }: { phase: string; label?: boolean }) {
  const p = PHASES[phase] ?? { label: phase, dot: "bg-white/30" };
  return (
    <span className="inline-flex items-center gap-1.5 text-xs text-dim" title={p.label}>
      <span className="relative flex size-2">
        {p.pulse && (
          <span
            className={`absolute inline-flex size-full animate-ping rounded-full opacity-60 ${p.dot}`}
          />
        )}
        <span className={`relative inline-flex size-2 rounded-full ${p.dot}`} />
      </span>
      {label && p.label}
    </span>
  );
}

export function Card({ children, className = "" }: { children: ReactNode; className?: string }) {
  return (
    <div className={`rounded-xl border border-line bg-card shadow-card ${className}`}>
      {children}
    </div>
  );
}

/**
 * The shared shape of anything typed into: inputs, selects, textareas. Focus is a
 * blue ring rather than a border colour, so a field doesn't shift when you click it.
 */
export const field =
  "rounded-lg border border-line bg-well px-3 py-2 text-sm text-white outline-none transition-colors " +
  "placeholder:text-ghost hover:border-edge focus:border-blue-400 focus:ring-2 focus:ring-blue-500/25";

/**
 * A dropdown. The native element does the work — keyboard, type-to-select, the
 * platform's own menu — and only its chrome is replaced, because a hand-built listbox
 * is a great deal of code to get wrong.
 */
export function Select({
  className = "",
  children,
  ...props
}: React.SelectHTMLAttributes<HTMLSelectElement>) {
  return (
    <div className="relative inline-flex min-w-0">
      <select
        className={`${field} w-full cursor-pointer appearance-none pr-8 ${className}`}
        {...props}
      >
        {children}
      </select>
      <svg
        viewBox="0 0 12 12"
        className="pointer-events-none absolute top-1/2 right-2.5 size-3 -translate-y-1/2 text-faint"
        aria-hidden
      >
        <path
          d="M3 4.5 6 7.5 9 4.5"
          fill="none"
          stroke="currentColor"
          strokeWidth="1.5"
          strokeLinecap="round"
          strokeLinejoin="round"
        />
      </svg>
    </div>
  );
}

/** A labelled field, with the hint that stops people guessing. */
export function Field({
  label,
  hint,
  children,
  className = "",
}: {
  label: ReactNode;
  hint?: ReactNode;
  children: ReactNode;
  className?: string;
}) {
  return (
    <label className={`flex flex-col gap-1.5 ${className}`}>
      <span className="text-xs font-medium text-dim">{label}</span>
      {children}
      {hint && <span className="text-xs text-dim">{hint}</span>}
    </label>
  );
}

/** One choice among a few, all visible at once. */
export function Segmented<T extends string>({
  options,
  value,
  onChange,
}: {
  options: readonly T[];
  value: T;
  onChange: (value: T) => void;
}) {
  return (
    <div className="inline-flex rounded-lg border border-line bg-well p-0.5 shadow-card">
      {options.map((option) => (
        <button
          key={option}
          type="button"
          onClick={() => onChange(option)}
          className={`rounded-md px-3 py-1.5 text-sm font-medium transition-colors ${
            option === value ? "bg-card text-white shadow-card" : "text-dim hover:text-white"
          }`}
        >
          {option}
        </button>
      ))}
    </div>
  );
}

export function Stat({
  label,
  value,
  hint,
}: {
  label: string;
  value: ReactNode;
  hint?: ReactNode;
}) {
  return (
    <Card className="px-4 py-4">
      <div className="text-[11px] font-medium tracking-[0.08em] text-faint uppercase">{label}</div>
      <div className="mt-2 truncate font-mono text-2xl leading-none font-semibold text-white tnum">
        {value}
      </div>
      {hint && <div className="mt-2 truncate text-xs text-dim">{hint}</div>}
    </Card>
  );
}

export function PageHeader({
  title,
  hint,
  children,
}: {
  title: ReactNode;
  hint?: ReactNode;
  children?: ReactNode;
}) {
  return (
    <header className="sticky top-0 z-10 flex flex-wrap items-center justify-between gap-3 border-b border-line bg-page/80 px-8 py-4 backdrop-blur-xl">
      <div className="min-w-0">
        <h1 className="truncate text-xl font-semibold tracking-tight text-white">{title}</h1>
        {hint && <p className="mt-0.5 truncate text-sm text-dim">{hint}</p>}
      </div>
      <div className="flex items-center gap-2">{children}</div>
    </header>
  );
}

export function Notice({
  tone = "neutral",
  title,
  children,
}: {
  tone?: "neutral" | "warning" | "error";
  title: ReactNode;
  children?: ReactNode;
}) {
  const tones = {
    neutral: "border-line bg-white/[0.04] text-dim",
    warning: "border-amber-500/30 bg-amber-500/15 text-amber-200",
    error: "border-red-500/30 bg-red-500/15 text-red-200",
  };
  return (
    <div className={`rounded-xl border px-4 py-3 text-sm ${tones[tone]}`}>
      <div className="font-medium">{title}</div>
      {children && <div className="mt-1 opacity-80">{children}</div>}
    </div>
  );
}

/** Shown when Nineveh can't be reached: how to start it. */
export function Offline({ error }: { error: string }) {
  return (
    <div className="mx-auto mt-24 max-w-md px-6 text-center">
      <div className="text-sm font-medium">Studio can't reach Nineveh</div>
      <p className="mt-2 text-sm text-dim">{error}</p>
      <p className="mt-6 text-sm text-dim">Start it, with a Geomi API key and a Postgres:</p>
      <pre className="mt-2 rounded-lg bg-well px-4 py-3 text-left font-mono text-sm text-white">
        nineveh up
      </pre>
      <p className="mt-3 text-xs text-faint">
        Studio talks to <span className="font-mono">{API_URL}</span>. Set{" "}
        <span className="font-mono">NEXT_PUBLIC_NINEVEH_API</span> to change it.
      </p>
    </div>
  );
}

export function Live({ connected }: { connected: boolean }) {
  return (
    <span
      className={`inline-flex items-center gap-1.5 rounded-full px-2 py-0.5 text-xs font-medium ${
        connected ? "bg-emerald-500/15 text-emerald-300" : "bg-well text-dim"
      }`}
    >
      <PhaseDot phase={connected ? "running" : "offline"} />
      {connected ? "Live" : "Connecting"}
    </span>
  );
}

export function OpBadge({ op }: { op: string }) {
  const styles: Record<string, string> = {
    insert: "bg-emerald-500/15 text-emerald-300",
    update: "bg-blue-500/15 text-blue-300",
    delete: "bg-red-500/15 text-red-300",
  };
  return (
    <span className={`rounded px-1.5 py-0.5 font-mono text-[11px] font-medium ${styles[op] ?? ""}`}>
      {op}
    </span>
  );
}

const WIDE: ReadonlySet<ColumnType> = new Set(["u64", "u128", "u256", "i64", "i128", "i256"]);
const NARROW: ReadonlySet<ColumnType> = new Set(["u8", "u16", "u32", "i8", "i16", "i32"]);

export function isNumeric(type: ColumnType): boolean {
  return WIDE.has(type) || NARROW.has(type);
}

/**
 * One value, shown as its column's type reads best. Long values copy on click, unless
 * `plain`: inside something already clickable, where a nested button isn't allowed.
 */
export function Cell({
  type,
  value,
  plain = false,
}: {
  type: ColumnType | "version";
  value: unknown;
  plain?: boolean;
}) {
  const Long = plain ? Truncated : Copyable;
  if (value === null || value === undefined) {
    return <span className="text-ghost">null</span>;
  }
  if (type === "version") {
    return <span className="font-mono text-faint">{String(value)}</span>;
  }
  if (type === "bool") {
    return <span className={value ? "text-emerald-300" : "text-faint"}>{String(value)}</span>;
  }
  if (WIDE.has(type) || NARROW.has(type)) {
    return (
      <span className="font-mono tabular-nums">{formatInteger(value as string | number)}</span>
    );
  }
  if (type === "address" || type === "bytes") {
    return <Long text={String(value)} display={shortHex(String(value))} />;
  }
  if (type === "json") {
    const text = JSON.stringify(value);
    return (
      <Long
        text={JSON.stringify(value, null, 2)}
        display={text.length > 60 ? `${text.slice(0, 60)}…` : text}
      />
    );
  }
  const text = String(value);
  return text.length > 60 ? (
    <Long text={text} display={`${text.slice(0, 60)}…`} />
  ) : (
    <span>{text}</span>
  );
}

function Truncated({ text, display }: { text: string; display: string }) {
  return (
    <span title={text} className="font-mono">
      {display}
    </span>
  );
}

function Copyable({ text, display }: { text: string; display: string }) {
  const [copied, setCopied] = useState(false);
  return (
    <button
      type="button"
      title={copied ? "Copied" : text}
      onClick={() => {
        void navigator.clipboard?.writeText(text).then(() => {
          setCopied(true);
          setTimeout(() => setCopied(false), 1200);
        });
      }}
      className="cursor-copy rounded font-mono text-left hover:text-blue-300"
    >
      {copied ? "copied" : display}
    </button>
  );
}

/** A button in Studio's three tones. */
export function Button({
  tone = "secondary",
  size = "md",
  className = "",
  ...props
}: React.ButtonHTMLAttributes<HTMLButtonElement> & {
  tone?: "primary" | "secondary" | "danger";
  size?: "md" | "lg";
}) {
  const tones = {
    primary:
      "bg-blue-600 text-white shadow-card hover:bg-blue-500 active:bg-blue-700 " +
      "disabled:bg-white/[0.06] disabled:text-faint disabled:shadow-none",
    secondary:
      "border border-line bg-white/[0.06] text-white/85 shadow-card hover:border-edge hover:bg-white/10 hover:text-white",
    danger: "border border-red-500/30 bg-card text-red-300 shadow-card hover:bg-red-500/15",
  };
  const sizes = { md: "px-3 py-1.5 text-sm", lg: "px-4 py-2.5 text-sm" };
  return (
    <button
      type="button"
      className={`inline-flex items-center justify-center gap-1.5 rounded-lg font-medium transition-colors outline-none focus-visible:ring-2 focus-visible:ring-blue-500/50 disabled:cursor-not-allowed ${sizes[size]} ${tones[tone]} ${className}`}
      {...props}
    />
  );
}
