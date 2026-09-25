import { Button, Code, Heading, Lede, Logo, Register, Section } from "@/components/bits";
import { Pricing } from "@/components/pricing";
import { Shots } from "@/components/shots";
import { Builds } from "@/components/builds";
import { Nav } from "@/components/nav";
import { Stream } from "@/components/stream";

// The whole backend, and it is the example from the docs — compiled in CI by
// nineveh-dsl's tests/landing.rs, so the front page cannot drift from the language.
const REDUCER = [
  "export const sellers = table({",
  "  key:     { seller: address },",
  "  columns: { sold: u64.default(0), revenue: u64.default(0) },",
  "})",
  "",
  "on(sold, (s) => {",
  "  const row = sellers.row(s.seller)",
  "  row.sold    += 1",
  "  row.revenue += s.price - s.fee",
  "})",
];

const RESPONSE = [
  "GET /v1/tables/sellers?order=revenue.desc&limit=2",
  "",
  '{ "rows": [',
  '    { "seller": "0x7a3f…c41d",',
  '      "sold": "128",',
  '      "revenue": "94210" },',
  '    { "seller": "0x1e87…8d2a",',
  '      "sold": "91",',
  '      "revenue": "63880" }',
  "  ],",
  '  "count": 4 }',
];

const ASKS = [
  ["Show me all of them, sorted", "Top players. Cheapest listings. Biggest holders."],
  ["What happened?", "A feed. A history. This user's last twenty actions."],
  ["How much, in total?", "Revenue per seller. Volume per day. Count per account."],
];

const STEPS = [
  {
    title: "The chain pushes",
    body: "Nineveh sits on Aptos' transaction firehose. Every transaction arrives in order as it commits — no polling, no cron, nothing to schedule.",
  },
  {
    title: "It decodes what you follow",
    body: "Each transaction carries its events and its write set — the exact storage slots it changed. That's why resources and tables work, not just events.",
  },
  {
    title: "It folds",
    body: "Records run through your rules. Everything from one transaction lands in a single database write, with a bookmark saying how far it got.",
  },
  {
    title: "You query it",
    body: "REST over every table, with filters, sorting and paging, plus a live change feed and signed webhooks.",
  },
];

const GUARANTEES = [
  { title: "In order", body: "A balance that goes 5 → 12 → 7 lands as 7, never as 12." },
  { title: "Exactly once", body: "Rows and cursor commit together, or neither does." },
  { title: "Crash-safe", body: "A restart resumes from the cursor. Nothing counted twice." },
  {
    title: "Exact arithmetic",
    body: "Move's integers, checked. A bad rule halts; it never corrupts.",
  },
];

/*
 * The page is dark throughout. It still runs in acts — the machinery drops a floor into
 * the `deep` ground and the close drops again and stays there — but the change is in
 * depth and in light, not in whether the lights are on.
 */
export default function Home() {
  return (
    <>
      <div className="canvas" />
      <div className="weave" />
      <Nav />
      <main>
        <Hero />
        <Problem />
        <Machinery />
        <Shots />
        <Payoff />
        <Pricing />
        <Closing />
      </main>
    </>
  );
}

function Hero() {
  return (
    <div id="top" className="mx-auto max-w-6xl px-6 pt-16 pb-10 sm:pt-24">
      {/* Flush left, not centred. Display type this size wants a margin to hang off —
          centred, it reads as a slide, and every line break lands in a different place.
          The lede sits under it at a narrower measure so the two edges agree. */}
      <div className="max-w-4xl">
        <h1 className="font-display text-[3.4rem] leading-[0.94] font-normal tracking-[-0.038em] text-balance text-cream sm:text-[4.6rem]">
          Your backend for Aptos
        </h1>
        <p className="mt-7 max-w-xl text-lg leading-relaxed text-pretty text-cream/55">
          Point Nineveh at your contract. Its events, resources and tables become live, queryable
          state — a REST API, realtime updates and signed webhooks. No indexer to write, nothing
          to run.
        </p>
        <div className="mt-9 flex flex-wrap items-center gap-3">
          <Button href="https://studio.nineveh.dev" size="lg">
            Start building
          </Button>
          <Button href="#how" tone="quiet" size="lg">
            See how it works
          </Button>
        </div>
      </div>

      <figure className="mt-16 sm:mt-20">
        <div className="rounded-[1.4rem] border border-cream/10 bg-cream/[0.035] p-1.5 shadow-hero backdrop-blur">
          <Stream />
        </div>
        <figcaption className="mt-4 text-xs text-cream/35">
          Every sale the contract emits, folded into the table your app queries.
        </figcaption>
      </figure>
    </div>
  );
}

