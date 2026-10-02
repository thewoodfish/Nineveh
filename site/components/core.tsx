// The one idea, said plainly.
//
// Everything else on this page is a consequence of a single rule — a reducer is the only
// thing that writes a table — and the page described the consequences at length without
// ever stating the rule. A reader could finish it knowing Nineveh replays, pushes
// changes and rebuilds without knowing why any of that is possible, or why none of it
// can be bolted onto a backend they already have.
//
// Markup, not an image, like the other two diagrams here: it stays legible at any width,
// carries its own text, and can't go stale.

import { Heading, Lede, Register, Section } from "./bits";

/** The path a value takes. The ends are the developer's: their contract, their app. */
const FLOW = [
  { name: "your contract", note: "Events and write sets. The source of truth, on chain." },
  { name: "records", note: "What arrived, in version order, kept so it can be replayed." },
  { name: "reducers", note: "Your fold. The only thing that writes a table." },
  { name: "state tables", note: "Rows, typed and queryable. Derived, never written to." },
  { name: "your app", note: "REST, a live feed, signed webhooks. Reads only." },
];

/**
 * What falls out of the rule. Each one is a thing a backend engineer would otherwise
 * build and get subtly wrong, and each is free here for the same reason.
 */
const FOLLOWS = [
  {
    title: "A wrong number has one cause",
    body: "There is no POST /rows. Every row exists because a reducer put it there, so a figure that looks wrong is a rule that is wrong. Nothing else can have touched it, and there is no other place to look.",
  },
  {
    title: "Replay is exact",
    body: "No clock, no randomness, no network call inside a fold. The same records always produce the same tables — which is what makes a rebuild a fact rather than a hope.",
  },
  {
    title: "Changes announce themselves",
    body: "A row changes in exactly one place, so that place is where the change is published. Rows and the feed leave in one commit: nothing is announced that isn't in the table, and nothing lands without saying so.",
  },
  {
    title: "Rules are cheap to change",
    body: "Edit a fold and every row it wrote is wrong by definition. Nineveh replays its own stored records — not the chain — into a new table beside the live one, and swaps them when it catches up.",
  },
];

export function Core() {
  return (
    <Section id="model" rule={false}>
      <Register at="model">
        <Heading>State is derived, not written.</Heading>
        <Lede>
          One rule holds the whole thing up: a reducer is the only thing that ever writes a
          table. Not your API, not a script, not a migration. Everything below is a
          consequence of it, and none of it is available to a backend that lets anything else
          write.
        </Lede>

        {/* The path, as one line on wide screens and a stack on narrow ones. Each rung
            carries its own note, so the drawing doesn't need a legend. */}
        <ol className="mt-14 grid gap-px overflow-hidden rounded-xl bg-white/10 sm:grid-cols-2 lg:grid-cols-5">
          {FLOW.map((step, i) => {
            const mine = i === 0 || i === FLOW.length - 1;
            return (
              <li key={step.name} className="flex flex-col gap-2 bg-deep p-5">
                <div className="flex items-center gap-2">
                  <span
                    aria-hidden
                    className={`size-1.5 shrink-0 rounded-full ${mine ? "bg-clay-400" : "bg-blue-400"}`}
                  />
                  <span
                    className={`font-mono text-[11px] font-semibold tracking-[0.04em] ${
                      mine ? "text-clay-400" : "text-blue-200"
                    }`}
                  >
                    {step.name}
                  </span>
                </div>
                <p className="text-sm leading-relaxed text-white/50">{step.note}</p>
              </li>
            );
          })}
        </ol>
        <p className="mt-5 text-sm leading-relaxed text-white/45">
          The two ends are yours. Everything between them is the part you would otherwise
          build.
        </p>

        <dl className="mt-20 grid gap-x-16 gap-y-10 sm:grid-cols-2">
          {FOLLOWS.map((item) => (
            <div key={item.title} className="border-t border-white/10 pt-5">
              <dt className="font-semibold text-white">{item.title}</dt>
              <dd className="mt-3 text-sm leading-relaxed text-white/55">{item.body}</dd>
            </div>
          ))}
        </dl>
      </Register>
    </Section>
  );
}
