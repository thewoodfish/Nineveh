import { Button, Code, Heading, Lede, Logo, Register, Section } from "@/components/bits";
import { Pricing } from "@/components/pricing";
import { Shots } from "@/components/shots";
import { Builds } from "@/components/builds";
import { Nav } from "@/components/nav";
import { Pipeline, Stack } from "@/components/layers";
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

/*
 * The workflow, as the developer does it — four things to describe, in the order you
 * describe them. This is the page's one list of what Nineveh is for, and it stays four
 * items long: a feature grid would say less.
 */
const WORKFLOW = [
  {
    title: "Define your application state",
    body: "Describe the tables your product needs — keys, columns, types — rather than building an indexing pipeline from scratch.",
  },
  {
    title: "React to Aptos activity",
    body: "Attach logic to the events and state changes your Move contracts already produce. Resources and tables too, not only events.",
  },
  {
    title: "Derive useful state",
    body: "Fold that activity into application-level state: counters, totals, relationships, computed columns.",
  },
  {
    title: "Give your frontend a backend",
    body: "Query the result over REST, subscribe to changes as they commit, or receive signed webhooks.",
  },
];

/*
 * What's underneath, for the developer who wants to know before they trust it. Every line
 * here is something the current build does — no numbers, no latency, no claims the
 * product can't be made to demonstrate on request.
 */
