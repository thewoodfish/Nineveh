"use client";

// What a rule change costs from the outside: a stale read. Two tracks, because two tables
// exist for a while — the one still answering requests and the one being rebuilt beside
// it. The rebuild reads Nineveh's own record log rather than the chain, which is why it's
// minutes rather than another backfill, and why the cap on the free tier is a log size
// rather than a time limit.
//
// The shadow bar fills once, when the card is first scrolled into view, and then stays
// full: a progress bar on a loop would say Nineveh is permanently rebuilding something.
//
// This was a scroll-driven CSS animation first (`animation-timeline`, as `.rise` uses).
// It doesn't work for a bar 8px tall: `view()` takes the animated element as its subject,
// so the whole `entry` phase is 8px of scrolling and the fill resolves as finished before
// it is ever on screen. Naming a timeline on the card is the documented fix and it stayed
// inactive in testing, so this observes the card instead and transitions the width, which
// is the same effect with none of the subtlety.

import { useEffect, useRef, useState, type ReactNode } from "react";

export function Rebuild() {
  const card = useRef<HTMLDivElement>(null);
  const [built, setBuilt] = useState(false);

  useEffect(() => {
    const node = card.current;
    if (!node) return;
    if (window.matchMedia("(prefers-reduced-motion: reduce)").matches) {
      setBuilt(true);
      return;
    }
    const watch = new IntersectionObserver(
      ([entry]) => {
        if (!entry?.isIntersecting) return;
        setBuilt(true);
        watch.disconnect();
      },
      { threshold: 0.45 },
    );
    watch.observe(node);
    return () => watch.disconnect();
  }, []);

  return (
    <figure>
      <div
        ref={card}
        className="rounded-2xl border border-white/10 bg-white/[0.025] p-6 sm:p-8"
      >
        <Track
          label="sellers"
          version="v1"
          note="serving, not advancing"
          bar={<div className="h-full w-full rounded-full bg-white/25" />}
        />
        <Track
          label="sellers"
          version="v2"
          note="folding your record log"
          lit
          bar={
            <div
              className="h-full rounded-full bg-blue-400/80 transition-[width] duration-[1600ms] ease-out"
              style={{ width: built ? "100%" : "0%" }}
            />
          }
        />
        <div className="mt-5 flex items-center justify-end gap-2 border-t border-white/10 pt-4">
          <span
            className={`font-mono text-[11px] transition-colors duration-500 ${
              built ? "text-white/45" : "text-white/20"
            }`}
          >
            v2 swaps in
          </span>
          <span
            className={`size-1.5 rounded-full transition-colors duration-500 ${
              built ? "bg-blue-400" : "bg-white/15"
            }`}
            aria-hidden
          />
        </div>
      </div>
      <figcaption className="mt-4 text-sm leading-relaxed text-white/45">
        Nobody is locked out and the history isn&apos;t re-streamed. The served table does go
        stale while the rebuild runs, and its status says so.
      </figcaption>
    </figure>
  );
}

/** One table's lifetime as a bar: which table, which build, what it's doing. */
function Track({
  label,
  version,
  note,
  bar,
  lit = false,
}: {
  label: string;
  version: string;
  note: string;
  bar: ReactNode;
  lit?: boolean;
}) {
  return (
    <div className="flex items-center gap-4 py-3">
      <div className="w-28 shrink-0 sm:w-36">
        <div className="font-mono text-[12px] text-white/70">
          {label} <span className={lit ? "text-blue-300" : "text-white/35"}>{version}</span>
        </div>
        <div className="mt-0.5 text-[11px] text-white/35">{note}</div>
      </div>
      {/* No `overflow-hidden`: the bar rounds its own ends, and a clipping parent here
          was what broke the CSS-timeline version. */}
      <div className="h-2 flex-1 rounded-full bg-white/[0.07]">{bar}</div>
    </div>
  );
}
