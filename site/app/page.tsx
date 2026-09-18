import { Button, Code, Eyebrow, Heading, Lede, Logo, Panel, Section } from "@/components/bits";
import { Builds } from "@/components/builds";
import { Stream } from "@/components/stream";

const CONFIG = [
  "# nineveh.yaml — the whole backend",
  "sources:",
  '  sold: { event: "0x…::market::Sold" }',
  "",
  "state:",
  "  sellers:",
  "    key: [seller]",
  "    columns:",
  "      seller:  address",
  "      sold:    { type: u64, default: 0 }",
  "      revenue: { type: u64, default: 0 }",
  "    reduce:",
  "      - on: sold",
  "        set:",
  '          sold:    "sold + 1"',
  '          revenue: "revenue + price - fee"',
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

export default function Home() {
  return (
    <>
      <div className="canvas" />
      <div className="weave" />
      <Nav />
      <main>
        <Hero />

        <Section id="problem">
          <div className="rise grid gap-10 lg:grid-cols-[1fr_1.05fr] lg:gap-16">
            <div>
              <Eyebrow>The problem</Eyebrow>
              <Heading>The chain answers one kind of question</Heading>
              <Lede>
                <em className="text-ink-700">What is X right now?</em> One account&apos;s balance.
                One listing by id. It can&apos;t sort, total, join or give you a feed — and the data
                your app needs isn&apos;t even in storage. It lives in events and write sets,
                because keeping totals on-chain costs gas on every transaction.
              </Lede>
              <p className="mt-6 max-w-lg text-ink-500">
                So every team writes an indexer: a processor, a database, a server, a deploy
                pipeline. A week of work, and something to maintain forever.{" "}
                <span className="font-medium text-ink-900">Nineveh is that week, done.</span>
              </p>
            </div>
            <ul className="flex flex-col divide-y divide-ink-200/70 self-center rounded-2xl border border-ink-200/70 bg-white/70 backdrop-blur">
              {[
                ["Show me all of them, sorted", "Top players. Cheapest listings. Biggest holders."],
                ["What happened?", "A feed. A history. This user's last twenty actions."],
                ["How much, in total?", "Revenue per seller. Volume per day. Count per account."],
              ].map(([q, a]) => (
                <li key={q} className="flex items-start gap-4 p-6">
                  <span className="mt-1.5 size-1.5 shrink-0 rounded-full bg-blue-400" />
                  <div>
                    <div className="font-medium text-ink-900">{q}</div>
                    <p className="mt-1 text-sm leading-relaxed text-ink-500">{a}</p>
                  </div>
                </li>
              ))}
            </ul>
          </div>
        </Section>

        {/* The page goes quiet here: code reads better on a dark ground, and one deep
            panel gives the argument a centre. */}
        <Section id="how" rule={false}>
          <Panel>
            <div className="rise">
              <Eyebrow dark>Seventeen lines</Eyebrow>
              <Heading dark>Describe the table. Get the API.</Heading>
              <Lede dark>
                No processor to write, no migrations, no schema to keep in step. Change a rule and
                Nineveh rebuilds the table from history in the background, then swaps it in — the
                old data keeps serving the whole time.
              </Lede>
            </div>
            <div className="rise mt-12 grid items-start gap-6 lg:grid-cols-2">
              <Code title="nineveh.yaml" lines={CONFIG} dark />
              <div className="flex flex-col gap-5">
                <Code title="your API, a second later" lines={RESPONSE} dark />
                <p className="text-sm leading-relaxed text-white/50">
                  Wide integers come back as strings, because a{" "}
                  <code className="rounded bg-white/10 px-1 py-0.5 font-mono text-[12px] text-blue-200">
                    u128
                  </code>{" "}
                  doesn&apos;t fit a JavaScript number. Every table gets the same treatment, plus a
                  live change feed and signed webhooks.
                </p>
              </div>
            </div>
          </Panel>
        </Section>

        {/* No hairline here: the panel above already broke the page. */}
        <Section rule={false}>
          <div className="rise">
            <Eyebrow>How it works</Eyebrow>
            <Heading>Four steps, and none of them are yours</Heading>
          </div>
          {/* A rail runs through the steps: one movement, not four boxes. */}
          <ol className="rise relative mt-14 grid gap-10 sm:grid-cols-2 lg:grid-cols-4 lg:gap-8">
            <div
              className="absolute top-3.5 right-0 left-0 hidden h-px bg-gradient-to-r from-blue-200 via-blue-300 to-transparent lg:block"
              aria-hidden
            />
            {STEPS.map((step, i) => (
              <li key={step.title} className="relative">
                <div className="flex size-7 items-center justify-center rounded-full border border-blue-200 bg-white font-mono text-[11px] font-semibold text-blue-700 shadow-soft">
                  {i + 1}
                </div>
                <h3 className="mt-5 font-semibold text-ink-900">{step.title}</h3>
                <p className="mt-2 text-sm leading-relaxed text-ink-500">{step.body}</p>
              </li>
            ))}
          </ol>
          {/* The promises, on the same dark ground as the code: the two places the page
              stops selling and states facts. */}
          <div className="rise mt-16 grid gap-px overflow-hidden rounded-2xl bg-white/[0.07] shadow-card ring-1 ring-ink-900 sm:grid-cols-2 lg:grid-cols-4">
            {GUARANTEES.map((g) => (
              <div key={g.title} className="bg-ink-900 p-5">
                <div className="text-sm font-semibold text-white">{g.title}</div>
                <p className="mt-1.5 text-sm leading-relaxed text-white/55">{g.body}</p>
              </div>
            ))}
          </div>
        </Section>

        <Section>
          <div className="rise">
            <Eyebrow>What you build with it</Eyebrow>
            <Heading>Questions your contract already answers, but can&apos;t be asked</Heading>
            <Lede>
              None of these need a contract change. The data is already on-chain; it simply
              isn&apos;t queryable.
            </Lede>
          </div>
          <div className="rise mt-12">
            <Builds />
          </div>
          <p className="rise mt-6 text-sm text-ink-500">
            Whatever your contract emits, you can fold it into a table shaped like the question you
            actually ask.
          </p>
        </Section>

        <Closing />
      </main>
      <Footer />
    </>
  );
}

function Nav() {
  return (
    <nav className="glass sticky top-0 z-20 border-b border-ink-200/60">
      <div className="mx-auto flex max-w-6xl items-center justify-between px-6 py-3.5">
        <a href="#top" className="flex items-center gap-2 text-ink-900">
          <Logo className="size-5 text-blue-600" />
          <span className="text-[15px] font-semibold tracking-tight">Nineveh</span>
        </a>
        <div className="flex items-center gap-6 text-sm">
          <a
            href="#how"
            className="hidden text-ink-500 transition-colors hover:text-ink-900 sm:block"
          >
            How it works
          </a>
          <a
            href="https://github.com/thewoodfish/Nineveh"
            className="text-ink-500 transition-colors hover:text-ink-900"
          >
            GitHub
          </a>
          <Button href="https://github.com/thewoodfish/Nineveh">Get started</Button>
        </div>
      </div>
    </nav>
  );
}

function Hero() {
  return (
    <div id="top" className="mx-auto max-w-6xl px-6 pt-16 pb-4 sm:pt-24">
      <div className="mx-auto max-w-3xl text-center">
        <a
          href="#how"
          className="inline-flex items-center gap-2 rounded-full border border-blue-200/70 bg-white/70 px-3.5 py-1.5 text-xs font-medium text-blue-700 backdrop-blur transition-colors hover:border-blue-300 hover:bg-white"
        >
          <span className="size-1.5 rounded-full bg-blue-500" />
          Built for Aptos, on the transaction stream
        </a>
        <h1 className="mt-7 text-[2.75rem] leading-[1.05] font-semibold tracking-tight text-balance text-ink-900 sm:text-[4rem]">
          A <span className="text-blue-600">live backend</span> for your Aptos contract
        </h1>
        <p className="mx-auto mt-6 max-w-xl text-lg leading-relaxed text-pretty text-ink-500">
          Point Nineveh at your contract&apos;s address. Get a database and an API that stay in sync
          with the chain — sorted, filtered, aggregated, live. No indexer to write, nothing to run.
        </p>
        <div className="mt-9 flex flex-wrap items-center justify-center gap-3">
          <Button href="https://github.com/thewoodfish/Nineveh" size="lg">
            Get started
          </Button>
          <Button href="#how" tone="quiet" size="lg">
            See how it works
          </Button>
        </div>
      </div>

      <figure className="mt-16 sm:mt-20">
        <div className="rounded-[1.4rem] border border-ink-200/60 bg-white/50 p-1.5 shadow-hero backdrop-blur">
          <Stream />
        </div>
        <figcaption className="mt-4 text-center text-xs text-ink-400">
          Every sale the contract emits, folded into the table your app queries.
        </figcaption>
      </figure>
    </div>
  );
}

function Closing() {
  return (
    <section className="mx-auto w-full max-w-6xl px-6 pb-24 sm:pb-32">
      <div className="relative overflow-hidden rounded-3xl bg-blue-600 px-8 py-16 text-center shadow-hero sm:px-16 sm:py-20">
        <div
          className="absolute inset-0 opacity-70"
          style={{
            background:
              "radial-gradient(30rem 20rem at 20% 0%, oklch(0.716 0.152 259 / 0.7), transparent 70%), radial-gradient(28rem 18rem at 85% 100%, oklch(0.412 0.158 262 / 0.8), transparent 70%)",
          }}
          aria-hidden
        />
        <div className="relative">
          <h2 className="mx-auto max-w-2xl text-3xl font-semibold tracking-tight text-balance text-white sm:text-[2.6rem] sm:leading-[1.1]">
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
  );
}

function Footer() {
  return (
    <footer className="border-t border-ink-200/70">
      <div className="mx-auto flex max-w-6xl flex-col items-center justify-between gap-4 px-6 py-8 text-sm text-ink-400 sm:flex-row">
        <div className="flex items-center gap-2">
          <Logo className="size-4 text-blue-500" />
          <span className="font-medium text-ink-700">Nineveh</span>
          <span>— a live backend for Aptos apps</span>
        </div>
        <a
          href="https://github.com/thewoodfish/Nineveh"
          className="transition-colors hover:text-ink-900"
        >
          GitHub
        </a>
      </div>
    </footer>
  );
}
