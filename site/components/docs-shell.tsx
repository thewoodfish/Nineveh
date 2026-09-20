"use client";

// The docs chrome: which document you're in on the left, where you are inside it on the
// right. Both rails are the same idea as the landing page's — a margin that tells you
// your position — so the site has one navigational metaphor rather than two.

import { useEffect, useState } from "react";

import type { Doc, Entry } from "@/lib/docs";

export function DocsNav({
  docs,
  current,
  headings,
}: {
  docs: { slug: string; title: string; blurb: string; href: string }[];
  current: string;
  headings: Entry[];
}) {
  const active = useActiveHeading(headings);
  return (
    <nav aria-label="Documentation" className="flex flex-col gap-7 text-sm">
      <ul className="flex flex-col gap-0.5">
        {docs.map((doc) => {
          const here = doc.slug === current;
          return (
            <li key={doc.slug}>
              <a
                href={doc.href}
                aria-current={here ? "page" : undefined}
                className={`block rounded-lg px-3 py-2 transition-colors ${
                  here
                    ? "bg-white/[0.07] text-white"
                    : "text-white/55 hover:bg-white/[0.04] hover:text-white"
                }`}
              >
                <span className="font-medium">{doc.title}</span>
                <span className="mt-0.5 block text-xs text-white/35">{doc.blurb}</span>
              </a>
            </li>
          );
        })}
      </ul>
    </nav>
  );
}

/** The right rail: the section you're reading and what's inside it. */
export function OnThisPage({ headings }: { headings: Entry[] }) {
  const active = useActiveHeading(headings);
  if (headings.length === 0) return null;
  return (
    <nav aria-label="On this page" className="text-sm">
      <p className="pb-3 text-xs font-medium text-white/40">On this page</p>
      <ul class="flex flex-col border-l border-white/10">
        {headings.map((h) => (
          <li key={h.id}>
            <a
              href={`#${h.id}`}
              className={`-ml-px block border-l py-1.5 transition-colors ${
                h.level === 3 ? "pl-7 text-[12.5px]" : "pl-4 text-[13px]"
              } ${
                active === h.id
                  ? "border-blue-400 text-blue-200"
                  : "border-transparent text-white/45 hover:border-white/25 hover:text-white/80"
              }`}
            >
              {h.text}
            </a>
          </li>
        ))}
      </ul>
    </nav>
  );
}

/**
 * The heading you're currently under: the last one whose top has passed the reading
 * line, a third of the way down the viewport. An IntersectionObserver alone reports
 * headings entering and leaving, which loses track whenever a long section fills the
 * screen and no heading is visible at all.
 */
function useActiveHeading(headings: Entry[]): string | null {
  const [active, setActive] = useState<string | null>(null);
  const ids = headings.map((h) => h.id).join(",");

  useEffect(() => {
    const list = ids ? ids.split(",") : [];
    if (list.length === 0) return;
    let frame = 0;
    const read = () => {
      frame = 0;
      const line = window.innerHeight * 0.3;
      let current = list[0] ?? null;
      for (const id of list) {
        const top = document.getElementById(id)?.getBoundingClientRect().top;
        if (top !== undefined && top <= line) current = id;
      }
      // At the very bottom the last heading wins, however short its section is.
      if (window.innerHeight + window.scrollY >= document.body.scrollHeight - 2) {
        current = list[list.length - 1] ?? current;
      }
      setActive(current);
    };
    const onScroll = () => {
      if (!frame) frame = requestAnimationFrame(read);
    };
    read();
    window.addEventListener("scroll", onScroll, { passive: true });
    window.addEventListener("resize", onScroll);
    return () => {
      window.removeEventListener("scroll", onScroll);
      window.removeEventListener("resize", onScroll);
      if (frame) cancelAnimationFrame(frame);
    };
  }, [ids]);

  return active;
}

export type { Doc };