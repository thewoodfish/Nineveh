"use client";

// The click behind the package-manager tabs.
//
// Like the copy buttons, the tabs themselves are static HTML from `lib/docs.ts` and
// this renders nothing. One listener on the document drives every block on the page,
// and picking a manager picks it everywhere at once — on this page and the next one,
// because nobody wants to say "pnpm" eleven times on their way through a guide.

import { useEffect } from "react";

const KEY = "nineveh-docs-pm";

export function PackageTabs() {
  useEffect(() => {
    const choose = (manager: string) => {
      for (const block of document.querySelectorAll<HTMLElement>(".snippet.pm")) {
        // A block only offers the managers it could derive, so a choice it doesn't
        // have leaves it where it was rather than blanking it.
        if (!block.querySelector(`.pm-pane[data-pm="${CSS.escape(manager)}"]`)) continue;
        for (const part of block.querySelectorAll<HTMLElement>(".pm-tab, .pm-pane")) {
          if (part.dataset.pm === manager) part.dataset.on = "";
          else delete part.dataset.on;
        }
      }
    };

    try {
      const stored = localStorage.getItem(KEY);
      if (stored) choose(stored);
    } catch {
      // No storage, no memory. The npm pane the HTML shipped with is still correct.
    }

    const onClick = (event: MouseEvent) => {
      const tab = (event.target as Element | null)?.closest<HTMLElement>(".pm-tab");
      const manager = tab?.dataset.pm;
      if (!manager) return;
      choose(manager);
      try {
        localStorage.setItem(KEY, manager);
      } catch {
        // Kept for this page only.
      }
    };

    document.addEventListener("click", onClick);
    return () => document.removeEventListener("click", onClick);
  }, []);

  return null;
}
