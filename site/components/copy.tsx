"use client";

// The click behind the docs' copy buttons.
//
// The buttons themselves are static HTML from `lib/docs.ts`, so this component renders
// nothing and only listens. It listens on the document rather than per button: a long
// page carries thirty snippets, and one listener is cheaper to install and impossible
// to leave half-attached.
//
// If the clipboard refuses, which it does in an insecure context and can do on a
// permission prompt, the snippet is selected instead. That is the same keystroke the
// reader would have reached for anyway, and it beats a tick that lied.

import { useEffect } from "react";

/** How long the tick stays up: long enough to read, short enough not to look stuck. */
const SETTLE = 1600;

export function SnippetCopy() {
  useEffect(() => {
    const timers = new Set<ReturnType<typeof setTimeout>>();

    const confirm = (button: HTMLElement) => {
      button.dataset.copied = "true";
      button.setAttribute("aria-label", "Copied");
      const timer = setTimeout(() => {
        delete button.dataset.copied;
        button.setAttribute("aria-label", "Copy");
        timers.delete(timer);
      }, SETTLE);
      timers.add(timer);
    };

    const select = (code: Element) => {
      const selection = window.getSelection();
      if (!selection) return;
      const range = document.createRange();
      range.selectNodeContents(code);
      selection.removeAllRanges();
      selection.addRange(range);
    };

    const onClick = (event: MouseEvent) => {
      const button = (event.target as Element | null)?.closest?.("button.copy");
      if (!(button instanceof HTMLButtonElement)) return;
      const code = button.closest(".snippet")?.querySelector("code");
      if (!code) return;
      // `textContent` is the snippet as the reader sees it: nothing to unescape, and no
      // prompt characters, because none were ever added. The trailing newline goes,
      // though: pasted into a shell it would run the command before they meant it to.
      const text = (code.textContent ?? "").replace(/\n+$/, "");
      void (async () => {
        try {
          await navigator.clipboard.writeText(text);
          confirm(button);
        } catch {
          select(code);
        }
      })();
    };

    document.addEventListener("click", onClick);
    return () => {
      document.removeEventListener("click", onClick);
      for (const timer of timers) clearTimeout(timer);
    };
  }, []);

  return null;
}
