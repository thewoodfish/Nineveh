"use client";

// Studio, photographed rather than described. These are real screens against real
// testnet data — a perpetuals DEX and the framework's token transfers — so the numbers
// in them are numbers Nineveh actually produced.
//
// The shots are tabbed rather than stacked: three full-width images in a row is a
// scroll, and the point is that these are three views of one product, not three
// features.

import Image from "next/image";
import { useState } from "react";

import { Heading, Lede, Register, Section } from "./bits";

const SHOTS = [
  {
    id: "table",
    tab: "Your data",
    src: "/shots/table.jpg",
    caption:
      "836,565 orders off a testnet perps DEX, typed by the config and filterable by any column.",
    alt: "Nineveh Studio showing a log table of 836,565 rows with typed, filterable columns",
  },
  {
    id: "changes",
    tab: "Live changes",
    src: "/shots/changes.jpg",
    caption:
      "Every row that changes, as it commits. The same feed your app subscribes to over SSE.",
    alt: "Nineveh Studio's change feed, showing inserts and updates arriving in commit order",
  },
  {
    id: "health",
    tab: "Is it keeping up?",
    src: "/shots/overview.jpg",
    caption:
      "Where the cursor is, how far behind the chain, how fast it's folding, and how much of its own history it's holding, which is how far back it can be rebuilt without reading the chain again.",
    alt: "Nineveh Studio's overview, showing a project caught up with the chain and its history usage",
  },
];

export function Shots() {
  const [active, setActive] = useState(SHOTS[0]!.id);
  const shot = SHOTS.find((s) => s.id === active) ?? SHOTS[0]!;

  return (
    <Section id="studio" rule={false}>
      <Register at="studio">
        <Heading>You get a dashboard too</Heading>
        <Lede>
          Studio builds the project, then shows you what it built: the tables, the changes
          arriving, and whether the whole thing is keeping up with the chain.
        </Lede>

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
              width={1499}
              height={812}
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
