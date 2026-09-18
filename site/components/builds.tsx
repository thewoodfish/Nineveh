// What you build with it. Each one carries a small picture of the answer rather than a
// sentence about it — a leaderboard looks like a leaderboard, a dashboard like a chart.
// Everything here is drawn, not fetched.

import type { ReactNode } from "react";

export function Builds() {
  return (
    <div className="grid gap-5 lg:grid-cols-3">
      <Card
        title="Leaderboards"
        line="Rank every player, live, from the events your game already emits."
        query="players?order=wins.desc"
        wide
      >
        <Leaderboard />
      </Card>

      <Card
        title="Marketplaces"
        line="What's for sale right now, and what each seller has made."
        query="listings?order=price.asc"
      >
        <Listings />
      </Card>

      <Card
        title="Token & points apps"
        line="Every holder, sortable, with the history behind each balance."
        query="holders?order=balance.desc"
      >
        <Holders />
      </Card>

      <Card
        title="Feeds and activity"
        line="Append-only history you can page through, by author or by time."
        query="posts?author=0x7a3f…"
      >
        <Feed />
      </Card>

      <Card
        title="Protocol dashboards"
        line="Volume, fees and exposure — totals the contract never stored."
        query="daily_volume?order=day.desc"
      >
        <Chart />
      </Card>
    </div>
  );
}

function Card({
  title,
  line,
  query,
  wide = false,
  children,
}: {
  title: string;
  line: string;
  query: string;
  wide?: boolean;
  children: ReactNode;
}) {
  return (
    <article
      className={`rise group flex flex-col overflow-hidden rounded-2xl border border-ink-200/70 bg-white/70 backdrop-blur transition-[transform,border-color,box-shadow] hover:-translate-y-1 hover:border-blue-200 hover:shadow-card ${
        wide ? "lg:col-span-2" : ""
      }`}
    >
      <div className="p-6 pb-4">
        <h3 className="font-semibold text-ink-900">{title}</h3>
        <p className="mt-1.5 max-w-sm text-sm leading-relaxed text-ink-500">{line}</p>
      </div>
      <div className="mt-auto px-6">{children}</div>
      <div className="mt-5 flex items-center gap-2 border-t border-ink-200/60 bg-ink-50/40 px-6 py-3">
        <span className="rounded bg-blue-100 px-1.5 py-0.5 font-mono text-[10px] font-semibold text-blue-700">
          GET
        </span>
        <code className="truncate font-mono text-[11.5px] text-ink-500">{query}</code>
      </div>
    </article>
  );
}

/** A mini table: the shape of an answer, three rows deep. */
function Rows({ children }: { children: ReactNode }) {
  return <div className="flex flex-col gap-1 font-mono text-[11px]">{children}</div>;
}

function Row({
  lead,
  body,
  value,
  top = false,
}: {
  lead?: ReactNode;
  body: ReactNode;
  value: ReactNode;
  top?: boolean;
}) {
  return (
    <div
      className={`flex items-baseline gap-2.5 rounded-lg px-2.5 py-1.5 ${
        top ? "bg-blue-50 text-blue-900" : "bg-ink-50/70 text-ink-600"
      }`}
    >
      {lead && (
        <span className={`w-4 shrink-0 ${top ? "text-blue-500" : "text-ink-400"}`}>{lead}</span>
      )}
      <span className="min-w-0 flex-1 truncate">{body}</span>
      <span className={`shrink-0 tabular-nums ${top ? "font-semibold" : "text-ink-800"}`}>
        {value}
      </span>
    </div>
  );
}

function Leaderboard() {
  const players = [
    ["0x7a3f…c41d", "1,284", true],
    ["0x1e87…8d2a", "1,109", false],
    ["0x9b02…4f77", "973", false],
  ] as const;
  return (
    <Rows>
      {players.map(([who, wins, top], i) => (
        <Row key={who} lead={`${i + 1}`} body={who} value={wins} top={top} />
      ))}
    </Rows>
  );
}

function Listings() {
  return (
    <Rows>
      <Row body="brass lamp" value="694" top />
      <Row body="oak chair" value="309" />
      <Row body="wool rug" value="803" />
    </Rows>
  );
}

function Holders() {
  const holders = [
    ["0x7a3f…c41d", 100],
    ["0x1e87…8d2a", 68],
    ["0x9b02…4f77", 34],
  ] as const;
  return (
    <div className="flex flex-col gap-2">
      {holders.map(([who, share]) => (
        <div key={who} className="flex items-center gap-3 font-mono text-[11px]">
          <span className="w-24 shrink-0 truncate text-ink-600">{who}</span>
          <span className="h-1.5 flex-1 overflow-hidden rounded-full bg-ink-100">
            <span
              className="block h-full rounded-full bg-blue-500/80"
              style={{ width: `${share}%` }}
            />
          </span>
        </div>
      ))}
    </div>
  );
}

function Feed() {
  const events = [
    ["Posted", "2m"],
    ["Replied", "9m"],
    ["Posted", "31m"],
  ] as const;
  return (
    <Rows>
      {events.map(([what, when], i) => (
        <Row
          key={`${what}-${when}`}
          body={
            <span className="flex items-center gap-2">
              <span
                className={`rounded px-1.5 py-0.5 text-[10px] font-medium ${
                  i === 0 ? "bg-blue-100 text-blue-700" : "bg-ink-200/70 text-ink-500"
                }`}
              >
                {what}
              </span>
              <span className="truncate text-ink-500">0x7a3f…c41d</span>
            </span>
          }
          value={<span className="text-ink-400">{when}</span>}
        />
      ))}
    </Rows>
  );
}

function Chart() {
  // Seven days of volume, drawn as bars: the point is the shape, not the numbers.
  const days = [42, 58, 35, 71, 64, 88, 76];
  return (
    <div className="flex h-20 items-end gap-1.5">
      {days.map((height, i) => (
        <span
          key={i}
          className={`flex-1 rounded-t-sm ${i === days.length - 2 ? "bg-blue-500" : "bg-blue-200"}`}
          style={{ height: `${height}%` }}
        />
      ))}
    </div>
  );
}
