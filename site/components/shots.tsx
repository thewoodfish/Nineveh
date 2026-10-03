"use client";

// Studio, photographed rather than described. These are real screens against the market
// example running on devnet — the same contract the demo page drives — so the rows in
// them are rows Nineveh actually produced.
//
// The shots are tabbed rather than stacked: five full-width images in a row is a scroll,
// and the point is that these are five views of one product, not five features.
//
// It sits under `Core` and carries no section heading of its own. Screenshots are
// evidence, and evidence goes after the claim — the section used to lead with them,
// which left a reader looking at a dashboard before they had been told what the thing
// does or why rows get into it.

import Image from "next/image";
import { useState } from "react";

import { Register, Section } from "./bits";

// The order is the order the work happens in: an address becomes a project, you describe
// a table, then you have one, then it moves, then you watch it keep up.
//
// The first shot predates adeacf5 and still shows a "Preview config" button that has
// since gone. Deliberate: it is one control in a 1920-pixel screenshot of a page that is
// otherwise current, and the alternative was not showing the step at all.

const SHOTS = [
  {
    id: "create",
    tab: "From an address",
    src: "/shots/create.jpg",
    width: 1920,
    height: 998,
    caption:
      "Paste a contract address and Nineveh reads it off the chain: every event it emits, every resource it stores, every table inside them. Tick what you want followed, name it, and the backend exists.",
    alt: "Nineveh Studio's new project page, having read a contract and listing the events, resources and tables it could follow",
  },
  {
    id: "reducer",
    tab: "Your own tables",
    src: "/shots/reducer.jpg",
    width: 1920,
    height: 998,
    caption:
      "Pick what a table folds. More than one source, when a row is made by one record and changed by another — the first brings rows into being, the rest alter what it made. Studio writes the reducer from the answers.",
    alt: "Nineveh Studio's new state table page, choosing which of a contract's events, resources and tables a new table folds",
  },
  {
    id: "table",
    tab: "Your data",
    src: "/shots/table.jpg",
    width: 1920,
    height: 998,
    caption:
      "Every table is browsable and filterable by any column, typed from the contract: addresses are addresses, a u64 is a u64. Its REST shape and schema are a tab away.",
    alt: "Nineveh Studio showing the rows of a log table with typed, filterable columns",
  },
  {
    id: "changes",
    tab: "Live changes",
    src: "/shots/changes.jpg",
    width: 1920,
    height: 998,
    caption:
      "Every row that changes, as it commits, with the record behind it. The same feed your app subscribes to over SSE — inserts, updates and deletes in the order the chain made them.",
    alt: "Nineveh Studio's change feed, showing inserts, updates and deletes arriving in commit order",
  },
  {
    id: "health",
    tab: "Keeping up",
    src: "/shots/overview.jpg",
    width: 1920,
    height: 998,
    caption:
      "Where the cursor is, how far behind the chain, and how fast it's folding — plus the URL your app calls. The history it keeps is what a rule change replays against, instead of reading the chain a second time.",
    alt: "Nineveh Studio's overview, showing a project caught up with the chain, its API URL and how much history it keeps",
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
          Studio creates the project, then shows you what it built: the tables you chose, the ones
          you folded yourself, every change as it commits, and whether the whole thing is keeping
          up. These are real screens against a contract running on devnet, not mockups — the rows
          in them came off the chain.
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
            {/* Eager, not lazy. Only one of these is mounted at a time and it is mounted
                because somebody pressed its tab, so it is on screen before it exists —
                and a lazily loaded image inserted already inside the viewport never has
                its observer fire. The second and third tabs showed nothing at all. */}
            <Image
              key={shot.id}
              src={shot.src}
              alt={shot.alt}
              width={shot.width}
              height={shot.height}
              loading="eager"
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
