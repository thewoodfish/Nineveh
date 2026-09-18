"use client";

// Light, dark, or whatever the machine says. The choice lives in localStorage and is
// stamped on <html> as `data-theme`; "system" stamps nothing, so the media query in
// globals.css stays in charge. A script in the document head applies the stored value
// before first paint, so this component never causes a flash — it only changes it.

import { useEffect, useState } from "react";

import { Icon } from "./ui";

export type Theme = "light" | "dark" | "system";

const KEY = "nineveh-theme";
const ORDER: Theme[] = ["system", "light", "dark"];
const ICONS: Record<Theme, string> = {
  system: "brightness_auto",
  light: "light_mode",
  dark: "dark_mode",
};
const LABELS: Record<Theme, string> = {
  system: "Theme: follows your system",
  light: "Theme: light",
  dark: "Theme: dark",
};

function apply(theme: Theme) {
  if (theme === "system") delete document.documentElement.dataset.theme;
  else document.documentElement.dataset.theme = theme;
  try {
    if (theme === "system") localStorage.removeItem(KEY);
    else localStorage.setItem(KEY, theme);
  } catch {
    // Private windows and blocked site data: the choice just doesn't outlive the tab.
  }
}

export function ThemeToggle() {
  // The server has no idea which theme this is, so the first client render has to
  // match it and correct itself immediately afterwards.
  const [theme, setTheme] = useState<Theme>("system");
  const [ready, setReady] = useState(false);

  useEffect(() => {
    const stored = (() => {
      try {
        return localStorage.getItem(KEY);
      } catch {
        return null;
      }
    })();
    if (stored === "light" || stored === "dark") setTheme(stored);
    setReady(true);
  }, []);

  const next = () => {
    const chosen = ORDER[(ORDER.indexOf(theme) + 1) % ORDER.length] ?? "system";
    setTheme(chosen);
    apply(chosen);
  };

  return (
    <button
      type="button"
      onClick={next}
      title={LABELS[theme]}
      aria-label={LABELS[theme]}
      className="state grid size-10 shrink-0 place-items-center rounded-full text-on-surface-variant"
    >
      <Icon name={ready ? ICONS[theme] : "brightness_auto"} className="text-[20px]" />
    </button>
  );
}
