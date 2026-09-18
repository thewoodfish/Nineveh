import { Button, Code, Eyebrow, Heading, Lede, Logo, Section } from "@/components/bits";
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
    body: "REST over every table, with filters, sorting and paging, plus a live change feed and signed webhooks. Nothing reads the chain at request time.",
  },
];

const BUILDS = [
  {
    title: "Leaderboards",
    chain: "can read one player's record",
    you: "rank every player, sorted, live",
  },
  {
    title: "Marketplaces",
    chain: "listings live in a table with no address to read",
    you: "what's for sale now, and revenue per seller",
  },
  {
    title: "Token & points apps",
    chain: "one balance at a time",
    you: "every holder, sortable, with their history",
  },
  {
    title: "Feeds and activity",
    chain: "no history, no ordering",
    you: "append-only history you can page through",
  },
  {
    title: "Protocol dashboards",
    chain: "no totals, no aggregates",
    you: "volume, fees, who's near liquidation",
  },
];

const GUARANTEES = [
  { title: "In order", body: "A balance that goes 5 → 12 → 7 lands as 7, never as 12." },
  { title: "Exactly once", body: "Rows and cursor commit together, or neither does." },
  { title: "Crash-safe", body: "A restart resumes from the cursor. Nothing counted twice." },
  { title: "Exact arithmetic", body: "Move's integers, checked. A bad rule halts, it never corrupts." },
];

export default function Home() {
  return (
    <main>
      <Hero />

      <Section>
        <div className="rise">
          <Eyebrow>The problem</Eyebrow>
          <Heading>The chain can only answer one kind of question</Heading>
          <Lede>
            <em>What is X right now?</em> One account&apos;s balance. One listing by id. It can&apos;t
            sort, total, join or give you a feed — and the data your app needs isn&apos;t even in
            storage. It&apos;s in events and write sets, because keeping totals on-chain costs gas
            on every transaction.
          </Lede>
        </div>
        <div className="rise mt-12 grid gap-4 sm:grid-cols-3">
          {[
            ["Show me all of them, sorted", "Top players. Cheapest listings. Biggest holders."],
            ["What happened?", "A feed. A history. This user's last twenty actions."],
            ["How much, in total?", "Revenue per seller. Volume per day. Count per account."],
          ].map(([q, a]) => (
            <div key={q} className="rounded-2xl border border-ink-200 bg-ink-50/60 p-6">
              <div className="text-base font-semibold text-ink-900">{q}</div>
              <p className="mt-2 text-sm leading-relaxed text-ink-500">{a}</p>
            </div>
          ))}
        </div>
        <p className="rise mt-10 max-w-2xl text-lg text-ink-500">
          So every team writes an indexer: a processor, a database, a server, a deploy pipeline. A
          week of work, and something to maintain forever.{" "}
          <span className="font-medium text-ink-900">Nineveh is that week, done.</span>
        </p>
      </Section>

      <Section dark id="how">
        <div className="rise">
          <div className="text-xs font-semibold tracking-[0.12em] text-blue-400 uppercase">
            Seventeen lines
          </div>
          <Heading dark>Describe the table. Get the API.</Heading>
          <Lede dark>
            No processor to write, no migrations, no schema to keep in step. Change a rule and
            Nineveh rebuilds the table from history in the background, then swaps it in — the old
            data keeps serving the whole time.
          </Lede>
        </div>
        <div className="rise mt-12 grid items-start gap-6 lg:grid-cols-2">
          <Code title="nineveh.yaml" lines={CONFIG} />
          <div className="flex flex-col gap-4">
            <Code title="your API, a second later" lines={RESPONSE} />
            <p className="text-sm text-white/45">
              Wide integers come back as strings, because a <code className="font-mono">u128</code>{" "}
              doesn&apos;t fit a JavaScript number. Every table gets the same treatment, plus a live
              change feed and signed webhooks.
            </p>
          </div>
        </div>
      </Section>

      <Section>
        <div className="rise">
          <Eyebrow>How it works</Eyebrow>
          <Heading>Four steps, and none of them are yours</Heading>
        </div>
        <ol className="rise mt-12 grid gap-px overflow-hidden rounded-2xl bg-ink-200 sm:grid-cols-2 lg:grid-cols-4">
          {STEPS.map((step, i) => (
            <li key={step.title} className="bg-white p-6">
              <div className="flex size-7 items-center justify-center rounded-lg bg-blue-50 font-mono text-xs font-semibold text-blue-700">
                {i + 1}
              </div>
              <h3 className="mt-4 text-base font-semibold text-ink-900">{step.title}</h3>
              <p className="mt-2 text-sm leading-relaxed text-ink-500">{step.body}</p>
            </li>
          ))}
        </ol>
        <div className="rise mt-6 grid gap-4 sm:grid-cols-4">
          {GUARANTEES.map((g) => (
            <div key={g.title} className="rounded-xl border border-ink-200 p-5">
              <div className="text-sm font-semibold text-ink-900">{g.title}</div>
              <p className="mt-1.5 text-sm leading-relaxed text-ink-500">{g.body}</p>
            </div>
          ))}
        </div>
      </Section>

      <Section>
        <div className="rise">
          <Eyebrow>What you build with it</Eyebrow>
          <Heading>Questions your contract already answers — just not out loud</Heading>
          <Lede>
            None of these need a contract change. The data is already on-chain; it simply
            isn&apos;t queryable.
          </Lede>
        </div>
        <div className="rise mt-12 grid gap-4 sm:grid-cols-2 lg:grid-cols-3">
          {BUILDS.map((b) => (
            <div
              key={b.title}
              className="group rounded-2xl border border-ink-200 bg-white p-6 shadow-card transition-shadow hover:shadow-lifted"
            >
              <h3 className="text-base font-semibold text-ink-900">{b.title}</h3>
              <p className="mt-3 flex gap-2 text-sm text-ink-400">
                <span className="font-mono text-xs text-ink-300">chain</span>
                <span className="line-through decoration-ink-300">{b.chain}</span>
              </p>
              <p className="mt-1.5 flex gap-2 text-sm text-ink-700">
                <span className="font-mono text-xs text-blue-600">you</span>
                <span>{b.you}</span>
              </p>
            </div>
          ))}
          <div className="flex flex-col justify-center rounded-2xl border border-dashed border-ink-300 p-6">
            <p className="text-sm leading-relaxed text-ink-500">
              Whatever your contract emits, you can fold it into a table shaped like the question
              you actually ask.
            </p>
          </div>
        </div>
      </Section>

      <Closing />
      <Footer />
    </main>
  );
}

