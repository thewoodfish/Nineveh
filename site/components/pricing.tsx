// Pricing, as a card with what you get in it.
//
// One card carries weight and one doesn't, on purpose: there is a plan you can have
// today and a list of things you can't have yet, and making them identical cards would
// imply a choice you don't get to make. The numbers are the ones the control plane
// enforces (`nineveh-control/src/tier.rs`), so the page and the product can't drift.
//
// The free tier is permanent, in the Supabase sense: paid plans add mainnet and
// production scale on top rather than switching the free one off. Copy here must never
// imply the free tier expires — a developer choosing where to build reads that as a
// countdown, and builds somewhere else.

import { Heading, Lede, Register, Section } from "./bits";

/** What the Free plan gives you, in the order a developer cares about it. */
const INCLUDED = [
  "Live tables built from your contract's events, resources and tables",
  "REST over every table — filters, sorting, paging, exact counts",
  "A live change feed over SSE, resumable from any position",
  "Signed webhooks, each endpoint with its own secret and cursor",
  "Studio: build a project from an address and watch it fill",
  "Rule changes that replay your own history instead of the chain",
  "Backfills, cursors, retries and crash recovery",
];

/**
 * The numbers, kept apart from the features so they stay scannable. Labels carry the
 * noun and values carry the unit, so the right-hand column reads straight down.
 */
const LIMITS = [
  ["Projects", "2"],
  ["Networks", "testnet, devnet"],
  ["History per project", "1 GB"],
  ["Change feed", "7 days"],
  ["Backfill", "6 hours"],
];

/** Deliberately unexplained: each one answers a line in the Free card's limits. */
const LATER = ["Mainnet", "More than two projects", "History deeper than six hours", "GraphQL"];

function Check() {
  return (
    <svg viewBox="0 0 16 16" className="mt-[5px] size-3.5 shrink-0 text-blue-400" aria-hidden>
      <path
        fill="none"
        stroke="currentColor"
        strokeWidth="2.2"
        strokeLinecap="round"
        strokeLinejoin="round"
        d="M2.5 8.5l3.5 3.5 7.5-8"
      />
    </svg>
  );
}

export function Pricing() {
  return (
    <Section id="pricing">
      <Register at="pricing">
        <Heading>Free to build on. Always.</Heading>
        <Lede>
          Not a trial. The free tier doesn&apos;t expire, doesn&apos;t need a card, and stays free
          when paid plans arrive — testnet and devnet for as long as you want them. The limits
          are real numbers Nineveh enforces, and when you reach one it tells you which and what
          it means.
        </Lede>

        <div className="mt-12 grid items-start gap-6 lg:grid-cols-[1.25fr_1fr] lg:gap-8">
          {/* The plan you can actually have: lifted, outlined in blue, its own light. */}
          <div className="relative overflow-hidden rounded-2xl border border-blue-400/30 bg-white/[0.045] p-8 shadow-glow backdrop-blur sm:p-10">
            <div
              className="pointer-events-none absolute inset-x-0 top-0 h-48 opacity-60"
              style={{
                background:
                  "radial-gradient(28rem 12rem at 20% 0%, oklch(0.552 0.221 261 / 0.45), transparent 70%)",
              }}
              aria-hidden
            />
            <div className="relative">
              <div className="flex items-center justify-between gap-4">
                <h3 className="font-display text-2xl font-semibold tracking-[-0.015em] text-white">
                  Free
                </h3>
                <span className="rounded-full border border-blue-400/30 bg-blue-500/15 px-3 py-1 text-xs font-medium text-blue-200">
                  Always free
                </span>
              </div>

              <div className="mt-5 flex items-baseline gap-2">
                <span className="font-display text-5xl leading-none font-semibold tracking-[-0.03em] text-white">
                  $0
                </span>
                <span className="text-sm text-white/45">per month, every account</span>
              </div>

              <a
                href="https://studio.nineveh.dev"
                className="mt-7 flex w-full items-center justify-center rounded-xl bg-blue-600 px-6 py-3.5 text-sm font-semibold text-white shadow-card transition-colors outline-none hover:bg-blue-500 focus-visible:ring-2 focus-visible:ring-blue-300"
              >
                Start building
              </a>

              <ul className="mt-8 flex flex-col gap-3 border-t border-white/10 pt-7">
                {INCLUDED.map((item) => (
                  <li key={item} className="flex gap-3 text-[15px] leading-relaxed text-white/70">
                    <Check />
                    {item}
                  </li>
                ))}
              </ul>

              {/* Inset, so the spec reads as a different kind of thing from the list
                  above it without needing a label to say so. */}
              <dl className="mt-8 rounded-xl bg-black/20 px-5 py-1.5 ring-1 ring-white/[0.07]">
                {LIMITS.map(([label, value]) => (
                  <div
                    key={label}
                    className="flex items-baseline justify-between gap-6 border-b border-white/[0.07] py-3 last:border-0"
                  >
                    <dt className="text-sm text-white/50">{label}</dt>
                    <dd className="font-mono text-[13px] whitespace-nowrap text-clay-400">
                      {value}
                    </dd>
                  </div>
                ))}
              </dl>
            </div>
          </div>

          {/* What isn't on offer yet: flat, quiet, and not pretending to be a choice. */}
          <div className="rounded-2xl border border-white/10 bg-white/[0.02] p-8 sm:p-10">
            <div className="flex items-center justify-between gap-4">
              <h3 className="font-display text-2xl font-semibold tracking-[-0.015em] text-white/70">
                More
              </h3>
              <span className="rounded-full border border-white/12 px-3 py-1 text-xs font-medium text-white/40">
                Coming soon
              </span>
            </div>

            <ul className="mt-8 flex flex-col gap-3">
              {LATER.map((item) => (
                <li key={item} className="flex gap-3 text-[15px] leading-relaxed text-white/55">
                  <span
                    className="mt-[9px] size-1.5 shrink-0 rounded-full border border-white/30"
                    aria-hidden
                  />
                  {item}
                </li>
              ))}
            </ul>

            <p className="mt-8 border-t border-white/10 pt-6 text-sm text-white/35">
              No billing yet, so nothing to buy.
            </p>
          </div>
        </div>
      </Register>
    </Section>
  );
}