function Problem() {
  return (
    <Section id="problem" rule={false}>
      <Register at="problem">
      <div className="grid gap-8 lg:grid-cols-[1fr_1fr] lg:gap-16">
        <div>
          <Heading>The chain answers one kind of question</Heading>
        </div>
        <div className="self-end">
          <p className="text-lg leading-relaxed text-pretty text-cream/55">
            <em className="text-cream/80">What is X right now?</em> One account&apos;s balance. One
            listing by id. It can&apos;t sort, total, join or give you a feed — and the data your
            app needs isn&apos;t even in storage. It lives in events and write sets, because keeping
            totals on-chain costs gas on every transaction.
          </p>
          <p className="mt-5 text-cream/55">
            So every team writes an indexer: a processor, a database, a server, a deploy pipeline. A
            week of work, and something to maintain forever.{" "}
            <span className="font-medium text-cream">Nineveh is that week, done.</span>
          </p>
        </div>
      </div>

      {/* The three asks, set as an editorial list rather than boxed up as cards. */}
      <dl className="mt-16 border-t border-cream/10">
        {ASKS.map(([ask, kinds]) => (
          <div
            key={ask}
            className="group grid gap-1.5 border-b border-cream/10 py-7 sm:grid-cols-[1.05fr_1fr] sm:gap-10"
          >
            <dt className="flex items-baseline gap-3 text-xl font-medium tracking-tight text-cream sm:text-2xl">
              <span className="mt-2 size-1.5 shrink-0 rounded-full bg-mint-300 transition-transform duration-300 group-hover:scale-150" />
              {ask}
            </dt>
            <dd className="self-center pl-6 text-cream/50 sm:pl-0">{kinds}</dd>
          </div>
        ))}
      </dl>
      </Register>
    </Section>
  );
}

/** The deep act: the config, the machine that runs it, and what it promises. */
function Machinery() {
  return (
    <section id="how" className="deep">
      <div className="mx-auto max-w-6xl px-6 pt-44 pb-40 sm:pt-52 sm:pb-48">
        <Register at="how">
        <div className="mx-auto max-w-3xl text-center">
          <Heading center>Describe the data you want. Get the API.</Heading>
          <Lede center>
            A reducer is a handler: when this arrives, this row changes. No processor to write, no
            migrations, no schema to keep in step. Change a rule and Nineveh rebuilds the table
            from history in the background, then swaps it in — the old data keeps serving the
            whole time.
          </Lede>
        </div>

        <div className="mt-16 grid items-start gap-6 lg:grid-cols-2">
          <div className="flex flex-col gap-5">
            <Code title="market.nineveh.ts" lines={REDUCER} />
            <p className="text-sm leading-relaxed text-cream/50">
              That is the whole language:{" "}
              <code className="rounded bg-cream/10 px-1 py-0.5 font-mono text-[12px] text-mint-200">
                on
              </code>
              , a row, an assignment,{" "}
              <code className="rounded bg-cream/10 px-1 py-0.5 font-mono text-[12px] text-mint-200">
                if
              </code>{" "}
              and{" "}
              <code className="rounded bg-cream/10 px-1 py-0.5 font-mono text-[12px] text-mint-200">
                return
              </code>
              . It reads like TypeScript and your editor treats it as such, but nothing is
              executed — it compiles to a fold that replays the same way every time.
            </p>
          </div>
          <div className="flex flex-col gap-5">
            <Code title="your API, a second later" lines={RESPONSE} />
            <p className="text-sm leading-relaxed text-cream/50">
              Wide integers come back as strings, because a{" "}
              <code className="rounded bg-cream/10 px-1 py-0.5 font-mono text-[12px] text-mint-200">
                u128
              </code>{" "}
              doesn&apos;t fit a JavaScript number. Every table gets the same treatment, plus a live
              change feed and signed webhooks.
            </p>
          </div>
        </div>

        <div className="mt-32">
          <Heading>Four steps, and none of them are yours</Heading>
        </div>
        {/* A rail runs through the steps, with a pulse travelling it: one movement, not
            four boxes. */}
        <ol className="relative mt-14 grid gap-10 sm:grid-cols-2 lg:grid-cols-4 lg:gap-8">
          <div className="absolute top-3.5 right-0 left-0 hidden lg:block" aria-hidden>
            <div className="h-px w-full bg-gradient-to-r from-mint-300/50 via-mint-300/25 to-transparent" />
            <span className="travel absolute -top-[3px] size-[7px] rounded-full bg-mint-200 shadow-[0_0_14px_4px_oklch(0.945_0.073_143_/_0.55)]" />
          </div>
          {STEPS.map((step, i) => (
            <li key={step.title} className="relative">
              <div className="flex size-7 items-center justify-center rounded-full border border-cream/15 bg-deep font-mono text-[11px] font-semibold text-mint-200">
                {i + 1}
              </div>
              <h3 className="mt-5 font-semibold text-cream">{step.title}</h3>
              <p className="mt-2 text-sm leading-relaxed text-cream/50">{step.body}</p>
            </li>
          ))}
        </ol>

        {/* The promises, stated plainly — the page stops selling for four lines. */}
        <div className="mt-28 grid gap-10 border-t border-cream/10 pt-12 sm:grid-cols-2 lg:grid-cols-4 lg:gap-12">
          {GUARANTEES.map((g) => (
            <div key={g.title}>
              <div className="text-sm font-semibold text-cream">{g.title}</div>
              <p className="mt-2 text-sm leading-relaxed text-cream/50">{g.body}</p>
            </div>
          ))}
        </div>
        </Register>
      </div>
    </section>
  );
}

