"use client";

// Light or dark, for the documentation only.
//
// Light is the default, and the choice is remembered. It is set on `<html>` rather than
// on a wrapper so the stylesheet can switch tokens in one place, and written by the
// inline script in the docs layout before first paint — a theme applied in an effect
// shows the wrong one for a frame, which on a whole page of text is a flash nobody
// should have to see.

import { useEffect, useState } from "react";

export const THEME_KEY = "nineveh-docs-theme";

/**
 * Sets the stored theme before the page paints. Rendered as an inline script, so it
 * runs ahead of React and ahead of the first paint. Falls back to light — both when
 * nothing is stored and when storage throws, which is what a private window does.
 */
export const THEME_SCRIPT = `(function(){try{var t=localStorage.getItem(${JSON.stringify(
  THEME_KEY,
)});document.documentElement.dataset.theme=t==="dark"?"dark":"light"}catch(e){document.documentElement.dataset.theme="light"}})()`;

export function ThemeToggle() {
  // Starts as null so the first render matches what the server sent, whatever the
  // script already put on `<html>`; the effect below reads the real value.
  const [theme, setTheme] = useState<"light" | "dark" | null>(null);

  useEffect(() => {
    const current = document.documentElement.dataset.theme === "dark" ? "dark" : "light";
    setTheme(current);
  }, []);

  const flip = () => {
    const next = theme === "dark" ? "light" : "dark";
    setTheme(next);
    document.documentElement.dataset.theme = next;
    try {
      localStorage.setItem(THEME_KEY, next);
    } catch {
      // A private window keeps the choice for this page and forgets it after. Better
      // than refusing to switch at all.
    }
  };

  const dark = theme === "dark";
  return (
    <button
      type="button"
      onClick={flip}
      // Named by what pressing it does, not by what is showing.
      aria-label={dark ? "Switch to light" : "Switch to dark"}
      title={dark ? "Switch to light" : "Switch to dark"}
      className="grid size-8 place-items-center rounded-lg text-ink-faint transition-colors hover:bg-ink-wash hover:text-ink-strong"
    >
      {/* One glyph, drawn both ways: the sun's rays retract into the moon's bite. */}
      <svg viewBox="0 0 20 20" className="size-[17px]" aria-hidden>
        {dark ? (
          <path
            fill="currentColor"
            d="M16.3 12.6A6.8 6.8 0 0 1 7.4 3.7a1 1 0 0 0-1.3-1.3 8.8 8.8 0 1 0 11.5 11.5 1 1 0 0 0-1.3-1.3Z"
          />
        ) : (
          <g fill="none" stroke="currentColor" strokeWidth="1.6" strokeLinecap="round">
            <circle cx="10" cy="10" r="3.4" />
            <path d="M10 2.2v1.6M10 16.2v1.6M17.8 10h-1.6M3.8 10H2.2M15.5 4.5l-1.1 1.1M5.6 14.4l-1.1 1.1M15.5 15.5l-1.1-1.1M5.6 5.6 4.5 4.5" />
          </g>
        )}
      </svg>
    </button>
  );
}
