"use client";

// The documentation's own bar.
//
// Not the landing page's `Nav`, which carries a scroll-progress line, a "Start
// building" call to action and a set of marketing anchors — all of which are answers to
// a question a reader in the docs has already stopped asking. This one holds the three
// things that are useful while reading: where you are, how to search, and how to get
// back out to the product.

import { Logo } from "./bits";
import { ThemeToggle } from "./theme";
import { SearchTrigger } from "./search";

export function DocsHeader() {
  return (
    <header className="sticky top-0 z-30 border-b border-ink-line bg-ground/85 backdrop-blur-md">
      <div className="mx-auto flex h-14 max-w-[88rem] items-center gap-4 px-6">
        <a href="/" className="flex shrink-0 items-center gap-2 text-ink">
          <Logo className="size-[18px] text-accent" />
          <span className="text-[15px] font-semibold tracking-tight">Nineveh</span>
          <span className="hidden text-[15px] text-ink-faint sm:inline">docs</span>
        </a>

        <div className="mx-auto w-full max-w-sm">
          <SearchTrigger />
        </div>

        <nav className="flex shrink-0 items-center gap-1 text-[13.5px]">
          <a
            href="https://studio.nineveh.dev"
            className="hidden rounded-lg px-2.5 py-1.5 text-ink-soft transition-colors hover:bg-ink-wash hover:text-ink-strong sm:block"
          >
            Studio
          </a>
          <a
            href="https://github.com/thewoodfish/Nineveh"
            target="_blank"
            rel="noreferrer"
            aria-label="Nineveh on GitHub"
            title="Nineveh on GitHub"
            className="grid size-8 place-items-center rounded-lg text-ink-faint transition-colors hover:bg-ink-wash hover:text-ink-strong"
          >
            <svg viewBox="0 0 16 16" className="size-[17px]" aria-hidden>
              <path
                fill="currentColor"
                d="M8 0C3.58 0 0 3.58 0 8c0 3.54 2.29 6.53 5.47 7.59.4.07.55-.17.55-.38 0-.19-.01-.82-.01-1.49-2.01.37-2.53-.49-2.69-.94-.09-.23-.48-.94-.82-1.13-.28-.15-.68-.52-.01-.53.63-.01 1.08.58 1.23.82.72 1.21 1.87.87 2.33.66.07-.52.28-.87.51-1.07-1.78-.2-3.64-.89-3.64-3.95 0-.87.31-1.59.82-2.15-.08-.2-.36-1.02.08-2.12 0 0 .67-.21 2.2.82.64-.18 1.32-.27 2-.27.68 0 1.36.09 2 .27 1.53-1.04 2.2-.82 2.2-.82.44 1.1.16 1.92.08 2.12.51.56.82 1.27.82 2.15 0 3.07-1.87 3.75-3.65 3.95.29.25.54.73.54 1.48 0 1.07-.01 1.93-.01 2.2 0 .21.15.46.55.38A8.013 8.013 0 0016 8c0-4.42-3.58-8-8-8z"
              />
            </svg>
          </a>
          <ThemeToggle />
        </nav>
      </div>
    </header>
  );
}