function Payoff() {
  return (
    <Section rule={false}>
      <Register at="questions">
      <div className="mx-auto max-w-3xl text-center">
        <Heading center>Questions your contract already answers, but can&apos;t be asked</Heading>
        <Lede center>
          None of these need a contract change. The data is already on-chain; it simply isn&apos;t
          queryable.
        </Lede>
      </div>
      {/* Each card carries its own `rise`, so they arrive as you reach them. */}
      <div className="mt-14">
        <Builds />
      </div>
      <p className="mt-8 text-center text-sm text-cream/45">
        Whatever your contract emits, you can fold it into a table shaped like the question you
        actually ask.
      </p>
      </Register>
    </Section>
  );
}

/** The page goes down for the last time, and stays there. */
function Closing() {
  return (
    <div className="deep to-end">
      <section className="mx-auto w-full max-w-6xl px-6 pt-48 pb-16 sm:pt-56">
        {/* The page's one light block. The reference drops a full-bleed field of
            colour into a dark page and sets ink on it — that inversion is the loudest
            thing either page does, so it happens exactly once, at the ask. Everything
            inside is ink or an ink tint; nothing here is cream. */}
        <div className="relative overflow-hidden rounded-3xl bg-mint-200 px-8 py-16 text-center text-ink sm:px-16 sm:py-20">
          <div
            className="absolute inset-0"
            style={{
              background:
                "radial-gradient(34rem 22rem at 18% 0%, oklch(0.972 0.038 145 / 0.9), transparent 72%), radial-gradient(30rem 20rem at 88% 100%, oklch(0.83 0.134 147 / 0.5), transparent 72%)",
            }}
            aria-hidden
          />
          <div className="relative">
            <h2 className="mx-auto max-w-2xl font-display text-3xl leading-[1.04] font-medium tracking-[-0.03em] text-balance text-ink sm:text-[2.9rem]">
              You deployed the contract. The backend is the easy part now.
            </h2>
            <p className="mx-auto mt-6 max-w-xl text-lg leading-relaxed text-pretty text-ink/65">
              Backfills, cursors, retries, crash recovery — Nineveh&apos;s problem, not yours.
            </p>
            <ol className="mx-auto mt-10 grid max-w-2xl gap-3 text-left sm:grid-cols-3">
              {[
                ["Paste your address", "Nineveh reads the contract's modules off the chain."],
                ["Tick what to follow", "Events, resources and tables become tables of your own."],
                ["Query it", "REST, a change feed and webhooks, seconds later."],
              ].map(([title, body], i) => (
                <li key={title} className="rounded-xl border border-ink/12 bg-ink/[0.05] p-4">
                  <span className="font-mono text-[11px] tracking-wider text-ink/40">0{i + 1}</span>
                  <div className="mt-2 text-sm font-semibold text-ink">{title}</div>
                  <p className="mt-1 text-xs leading-relaxed text-ink/60">{body}</p>
                </li>
              ))}
            </ol>
            <div className="mt-10 flex flex-wrap items-center justify-center gap-3">
              <a
                href="https://studio.nineveh.dev"
                className="inline-flex items-center justify-center rounded-full bg-ink px-6 py-3 text-sm font-semibold text-cream transition-colors outline-none hover:bg-ink/85 focus-visible:ring-2 focus-visible:ring-ink/40"
              >
                Start building
              </a>
              <a
                href="https://github.com/thewoodfish/Nineveh"
                className="inline-flex items-center justify-center rounded-full border border-ink/25 px-6 py-3 text-sm font-semibold text-ink/80 transition-colors outline-none hover:border-ink/45 hover:bg-ink/[0.06] focus-visible:ring-2 focus-visible:ring-ink/40"
              >
                Read the source
              </a>
            </div>
          </div>
        </div>
      </section>

      <footer className="mx-auto flex max-w-6xl flex-col items-center justify-between gap-4 px-6 pt-10 pb-12 text-sm text-cream/40 sm:flex-row">
        <div className="flex items-center gap-2">
          <Logo className="size-4 text-mint-300" />
          <span className="font-medium text-cream/80">Nineveh</span>
          <span>— a backend for Aptos contract data</span>
        </div>
        <a
          href="https://github.com/thewoodfish/Nineveh"
          className="transition-colors hover:text-cream"
        >
          GitHub
        </a>
      </footer>
    </div>
  );
}