const UNDERNEATH = [
  {
    label: "INPUT",
    body: "Aptos' Transaction Stream: every transaction in commit order, with its events and its write set — the exact storage slots it changed. Nothing to poll, nothing to schedule.",
  },
  {
    label: "PROCESSING",
    body: "A deterministic fold. Same records in, same rows out, so replaying your history produces exactly the state you have now.",
  },
  {
    label: "STORAGE",
    body: "PostgreSQL, a schema per build, written only by your own rules. Move's wide integers are kept exact.",
  },
  {
    label: "OUTPUT",
    body: "REST over every table, a change feed over SSE that resumes from where you left it, and signed webhooks.",
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
        <Layers />
        <Machinery />
        <Shots />
        <BuiltFor />
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
        <h1 className=" font-display text-4xl leading-[1.04] font-semibold tracking-[-0.022em] text-balance text-white sm:text-5xl">
          Your backend for Aptos.
        </h1>
        <p className="mx-auto mt-6 max-w-xl text-lg leading-relaxed text-pretty text-white/60">
          Build the backend for your Aptos app without building the infrastructure around it.
        </p>
        <p className="mx-auto mt-4 max-w-xl leading-relaxed text-pretty text-white/50">
          Connect your Move contracts to application state, backend logic and APIs — on the Aptos
          infrastructure you already know.
        </p>
        <div className="mt-9 flex flex-wrap items-center justify-center gap-3">
          <Button href="https://studio.nineveh.dev" size="lg">
            Start building
          </Button>
          <Button href="/docs" tone="quiet" size="lg">
            Read the docs
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

/*
 * The gap, stated without a villain in it. Aptos' data infrastructure does what it is for;
 * what's missing above it is application-specific, which is why every team writes it
 * again. That's the sentence the whole page turns on.
 */
function Problem() {
  return (
    <Section id="problem" rule={false}>
      <Register at="problem">
        <Heading>Aptos gives you the primitives. The application backend is still yours.</Heading>

        <div className="mt-10 grid items-start gap-12 lg:grid-cols-[1fr_auto] lg:gap-16">
          <div>
            <p className="max-w-[62ch] text-lg leading-relaxed text-pretty text-white/55">
              Aptos already provides the infrastructure for reading and indexing on-chain data: RPC
              for point reads, the hosted Indexer for common assets, the Transaction Stream for
              everything a transaction touched.
            </p>
            <p className="mt-5 max-w-[62ch] text-white/55">
              Turning that into the backend your frontend needs is still application logic, state
              management, database work and server code — and the data it needs mostly isn&apos;t
              in storage to begin with. It lives in events and write sets, because keeping totals
              on-chain costs gas on every transaction.
            </p>
            <p className="mt-5 max-w-[62ch] text-white/55">
              So it gets assembled the same way in every project: a processor, a database, a server,
              a deploy pipeline. A week of work, and something to maintain forever.{" "}
              <span className="font-medium text-white">
                Nineveh simplifies the layer in the middle.
              </span>
            </p>
          </div>

          <div className="lg:w-[19rem]">
            <Pipeline />
          </div>
        </div>

        {/* The three asks, set as an editorial list rather than boxed up as cards. */}
        <p className="mt-16 text-sm text-white/40">
          The questions an app asks, that a point read can&apos;t answer:
        </p>
        <dl className="mt-5 border-t border-white/10">
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

/*
 * Where Nineveh sits. This section exists to be unambiguous about one thing: nothing
 * underneath is being replaced. It's the first question an Aptos developer has, and
 * leaving it to inference invites the wrong answer.
 */
function Layers() {
  return (
    <Section id="layers">
      <Register at="layers">
        <div className="mx-auto max-w-3xl text-center">
          <Heading center>Built on Aptos. Not around it.</Heading>
          <Lede center>
            Aptos already provides the infrastructure for accessing chain data and building custom
            indexing pipelines. Nineveh sits one layer above it. Instead of every application
            developer assembling the same backend logic around those primitives, Nineveh gives you a
            simpler way to define and serve application-specific state.
          </Lede>
        </div>

        <div className="mt-14">
          <Stack />
        </div>

        <div className="mx-auto mt-20 max-w-3xl border-t border-white/10 pt-12 text-center">
          <h3 className="font-display text-2xl font-semibold tracking-[-0.015em] text-white">
            A familiar backend experience for Aptos.
          </h3>
          <p className="mx-auto mt-4 max-w-[58ch] leading-relaxed text-pretty text-white/55">
            Web developers have familiar abstractions for building application backends. Nineveh
            brings that simplicity to Aptos applications — while keeping Aptos as the source of
            truth.
          </p>
        </div>
      </Register>
    </Section>
  );
}

/** The deep act: the promise, the code that keeps it, and what runs underneath. */
function Machinery() {
  return (
    <section id="how" className="deep">
      <div className="mx-auto max-w-6xl px-6 pt-44 pb-40 sm:pt-52 sm:pb-48">
        <Register at="how">
          <div className="mx-auto max-w-3xl text-center">
            <Heading center>From on-chain state to application state.</Heading>
            <Lede center>
              Your contract is the source of truth. A reducer says what to do when something
              arrives: when this event lands, this row changes. No processor to write, no
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
                doesn&apos;t fit a JavaScript number. Every table gets the same treatment, plus a
                live change feed and signed webhooks.
              </p>
            </div>
          </div>

          <div className="mt-32">
            <Heading>Describe the data you want. Get the API.</Heading>
          </div>
          {/* A rail runs through the four, with a pulse travelling it: one movement, not
              four boxes. */}
          <ol className="relative mt-14 grid gap-10 sm:grid-cols-2 lg:grid-cols-4 lg:gap-8">
            <div className="absolute top-3.5 right-0 left-0 hidden lg:block" aria-hidden>
              <div className="h-px w-full bg-gradient-to-r from-blue-400/50 via-blue-400/25 to-transparent" />
              <span className="travel absolute -top-[3px] size-[7px] rounded-full bg-blue-200 shadow-[0_0_14px_4px_oklch(0.716_0.152_259_/_0.65)]" />
            </div>
            {WORKFLOW.map((step, i) => (
              <li key={step.title} className="relative">
                <div className="flex size-7 items-center justify-center rounded-full border border-white/15 bg-deep font-mono text-[11px] font-semibold text-blue-200">
                  {i + 1}
                </div>
                <h3 className="mt-5 font-semibold text-white">{step.title}</h3>
                <p className="mt-2 text-sm leading-relaxed text-white/50">{step.body}</p>
              </li>
            ))}
          </ol>

          {/* What it's made of, then what it promises — the page stops selling here. */}
          <div className="mt-32">
            <Heading>Your data. Your logic. Your backend.</Heading>
          </div>
          <dl className="mt-12 grid gap-x-16 gap-y-8 sm:grid-cols-2">
            {UNDERNEATH.map((part) => (
              <div key={part.label} className="border-t border-white/10 pt-5">
                <dt className="font-mono text-[11px] font-semibold tracking-[0.06em] text-clay-400">
                  {part.label}
                </dt>
                <dd className="mt-3 text-sm leading-relaxed text-white/55">{part.body}</dd>
              </div>
            ))}
          </dl>

          <div className="mt-20 grid gap-10 border-t border-white/10 pt-12 sm:grid-cols-2 lg:grid-cols-4 lg:gap-12">
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

/** Who it's for, shown as the shapes they build rather than listed as verticals. */
function BuiltFor() {
  return (
    <Section id="built-for" rule={false}>
      <Register at="built-for">
        <div className="mx-auto max-w-3xl text-center">
          <Heading center>Built for Aptos application developers.</Heading>
          <Lede center>
            DeFi, marketplaces, games, social and consumer apps, agents, dashboards. The shapes
            differ; the need doesn&apos;t. Each one wants application state derived from what its
            contracts do, queryable the way the product actually asks for it.
          </Lede>
        </div>
        {/* Each card carries its own `rise`, so they arrive as you reach them. */}
        <div className="mt-14">
          <Builds />
        </div>
        <p className="mt-8 text-center text-sm text-white/45">
          Whatever your contract emits, you can fold it into a table shaped like the question you
          actually ask. None of these need a contract change.
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
              Build the backend. Build the app.
            </h2>
            <p className="mx-auto mt-5 max-w-xl text-lg leading-relaxed text-pretty text-blue-50/90">
              Start with Aptos. Let Nineveh handle the application layer — backfills, cursors,
              retries and crash recovery included.
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
            <div className="mt-10 flex flex-wrap items-center justify-center gap-3">
              <a
                href="https://studio.nineveh.dev"
                className="inline-flex items-center justify-center rounded-xl bg-white px-6 py-3 text-sm font-semibold text-blue-700 shadow-card transition-colors hover:bg-blue-50"
              >
                Start building
              </a>
              <a
                href="/docs"
                className="inline-flex items-center justify-center rounded-xl border border-white/30 px-6 py-3 text-sm font-semibold text-white transition-colors hover:bg-white/10"
              >
                Read the docs
              </a>
            </div>
          </div>
        </div>
      </section>

      <footer className="mx-auto flex max-w-6xl flex-col items-center justify-between gap-4 px-6 pt-10 pb-12 text-sm text-white/40 sm:flex-row">
        <div className="flex items-center gap-2">
          <Logo className="size-4 text-blue-400" />
          <span className="font-medium text-white/80">Nineveh</span>
          <span>— an application backend for Aptos</span>
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
