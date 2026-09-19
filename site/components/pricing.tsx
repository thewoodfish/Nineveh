// Pricing. There is one plan and there is no billing, so this says so plainly instead
// of arranging three columns and putting a badge on the middle one. The numbers are the
// ones the control plane actually enforces (`nineveh-control/src/tier.rs`), so the page
// and the product can't drift.

import { Button, Heading, Lede, Register, Section } from "./bits";

const LIMITS = [
  ["Projects", "2", "A live one and a scratch one."],
  ["Networks", "testnet, devnet", "Mainnet is the one that costs real stream time."],
  ["History per project", "1 GB", "About two million records — months of a normal contract."],
  ["Change feed kept", "7 days", "Long enough for a receiver that was down over a weekend."],
  ["Backfill", "6 hours", "How far before the chain's tip a new project can start."],
];

const INCLUDED = [
  "REST over every state table, with filters, sorting and paging",
  "A live change feed, and signed webhooks with their own secrets",
  "Studio: build a project from an address, watch it fill, query it",
  "Rebuilds that replay your own records instead of re-reading the chain",
  "Backfills, cursors, retries and crash recovery",
];

const LATER = [
  ["Mainnet", "The stream time a mainnet project uses is the real cost here."],
  ["Deeper history", "Starting a project further back than six hours."],
  ["More projects", "Two is where it starts, not where it stays."],
  ["GraphQL", "REST and the change feed are built; this one isn't."],
];

export function Pricing() {
  return (
    <Section id="pricing">
      <Register at="pricing">
        <Heading>Free while Nineveh is in alpha</Heading>
        <Lede>
          There is one plan and no card. The limits below are real numbers the backend
          enforces, not a trial that expires — when you hit one, it tells you which one and
          what it means.
        </Lede>

        <div className="mt-14 grid gap-x-16 gap-y-12 lg:grid-cols-[1.1fr_1fr]">
          <div>
            <div className="flex items-baseline gap-3">
              <span className="font-display text-4xl font-semibold tracking-[-0.02em] text-white">
                Free
              </span>
              <span className="text-sm text-white/40">every account, today</span>
            </div>

            <dl className="mt-8 border-t border-white/10">
              {LIMITS.map(([label, value, note]) => (
                <div key={label} className="grid gap-x-6 border-b border-white/10 py-4 sm:grid-cols-[1fr_auto]">
                  <dt className="text-[15px] font-medium text-white">{label}</dt>
                  <dd className="row-start-1 font-mono text-[13px] text-clay-400 sm:col-start-2 sm:text-right">
                    {value}
                  </dd>
                  <dd className="mt-1 text-sm text-white/45 sm:col-span-2">{note}</dd>
                </div>
              ))}
            </dl>

            <div className="mt-8 flex flex-wrap items-center gap-3">
              <Button href="/docs" size="lg">
                Start building
              </Button>
              <Button href="/docs#13-a-walkthrough-to-test-against" tone="quiet" size="lg">
                See the walkthrough
              </Button>
            </div>
          </div>

          <div>
            <h3 className="text-[15px] font-medium text-white">All of it, at no tier</h3>
            <ul className="mt-5 flex flex-col gap-3">
              {INCLUDED.map((item) => (
                <li key={item} className="flex gap-3 text-[15px] leading-relaxed text-white/60">
                  <svg viewBox="0 0 16 16" className="mt-[7px] size-3 shrink-0 text-blue-400" aria-hidden>
                    <path
                      fill="none"
                      stroke="currentColor"
                      strokeWidth="2.2"
                      strokeLinecap="round"
                      strokeLinejoin="round"
                      d="M2.5 8.5l3.5 3.5 7.5-8"
                    />
                  </svg>
                  {item}
                </li>
              ))}
            </ul>

            <h3 className="mt-11 text-[15px] font-medium text-white">Not yet</h3>
            <dl className="mt-5 border-t border-white/10">
              {LATER.map(([title, why]) => (
                <div key={title} className="border-b border-white/10 py-3.5">
                  <dt className="text-[15px] text-white/65">{title}</dt>
                  <dd className="mt-0.5 text-sm leading-relaxed text-white/35">{why}</dd>
                </div>
              ))}
            </dl>
          </div>
        </div>
      </Register>
    </Section>
  );
}
