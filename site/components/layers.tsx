// Where Nineveh sits, drawn twice.
//
// Both drawings answer the question an Aptos developer asks in the first ten seconds —
// "is this instead of the tooling I already use, or on top of it?" — and the answer is on
// top. RPC, the Indexer and the Transaction Stream stay exactly where they are. What
// Nineveh takes is the application-backend work every team otherwise assembles above
// them.
//
// `Pipeline` is the path a value takes from a contract to a frontend, with the stretch
// Nineveh covers bracketed. `Stack` is the same claim as a hierarchy: a layer added, not
// a layer replaced. Both are markup rather than images, so they stay legible at any
// width, carry their own text, and can't go stale against a screenshot.

import type { ReactNode } from "react";

/** One rung of the path. `lit` is for the two ends: the developer's own code. */
function Rung({
  children,
  lit = false,
  first = false,
}: {
  children: ReactNode;
  lit?: boolean;
  first?: boolean;
}) {
  return (
    <li className="relative">
      {!first && (
        <span
          className="absolute -top-1.5 left-1/2 h-1.5 w-px -translate-x-1/2 bg-ink-line-strong"
          aria-hidden
        />
      )}
      <div
        className={`rounded-lg px-3.5 py-2 text-center text-[13px] ${
          lit ? "bg-ink-wash-strong font-medium text-ink-strong" : "bg-ink-wash text-ink-soft"
        }`}
      >
        {children}
      </div>
    </li>
  );
}

/** The link between two groups: a longer line, so the bracket reads as a stretch. */
function Link() {
  return <span className="mx-auto my-1.5 block h-4 w-px bg-ink-line-strong" aria-hidden />;
}

export function Pipeline() {
  return (
    <figure className="mx-auto w-full max-w-[19rem]">
      <ul className="flex flex-col gap-1.5">
        <Rung first lit>
          Move contracts
        </Rung>
        <Rung>Events and state changes</Rung>
        <Rung>RPC · Indexer · Transaction Stream</Rung>
      </ul>

      <Link />

      {/* The middle, bracketed: the part that is the same work in every app. */}
      <div className="relative rounded-xl border border-blue-400/35 bg-blue-500/[0.07] p-3 pt-8">
        <span className="absolute top-2.5 left-3.5 font-mono text-[10px] font-semibold tracking-[0.06em] text-accent">
          NINEVEH
        </span>
        <ul className="flex flex-col gap-1.5">
          <Rung first>Application logic</Rung>
          <Rung>Database</Rung>
          <Rung>API</Rung>
        </ul>
      </div>

      <Link />

      <ul>
        <Rung first lit>
          Your frontend
        </Rung>
      </ul>

      <figcaption className="mt-4 text-center text-sm text-ink-faint">
        Nineveh simplifies the layer in the middle.
      </figcaption>
    </figure>
  );
}

/** The bands of the stack, top to bottom: your app down to the contract. */
const BANDS = [
  { label: "Your application", note: "web, mobile, agents" },
  { label: "Nineveh", note: "application backend", lit: true },
  {
    label: "Aptos data infrastructure",
    parts: ["RPC", "Indexer", "Transaction Stream"],
  },
  { label: "Aptos", note: "consensus, finality, storage" },
  { label: "Move contracts", note: "your source of truth" },
];

function Down() {
  return (
    <svg viewBox="0 0 12 12" className="mx-auto my-2 h-2.5 w-3 text-ink-faint" aria-hidden>
      <path
        fill="none"
        stroke="currentColor"
        strokeWidth="1.5"
        strokeLinecap="round"
        strokeLinejoin="round"
        d="M6 1v8M2.5 6.5L6 10l3.5-3.5"
      />
    </svg>
  );
}

export function Stack() {
  return (
    <figure className="mx-auto w-full max-w-xl">
      {BANDS.map((band, i) => (
        <div key={band.label}>
          {i > 0 && <Down />}
          <div
            className={`flex flex-wrap items-center justify-between gap-x-4 gap-y-2 rounded-xl px-5 py-4 ${
              band.lit
                ? "border border-blue-400/40 bg-blue-500/10 shadow-glow"
                : "border border-ink-line bg-ink-wash"
            }`}
          >
            <span
              className={`text-sm font-semibold ${band.lit ? "text-ink" : "text-ink-body"}`}
            >
              {band.label}
            </span>
            {band.note && (
              <span className={`text-xs ${band.lit ? "text-accent/80" : "text-ink-faint"}`}>
                {band.note}
              </span>
            )}
            {band.parts && (
              <span className="flex flex-wrap gap-1.5">
                {band.parts.map((part) => (
                  <span
                    key={part}
                    className="rounded-md bg-ink-wash-strong px-2 py-1 font-mono text-[11px] text-ink-soft"
                  >
                    {part}
                  </span>
                ))}
              </span>
            )}
          </div>
        </div>
      ))}
      <figcaption className="mt-6 text-center text-sm text-ink-faint">
        Nineveh adds a layer. It doesn&apos;t replace the layers underneath it.
      </figcaption>
    </figure>
  );
}

