"use client";

// The bar. It carries a line showing how far down the page you are, and lifts off the
// page once you've left the hero — the only chrome that moves on its own.

import { useEffect, useState } from "react";
import { Button, Logo } from "./bits";

/** The bar's links. `docs` sends you out of the landing page; the rest are anchors. */
const LINKS = [
  { label: "How it works", href: "/#how", anchor: true },
  { label: "Pricing", href: "/#pricing", anchor: true },
  { label: "Docs", href: "/docs", anchor: false },
];

export function Nav() {
  const [progress, setProgress] = useState(0);
  const [lifted, setLifted] = useState(false);

  useEffect(() => {
    let frame = 0;
    const read = () => {
      frame = 0;
      const scrollable = document.documentElement.scrollHeight - window.innerHeight;
      setProgress(scrollable > 0 ? Math.min(1, window.scrollY / scrollable) : 0);
      setLifted(window.scrollY > 24);
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

  return (
    <nav
      className={`sticky top-0 z-20 border-b transition-colors duration-500 ${
        lifted ? "glass border-white/10" : "border-transparent"
      }`}
    >
      <div className="mx-auto flex max-w-6xl items-center justify-between px-6 py-3.5">
        <a href="/" className="flex items-center gap-2 text-white">
          <Logo className="size-5 text-blue-400" />
          <span className="text-[15px] font-semibold tracking-tight">Nineveh</span>
        </a>
        <div className="flex items-center gap-6 text-sm">
          {LINKS.map((link) => (
            <a
              key={link.href}
              href={link.href}
              className={`text-white/55 transition-colors hover:text-white ${
                link.anchor ? "hidden sm:block" : ""
              }`}
            >
              {link.label}
            </a>
          ))}
          <a
            href="https://github.com/thewoodfish/Nineveh"
            className="hidden text-white/55 transition-colors hover:text-white sm:block"
          >
            GitHub
          </a>
          <Button href="/docs">Start building</Button>
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
