"use client";

// Studio, photographed rather than described. These are real screens against real
// testnet data — a perpetuals DEX and the framework's token transfers — so the numbers
// in them are numbers Nineveh actually produced.
//
// The shots are tabbed rather than stacked: three full-width images in a row is a
// scroll, and the point is that these are three views of one product, not three
// features.
//
// It sits under `Core` and carries no section heading of its own. Screenshots are
// evidence, and evidence goes after the claim — the section used to lead with them,
// which left a reader looking at a dashboard before they had been told what the thing
// does or why rows get into it.

import Image from "next/image";
import { useState } from "react";

import { Register, Section } from "./bits";

// The order is the order the work happens in: you describe a table, then you have one,
// then it moves, then you watch it keep up.
//
// TODO: a fifth shot of the create flow — the address pasted, the catalog listed, the
// events and resources ticked. It's the one screen that would prove the closing panel's
// first two steps instead of asking to be believed. Drop the jpg in `public/shots/` and
// add it here as the first entry; the tabs and the figure need no other change.
const SHOTS = [
  {
    id: "reducer",
    tab: "Your own tables",
    src: "/shots/reducer.jpg",
    width: 1512,
    height: 787,
    caption:
      "Four questions and a shape, and Studio writes the first draft. It checks as you type, lists every source and table beside the file, and holds the save until the project builds.",
    alt: "Nineveh Studio's state table editor, showing a generated reducer beside a panel of the project's sources and tables",
  },
  {
    id: "table",
    tab: "Your data",
    src: "/shots/table.jpg",
    width: 1499,
    height: 812,
    caption:
      "836,565 orders off a testnet perps DEX, typed by the config and filterable by any column.",
    alt: "Nineveh Studio showing a log table of 836,565 rows with typed, filterable columns",
  },
  {
    id: "changes",
    tab: "Live changes",
    src: "/shots/changes.jpg",
    width: 1499,
    height: 812,
    caption:
      "Every row that changes, as it commits. The same feed your app subscribes to over SSE.",
    alt: "Nineveh Studio's change feed, showing inserts and updates arriving in commit order",
  },
  {
    id: "health",
    tab: "Keeping up",
    src: "/shots/overview.jpg",
    width: 1499,
    height: 812,
    caption:
      "Where the cursor is, how far behind the chain, and how fast it's folding. The history it holds is what sets how far back a rebuild can reach without reading the chain again.",
    alt: "Nineveh Studio's overview, showing a project caught up with the chain and its history usage",
  },
];

export function Shots() {
  const [active, setActive] = useState(SHOTS[0]!.id);
  const shot = SHOTS.find((s) => s.id === active) ?? SHOTS[0]!;

  return (
    <Section id="studio" rule={false}>
      <Register at="studio">
        <h3 className="font-display text-2xl font-semibold tracking-[-0.015em] text-balance text-white">
          And here it is, running.
        </h3>
        <p className="mt-4 max-w-[62ch] leading-relaxed text-pretty text-white/55">
          Studio creates the project, then shows you what it built: the tables, the changes
          arriving, and whether the whole thing is keeping up with the chain. Every screen below
          is Studio against a live contract on testnet, not a mockup — the rows in it are rows a
          reducer wrote.
        </p>

        <div
          role="tablist"
          aria-label="Studio screens"
          className="mt-10 flex flex-wrap gap-1 border-b border-white/10"
        >
          {SHOTS.map((s) => (
            <button
              key={s.id}
              role="tab"
              type="button"
              aria-selected={s.id === active}
              onClick={() => setActive(s.id)}
              className={`-mb-px cursor-pointer border-b-2 px-4 py-3 text-sm font-medium transition-colors outline-none focus-visible:ring-2 focus-visible:ring-blue-400 ${
                s.id === active
                  ? "border-blue-400 text-white"
                  : "border-transparent text-white/45 hover:text-white/75"
              }`}
            >
              {s.tab}
            </button>
          ))}
        </div>

        <figure className="mt-8">
          <div className="overflow-hidden rounded-xl border border-white/10 bg-deep shadow-hero">
            <Image
              key={shot.id}
              src={shot.src}
              alt={shot.alt}
              width={shot.width}
              height={shot.height}
              priority={false}
              className="w-full"
            />
          </div>
          <figcaption className="mt-4 max-w-[62ch] text-sm leading-relaxed text-white/45">
            {shot.caption}
          </figcaption>
        </figure>
      </Register>
    </Section>
  );
}
