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
      <div className="mx-auto max-w-3xl text-center">
        <a
          href="#how"
          className="inline-flex items-center gap-2 rounded-full border border-blue-400/25 bg-blue-500/10 px-3.5 py-1.5 text-xs font-medium text-blue-200 backdrop-blur transition-colors hover:border-blue-400/50 hover:bg-blue-500/15"
        >
          <span className="size-1.5 rounded-full bg-blue-400 shadow-[0_0_8px_2px_oklch(0.716_0.152_259_/_0.6)]" />
          Built for Aptos, on the transaction stream
        </a>
        <h1 className="mt-7 font-display text-4xl leading-[1.04] font-semibold tracking-[-0.022em] text-balance text-white sm:text-5xl">
          A live backend for your Aptos contract
        </h1>
        <p className="mx-auto mt-6 max-w-xl text-lg leading-relaxed text-pretty text-white/55">
          Point Nineveh at your contract&apos;s address. Get a database and an API that stay in sync
          with the chain — sorted, filtered, aggregated, live. No indexer to write, nothing to
          run.
        </p>
        <div className="mt-9 flex flex-wrap items-center justify-center gap-3">
          <Button href="https://studio.nineveh.dev" size="lg">
            Get started
          </Button>
          <Button href="#how" tone="quiet" size="lg">
            See how it works
          </Button>
        </div>
      </div>

      <figure className="mt-16 sm:mt-20">
        <div className="rounded-[1.4rem] border border-white/10 bg-white/[0.035] p-1.5 shadow-hero backdrop-blur">
          <Stream />
        </div>
        <figcaption className="mt-4 text-center text-xs text-white/35">
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
          <p className="text-lg leading-relaxed text-pretty text-white/55">
            <em className="text-white/80">What is X right now?</em> One account&apos;s balance. One
            listing by id. It can&apos;t sort, total, join or give you a feed — and the data your
            app needs isn&apos;t even in storage. It lives in events and write sets, because keeping
            totals on-chain costs gas on every transaction.
          </p>
          <p className="mt-5 text-white/55">
            So every team writes an indexer: a processor, a database, a server, a deploy pipeline. A
            week of work, and something to maintain forever.{" "}
            <span className="font-medium text-white">Nineveh is that week, done.</span>
          </p>
        </div>
      </div>

      {/* The three asks, set as an editorial list rather than boxed up as cards. */}
      <dl className="mt-16 border-t border-white/10">
        {ASKS.map(([ask, kinds]) => (
          <div
            key={ask}
            className="group grid gap-1.5 border-b border-white/10 py-7 sm:grid-cols-[1.05fr_1fr] sm:gap-10"
          >
            <dt className="flex items-baseline gap-3 text-xl font-medium tracking-tight text-white sm:text-2xl">
              <span className="mt-2 size-1.5 shrink-0 rounded-full bg-blue-400 transition-transform duration-300 group-hover:scale-150" />
              {ask}
            </dt>
            <dd className="self-center pl-6 text-white/50 sm:pl-0">{kinds}</dd>
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
          <Heading center>Say what changes. Get the API.</Heading>
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
            <p className="text-sm leading-relaxed text-white/50">
              That is the whole language:{" "}
              <code className="rounded bg-white/10 px-1 py-0.5 font-mono text-[12px] text-blue-200">
                on
              </code>
              , a row, an assignment,{" "}
              <code className="rounded bg-white/10 px-1 py-0.5 font-mono text-[12px] text-blue-200">
                if
              </code>{" "}
              and{" "}
              <code className="rounded bg-white/10 px-1 py-0.5 font-mono text-[12px] text-blue-200">
                return
              </code>
              . It reads like TypeScript and your editor treats it as such, but nothing is
              executed — it compiles to a fold that replays the same way every time.
            </p>
          </div>
          <div className="flex flex-col gap-5">
            <Code title="your API, a second later" lines={RESPONSE} />
            <p className="text-sm leading-relaxed text-white/50">
              Wide integers come back as strings, because a{" "}
              <code className="rounded bg-white/10 px-1 py-0.5 font-mono text-[12px] text-blue-200">
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
            <div className="h-px w-full bg-gradient-to-r from-blue-400/50 via-blue-400/25 to-transparent" />
            <span className="travel absolute -top-[3px] size-[7px] rounded-full bg-blue-200 shadow-[0_0_14px_4px_oklch(0.716_0.152_259_/_0.65)]" />
          </div>
          {STEPS.map((step, i) => (
            <li key={step.title} className="relative">
              <div className="flex size-7 items-center justify-center rounded-full border border-white/15 bg-deep font-mono text-[11px] font-semibold text-blue-200">
                {i + 1}
              </div>
              <h3 className="mt-5 font-semibold text-white">{step.title}</h3>
              <p className="mt-2 text-sm leading-relaxed text-white/50">{step.body}</p>
            </li>
          ))}
        </ol>

        {/* The promises, stated plainly — the page stops selling for four lines. */}
        <div className="mt-28 grid gap-10 border-t border-white/10 pt-12 sm:grid-cols-2 lg:grid-cols-4 lg:gap-12">
          {GUARANTEES.map((g) => (
            <div key={g.title}>
              <div className="text-sm font-semibold text-white">{g.title}</div>
              <p className="mt-2 text-sm leading-relaxed text-white/50">{g.body}</p>
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
      <p className="mt-8 text-center text-sm text-white/45">
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
        <div className="relative overflow-hidden rounded-3xl bg-blue-600 px-8 py-16 text-center shadow-glow sm:px-16 sm:py-20">
          <div
            className="absolute inset-0 opacity-70"
            style={{
              background:
                "radial-gradient(30rem 20rem at 20% 0%, oklch(0.716 0.152 259 / 0.7), transparent 70%), radial-gradient(28rem 18rem at 85% 100%, oklch(0.412 0.158 262 / 0.8), transparent 70%)",
            }}
            aria-hidden
          />
          <div className="relative">
            <h2 className="mx-auto max-w-2xl text-3xl font-semibold tracking-tight text-balance text-white sm:text-[2.7rem] sm:leading-[1.08]">
              You deployed the contract. The backend is the easy part now.
            </h2>
            <p className="mx-auto mt-5 max-w-xl text-lg leading-relaxed text-pretty text-blue-50/90">
              Backfills, cursors, retries, crash recovery — Nineveh&apos;s problem, not yours.
            </p>
            <ol className="mx-auto mt-10 grid max-w-2xl gap-3 text-left sm:grid-cols-3">
              {[
                ["Paste your address", "Nineveh reads the contract's modules off the chain."],
                ["Tick what to follow", "Events, resources and tables become tables of your own."],
                ["Query it", "REST, a change feed and webhooks, seconds later."],
              ].map(([title, body], i) => (
                <li key={title} className="rounded-xl border border-white/20 bg-white/10 p-4">
                  <span className="font-mono text-xs text-blue-100">0{i + 1}</span>
                  <div className="mt-2 text-sm font-medium text-white">{title}</div>
                  <p className="mt-1 text-xs leading-relaxed text-blue-50/75">{body}</p>
                </li>
              ))}
            </ol>
            <div className="mt-10">
              <a
                href="https://github.com/thewoodfish/Nineveh"
                className="inline-flex items-center justify-center rounded-xl bg-white px-6 py-3 text-sm font-semibold text-blue-700 shadow-card transition-colors hover:bg-blue-50"
              >
                Get started
              </a>
            </div>
          </div>
        </div>
      </section>

      <footer className="mx-auto flex max-w-6xl flex-col items-center justify-between gap-4 px-6 pt-10 pb-12 text-sm text-white/40 sm:flex-row">
        <div className="flex items-center gap-2">
          <Logo className="size-4 text-blue-400" />
          <span className="font-medium text-white/80">Nineveh</span>
          <span>— a live backend for Aptos apps</span>
        </div>
        <a
          href="https://github.com/thewoodfish/Nineveh"
          className="transition-colors hover:text-white"
        >
          GitHub
        </a>
      </footer>
    </div>
  );
}