function Hero() {
  return (
    <div className="relative overflow-hidden bg-ink-950">
      <div className="absolute inset-0 aurora" />
      <div className="absolute inset-0 grid-lines" />
      <div className="relative">
        <Nav />
        <div className="mx-auto max-w-6xl px-6 pt-16 pb-20 sm:pt-24 sm:pb-28">
          <div className="mx-auto max-w-3xl text-center">
            <a
              href="#how"
              className="inline-flex items-center gap-2 rounded-full border border-white/15 bg-white/[0.06] px-3.5 py-1.5 text-xs text-white/70 backdrop-blur transition-colors hover:bg-white/10"
            >
              <span className="size-1.5 rounded-full bg-blue-400" />
              Built for Aptos, on the transaction stream
            </a>
            <h1 className="mt-6 text-4xl font-semibold tracking-tight text-balance text-white sm:text-6xl sm:leading-[1.05]">
              A live backend for your Aptos contract
            </h1>
            <p className="mx-auto mt-6 max-w-xl text-lg leading-relaxed text-pretty text-white/60">
              Point Nineveh at your contract&apos;s address. Get a database and an API that stay in
              sync with the chain — sorted, filtered, aggregated, live. No indexer to write, nothing
              to run.
            </p>
            <div className="mt-9 flex flex-wrap items-center justify-center gap-3">
              <Button href="#how">See how it works</Button>
              <Button href="https://github.com/thewoodfish/Nineveh" tone="ghost">
                Read the docs
              </Button>
            </div>
          </div>

          <div className="mt-16 sm:mt-20">
            <div className="mx-auto max-w-4xl rounded-[1.25rem] border border-white/10 bg-white/[0.03] p-1.5 shadow-glow backdrop-blur">
              <Stream />
            </div>
            <p className="mt-4 text-center text-xs text-white/35">
              Every sale the contract emits, folded into the table your app queries.
            </p>
          </div>
        </div>
      </div>
    </div>
  );
}

function Nav() {
  return (
    <nav className="mx-auto flex max-w-6xl items-center justify-between px-6 py-5">
      <a href="/" className="flex items-center gap-2 text-white">
        <Logo className="size-5 text-blue-400" />
        <span className="text-[15px] font-semibold tracking-tight">Nineveh</span>
      </a>
      <div className="flex items-center gap-6 text-sm">
        <a href="#how" className="hidden text-white/60 transition-colors hover:text-white sm:block">
          How it works
        </a>
        <a
          href="https://github.com/thewoodfish/Nineveh"
          className="text-white/60 transition-colors hover:text-white"
        >
          GitHub
        </a>
      </div>
    </nav>
  );
}

function Closing() {
  return (
    <section className="relative overflow-hidden bg-ink-950">
      <div className="absolute inset-0 aurora opacity-70" />
      <div className="relative mx-auto max-w-3xl px-6 py-24 text-center sm:py-32">
        <h2 className="text-3xl font-semibold tracking-tight text-balance text-white sm:text-4xl">
          You deployed the contract. The backend is the easy part now.
        </h2>
        <p className="mx-auto mt-5 max-w-xl text-lg leading-relaxed text-pretty text-white/60">
          Everything else — backfills, cursors, retries, crash recovery — is Nineveh&apos;s
          problem.
        </p>
        <ol className="mx-auto mt-10 grid max-w-2xl gap-3 text-left sm:grid-cols-3">
          {[
            ["Paste your address", "Nineveh reads the contract's modules off the chain."],
            ["Tick what to follow", "Events, resources and tables become tables of your own."],
            ["Query it", "REST, a change feed, and webhooks, a few seconds later."],
          ].map(([title, body], i) => (
            <li key={title} className="rounded-xl border border-white/10 bg-white/[0.04] p-4">
              <span className="font-mono text-xs text-blue-300">0{i + 1}</span>
              <div className="mt-2 text-sm font-medium text-white">{title}</div>
              <p className="mt-1 text-xs leading-relaxed text-white/50">{body}</p>
            </li>
          ))}
        </ol>
        <div className="mt-9 flex flex-wrap items-center justify-center gap-3">
          <Button href="https://github.com/thewoodfish/Nineveh">Get started</Button>
          <Button href="#how" tone="ghost">
            See it again
          </Button>
        </div>
      </div>
    </section>
  );
}

function Footer() {
  return (
    <footer className="border-t border-white/10 bg-ink-950">
      <div className="mx-auto flex max-w-6xl flex-col items-center justify-between gap-4 px-6 py-8 text-sm text-white/40 sm:flex-row">
        <div className="flex items-center gap-2">
          <Logo className="size-4 text-blue-400" />
          <span className="font-medium text-white/70">Nineveh</span>
          <span>— a live backend for Aptos apps</span>
        </div>
        <a
          href="https://github.com/thewoodfish/Nineveh"
          className="transition-colors hover:text-white"
        >
          GitHub
        </a>
      </div>
    </footer>
  );
}
