"use client";

import { useState, type ReactNode } from "react";

import { API_URL, type ColumnType } from "@/lib/api";
import { formatInteger, shortHex } from "@/lib/format";

const PHASES: Record<string, { label: string; dot: string; pulse?: boolean }> = {
  running: { label: "Running", dot: "bg-tertiary", pulse: true },
  serving: { label: "Serving", dot: "bg-tertiary" },
  starting: { label: "Starting", dot: "bg-primary", pulse: true },
  retrying: { label: "Retrying", dot: "bg-warning", pulse: true },
  halted: { label: "Halted", dot: "bg-error" },
  failed: { label: "Failed", dot: "bg-error" },
  stopped: { label: "Stopped", dot: "bg-outline" },
  offline: { label: "Offline", dot: "bg-outline-variant" },
};

export function PhaseDot({ phase, label = false }: { phase: string; label?: boolean }) {
  const p = PHASES[phase] ?? { label: phase, dot: "bg-outline" };
  return (
    <span
      className="inline-flex items-center gap-1.5 text-xs text-on-surface-variant"
      title={p.label}
    >
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

/**
 * Material's filled card: a surface container lifted by *tone* rather than by a shadow.
 * Shadows are kept for things that genuinely float — menus, dialogs, the app bar once
 * the page scrolls under it.
 */
export function Card({ children, className = "" }: { children: ReactNode; className?: string }) {
  return <div className={`rounded-sm bg-surface-container-low ${className}`}>{children}</div>;
}

/**
 * The filled button's look as a bare class, for the few places the control has to be a
 * link rather than a button. `on-primary` is the point: a filled primary is dark blue
 * in light and light blue in dark, so its label can never be the page's text colour.
 */
export const filledButton =
  "state inline-flex h-10 items-center justify-center gap-2 rounded-xl bg-primary px-6 " +
  "text-sm font-medium text-on-primary shadow-e1";

/** A Material Symbol. One font, one name, the same optical size everywhere. */
export function Icon({
  name,
  filled = false,
  className = "",
}: {
  name: string;
  filled?: boolean;
  className?: string;
}) {
  return (
    <span aria-hidden className={`symbol ${filled ? "symbol-filled" : ""} ${className}`}>
      {name}
    </span>
  );
}

/**
 * The shared shape of anything typed into: inputs, selects, textareas. Focus is a
 * blue ring rather than a border colour, so a field doesn't shift when you click it.
 */
export const field =
  "rounded-xs border border-outline bg-transparent px-3 py-2 text-sm text-on-surface outline-none " +
  "transition-colors placeholder:text-on-surface-variant/60 hover:border-on-surface " +
  "focus:border-primary focus:ring-1 focus:ring-primary disabled:border-outline-variant";

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
        className={`${field} w-full cursor-pointer appearance-none pr-9 ${className}`}
        {...props}
      >
        {children}
      </select>
      <Icon
        name="arrow_drop_down"
        className="pointer-events-none absolute top-1/2 right-1.5 -translate-y-1/2 text-[20px] text-on-surface-variant"
      />
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
      <span className="text-xs font-medium text-on-surface-variant">{label}</span>
      {children}
      {hint && <span className="text-xs text-on-surface-variant">{hint}</span>}
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
    <div className="inline-flex divide-x divide-outline overflow-hidden rounded-sm border border-outline">
      {options.map((option) => (
        <button
          key={option}
          type="button"
          onClick={() => onChange(option)}
          aria-pressed={option === value}
          className={`state inline-flex items-center gap-1.5 px-3.5 py-1.5 text-sm font-medium ${
            option === value
              ? "bg-secondary-container text-on-secondary-container"
              : "text-on-surface-variant"
          }`}
        >
          {option === value && <Icon name="check" className="text-[16px]" />}
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
      <div className="truncate text-xs font-medium text-on-surface-variant">{label}</div>
      <div className="mt-2 truncate font-mono text-[28px] leading-8 text-on-surface tnum">
        {value}
      </div>
      {hint && <div className="mt-1.5 truncate text-xs text-on-surface-variant">{hint}</div>}
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
    <header className="sticky top-0 z-10 flex flex-wrap items-center justify-between gap-3 bg-surface px-6 py-3">
      <div className="min-w-0">
        <h1 className="truncate text-[22px] leading-7 text-on-surface">{title}</h1>
        {hint && <p className="mt-0.5 truncate text-sm text-on-surface-variant">{hint}</p>}
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
  // Material has no alert component, so the console states things in a tonal container
  // with the matching symbol. That is what this is.
  const tones = {
    neutral: "bg-surface-container-high text-on-surface",
    warning: "bg-warning-container text-on-warning-container",
    error: "bg-error-container text-on-error-container",
  };
  const icons = { neutral: "info", warning: "warning", error: "error" };
  return (
    <div className={`flex gap-3 rounded-sm px-4 py-3 text-sm ${tones[tone]}`}>
      <Icon name={icons[tone]} className="mt-px shrink-0 text-[20px]" />
      <div className="min-w-0">
        <div className="font-medium">{title}</div>
        {children && <div className="mt-1 opacity-90">{children}</div>}
      </div>
    </div>
  );
}

/** Shown when Nineveh can't be reached: how to start it. */
export function Offline({ error }: { error: string }) {
  return (
    <div className="mx-auto mt-24 max-w-md px-6 text-center">
      <Icon name="cloud_off" className="text-[40px] text-on-surface-variant" />
      <div className="mt-4 text-lg text-on-surface">Studio can't reach Nineveh</div>
      <p className="mt-2 text-sm text-on-surface-variant">{error}</p>
      <p className="mt-6 text-sm text-on-surface-variant">
        Start it, with a Geomi API key and a Postgres:
      </p>
      <pre className="mt-2 rounded-sm bg-surface-container-high px-4 py-3 text-left font-mono text-sm text-on-surface">
        nineveh up
      </pre>
      <p className="mt-3 text-xs text-on-surface-variant">
        Studio talks to <span className="font-mono">{API_URL}</span>. Set{" "}
        <span className="font-mono">NEXT_PUBLIC_NINEVEH_API</span> to change it.
      </p>
    </div>
  );
}

export function Live({ connected }: { connected: boolean }) {
  return (
    <span
      className={`inline-flex items-center gap-1.5 rounded-sm px-2 py-1 text-xs font-medium ${
        connected
          ? "bg-tertiary-container text-on-tertiary-container"
          : "bg-surface-container-high text-on-surface-variant"
      }`}
    >
      <PhaseDot phase={connected ? "running" : "offline"} />
      {connected ? "Live" : "Connecting"}
    </span>
  );
}

export function OpBadge({ op }: { op: string }) {
  const styles: Record<string, string> = {
    insert: "bg-tertiary-container text-on-tertiary-container",
    update: "bg-secondary-container text-on-secondary-container",
    delete: "bg-error-container text-on-error-container",
  };
  return (
    <span
      className={`rounded-xs px-1.5 py-0.5 font-mono text-[11px] font-medium ${styles[op] ?? ""}`}
    >
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
    return <span className="text-on-surface-variant/50">null</span>;
  }
  if (type === "version") {
    return <span className="font-mono text-on-surface-variant">{String(value)}</span>;
  }
  if (type === "bool") {
    return (
      <span className={value ? "text-tertiary" : "text-on-surface-variant"}>{String(value)}</span>
    );
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
      className="cursor-copy rounded-xs font-mono text-left hover:text-primary"
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
  tone?: "primary" | "tonal" | "secondary" | "danger" | "text";
  size?: "md" | "lg";
}) {
  // Material's common buttons, in the order it ranks them: filled for the one action
  // the screen is for, tonal for the next most important, outlined and text for the
  // rest. Each is a pill, and each takes its hover and press from the shared state
  // layer rather than from a colour of its own.
  const tones = {
    primary: "bg-primary text-on-primary shadow-e1",
    tonal: "bg-secondary-container text-on-secondary-container",
    secondary: "border border-outline text-primary",
    danger: "border border-outline text-error",
    text: "text-primary",
  };
  const sizes = { md: "h-9 px-4 text-sm", lg: "h-10 px-6 text-sm" };
  return (
    <button
      type="button"
      className={`state inline-flex items-center justify-center gap-2 rounded-sm font-medium transition-colors disabled:cursor-not-allowed disabled:border-on-surface/12 disabled:bg-on-surface/12 disabled:text-on-surface/38 disabled:shadow-none ${sizes[size]} ${tones[tone]} ${className}`}
      {...props}
    />
  );
}

/** A button that is only a symbol: Material's icon button, round and 40px. */
export function IconButton({
  name,
  filled = false,
  className = "",
  ...props
}: React.ButtonHTMLAttributes<HTMLButtonElement> & { name: string; filled?: boolean }) {
  return (
    <button
      type="button"
      className={`state grid size-10 shrink-0 place-items-center rounded-full text-on-surface-variant disabled:text-on-surface/38 ${className}`}
      {...props}
    >
      <Icon name={name} filled={filled} className="text-[20px]" />
    </button>
  );
}
