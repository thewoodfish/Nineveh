"use client";

// The docs chrome: which document you're in on the left, where you are inside it on the
// right. Both rails are the same idea as the landing page's — a margin that tells you
// your position — so the site has one navigational metaphor rather than two.

import { useEffect, useState } from "react";

import type { Doc, Entry } from "@/lib/docs";
import { GROUPS, type Group } from "@/lib/groups";

export function DocsNav({
  docs,
  current,
}: {
  docs: { slug: string; title: string; href: string; group: Group }[];
  current: string;
}) {
  return (
    <nav aria-label="Documentation" className="flex flex-col gap-7 text-[13.5px]">
      {GROUPS.map((group) => {
        const inGroup = docs.filter((d) => d.group === group);
        if (inGroup.length === 0) return null;
        return (
          <div key={group}>
            {/* The section's name, not a label above a heading: these are the four
                parts of the documentation, and a reader scanning for "React" is
                scanning for exactly this word. */}
            <h2 className="px-3 pb-2 text-[11.5px] font-semibold tracking-[0.04em] text-ink-faint uppercase">
              {group}
            </h2>
            <ul className="flex flex-col">
              {inGroup.map((doc) => {
                const here = doc.slug === current;
                return (
                  <li key={doc.slug}>
                    <a
                      href={doc.href}
                      aria-current={here ? "page" : undefined}
                      className={`block rounded-lg px-3 py-[7px] transition-colors ${
                        here
                          ? "bg-accent-wash font-medium text-accent"
                          : "text-ink-soft hover:bg-ink-wash hover:text-ink-strong"
                      }`}
                    >
                      {doc.title}
                    </a>
                  </li>
                );
              })}
            </ul>
          </div>
        );
      })}
    </nav>
  );
}

/** The right rail: the section you're reading and what's inside it. */
export function OnThisPage({ headings }: { headings: Entry[] }) {
  const active = useActiveHeading(headings);
  if (headings.length === 0) return null;
  return (
    <nav aria-label="On this page" className="text-sm">
      <p className="pb-3 text-xs font-medium text-ink-faint">On this page</p>
      <ul className="flex flex-col border-l border-ink-line">
        {headings.map((h) => (
          <li key={h.id}>
            <a
              href={`#${h.id}`}
              className={`-ml-px block border-l py-1.5 transition-colors ${
                h.level === 3 ? "pl-7 text-[12.5px]" : "pl-4 text-[13px]"
              } ${
                active === h.id
                  ? "border-accent font-medium text-accent"
                  : "border-transparent text-ink-faint hover:border-ink-line-strong hover:text-ink-strong"
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