"use client";

// The bar follows the page. It goes dark while a dark stretch is behind it, and carries
// a thin line showing how far down you are — the only chrome that moves on its own.

import { useEffect, useState } from "react";
import { Button, Logo } from "./bits";

export function Nav() {
  const [night, setNight] = useState(false);
  const [progress, setProgress] = useState(0);

  useEffect(() => {
    let frame = 0;
    const read = () => {
      frame = 0;
      const scrollable = document.documentElement.scrollHeight - window.innerHeight;
      setProgress(scrollable > 0 ? Math.min(1, window.scrollY / scrollable) : 0);
      // A dark stretch counts as "behind the bar" only once its top fade has passed and
      // its bottom fade hasn't started, so the bar flips while the ground is truly dark.
      setNight(
        Array.from(document.querySelectorAll(".night")).some((el) => {
          const box = el.getBoundingClientRect();
          return box.top < -80 && box.bottom > 100;
        }),
      );
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
  }, []);

  const link = night ? "text-white/60 hover:text-white" : "text-ink-500 hover:text-ink-900";

  return (
    <nav
      className={`sticky top-0 z-20 border-b backdrop-blur-xl transition-colors duration-500 ${
        night ? "border-white/10 bg-ink-950/70" : "border-ink-200/60 bg-white/72"
      }`}
    >
      <div className="mx-auto flex max-w-6xl items-center justify-between px-6 py-3.5">
        <a
          href="#top"
          className={`flex items-center gap-2 transition-colors duration-500 ${
            night ? "text-white" : "text-ink-900"
          }`}
        >
          <Logo className={`size-5 ${night ? "text-blue-400" : "text-blue-600"}`} />
          <span className="text-[15px] font-semibold tracking-tight">Nineveh</span>
        </a>
        <div className="flex items-center gap-6 text-sm">
          <a href="#how" className={`hidden transition-colors duration-500 sm:block ${link}`}>
            How it works
          </a>
          <a
            href="https://github.com/thewoodfish/Nineveh"
            className={`transition-colors duration-500 ${link}`}
          >
            GitHub
          </a>
          <Button href="https://github.com/thewoodfish/Nineveh">Get started</Button>
        </div>
      </div>
      <div
        className="h-px origin-left bg-gradient-to-r from-blue-500 to-blue-300 transition-transform duration-150"
        style={{ transform: `scaleX(${progress})` }}
        aria-hidden
      />
    </nav>
  );
}
