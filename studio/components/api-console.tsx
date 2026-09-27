"use client";

/**
 * The requests a developer would make against their own state, ready to run and copy.
 *
 * It used to be one request wrapped in a form: pick a table, add filters, set a limit,
 * press Run. That form could build exactly one kind of request and made it look like it
 * could build any — so it invited the two things the API doesn't do, ranges and
 * aggregates, and gave no help with the things it does. What a developer needs on this
 * page isn't a builder; it's the half-dozen requests their app is going to make, named
 * for what they're for, with their own data in them.
 *
 * Shared by the API page and a table's API tab. The two differ by one thing — whether
 * the table is theirs to choose or already decided — so they differ by one prop.
 */

import Link from "next/link";
import { useEffect, useMemo, useRef, useState } from "react";

import { type Change, type Table, getPath } from "@/lib/api";
import { type Recipe, projectRecipes, ready, tableRecipes } from "@/lib/api-recipes";
import { useFeed } from "@/lib/hooks";
import { useHref, useProject } from "@/lib/project";

import { Button, Card, Icon, OpBadge, Segmented, Select, field } from "./ui";

/** What a run came back with. */
type Result = { ok: boolean; body: string; ms: number };

const TABS = ["Requests", "Live", "GraphQL"] as const;
type Tab = (typeof TABS)[number];

export function ApiConsole({
  table,
  pick,
}: {
  table: Table;
  /** Render a table picker. Omitted where the table is already the subject of the page. */
  pick?: { tables: Table[]; onPick: (name: string) => void };
}) {
  const [tab, setTab] = useState<Tab>("Requests");

  return (
    <div className="flex min-h-0 w-full max-w-4xl flex-1 flex-col gap-5">
      <div className="flex flex-wrap items-center justify-between gap-3">
        <Segmented
          options={TABS}
          value={tab}
          onChange={setTab}
          // Visible rather than hidden: GraphQL is in the config already, and a choice
          // someone can see coming reads as a roadmap where a missing one reads as a
          // product that can't do it.
          unavailable={["GraphQL"]}
          unavailableHint="GraphQL is coming; REST serves every table today"
          unavailableBadge={() => "soon"}
        />
        {pick && (
          <label className="flex items-center gap-2 text-xs font-medium text-on-surface-variant">
            Table
            <Select
              value={table.name}
              onChange={(e) => pick.onPick(e.target.value)}
              className="font-mono"
            >
              {pick.tables.map((t) => (
                <option key={t.name} value={t.name}>
                  {t.name}
                </option>
              ))}
            </Select>
          </label>
        )}
      </div>

      {tab === "Requests" ? <Requests table={table} /> : <LiveTab table={table} />}
    </div>
  );
}

/** Every request worth making, this project's first and then this table's. */
function Requests({ table }: { table: Table }) {
  const { hosted } = useProject();
  const project = useMemo(projectRecipes, []);
  const own = useMemo(() => tableRecipes(table), [table]);
  // One answer on screen at a time, under the recipe it belongs to: a page of eight
  // response panes is a page nobody reads.
  const [shown, setShown] = useState<{ id: string; result: Result } | null>(null);

  return (
    <div className="flex min-h-0 flex-col gap-5 overflow-y-auto pb-6">
      {hosted && <KeyNote />}

      <Group title="This project">
        {project.map((recipe) => (
          <RecipeCard
            key={recipe.id}
            recipe={recipe}
            result={shown?.id === recipe.id ? shown.result : null}
            onResult={(result) => setShown({ id: recipe.id, result })}
          />
        ))}
      </Group>

      <Group title={table.name} mono>
        {own.map((recipe) => (
          <RecipeCard
            key={`${table.name}:${recipe.id}`}
            recipe={recipe}
            result={shown?.id === recipe.id ? shown.result : null}
            onResult={(result) => setShown({ id: recipe.id, result })}
          />
        ))}
      </Group>
    </div>
  );
}

function Group({
  title,
  mono = false,
  children,
}: {
  title: string;
  mono?: boolean;
  children: React.ReactNode;
}) {
  return (
    <section>
      {/* A table's name is an identifier, so it is set as one. Uppercasing it the way a
          section label is uppercased would print a name the config doesn't contain. */}
      <h2
        className={`text-xs font-medium text-on-surface-variant ${
          mono ? "font-mono" : "tracking-[0.08em] uppercase"
        }`}
      >
        {title}
      </h2>
      <div className="mt-2 flex flex-col gap-2">{children}</div>
    </section>
  );
}

/**
 * Where the key goes, rather than a key.
 *
 * The snippets say `$NINEVEH_KEY`, which is both the shape of the header and a shell
 * variable that works when it is set — so what gets copied is a line that runs, and
 * nobody's key ends up in a screenshot of this page.
 */
function KeyNote() {
  const href = useHref();
  return (
    <p className="rounded-md bg-surface-container-high px-4 py-3 text-xs leading-relaxed text-on-surface-variant">
      Every request to this project takes one of its API keys:{" "}
      <span className="font-mono text-on-surface">Authorization: Bearer nvk_…</span>. The snippets
      below read it from <span className="font-mono text-on-surface">$NINEVEH_KEY</span> so what you
      copy is a line that runs. Create and revoke keys in{" "}
      <Link href={href("/settings")} className="text-primary underline-offset-2 hover:underline">
        Settings
      </Link>
      .
    </p>
  );
}

/** One request: what it's for, what it needs, and what came back. */
function RecipeCard({
  recipe,
  result,
  onResult,
}: {
  recipe: Recipe;
  result: Result | null;
  onResult: (result: Result) => void;
}) {
  const { base, hosted } = useProject();
  const [values, setValues] = useState<Record<string, string>>(recipe.defaults);
  const [running, setRunning] = useState(false);
  const [copied, setCopied] = useState("");

  const path = recipe.path(values);
  const url = `${base ?? ""}${path}`;
  const can = ready(recipe, values);

  const snippets: Record<string, string> = {
    URL: url,
    curl: hosted
      ? `curl -H "Authorization: Bearer $NINEVEH_KEY" \\\n  '${url}'`
      : `curl '${url}'`,
    fetch: hosted
      ? `const res = await fetch('${url}', {\n  headers: { Authorization: \`Bearer \${process.env.NINEVEH_KEY}\` },\n})\nconst { rows } = await res.json()`
      : `const res = await fetch('${url}')\nconst { rows } = await res.json()`,
  };

  const copy = (what: string) => {
    void navigator.clipboard?.writeText(snippets[what] ?? "");
    setCopied(what);
    setTimeout(() => setCopied(""), 1500);
  };

  const run = async () => {
    setRunning(true);
    const started = performance.now();
    try {
      if (!base) throw new Error("Open a project first");
      const body = await getPath(base, path);
      onResult({
        ok: true,
        body: JSON.stringify(body, null, 2),
        ms: performance.now() - started,
      });
    } catch (e) {
      onResult({
        ok: false,
        body: e instanceof Error ? e.message : String(e),
        ms: performance.now() - started,
      });
    } finally {
      setRunning(false);
    }
  };

  return (
    <Card className="p-4">
      <div className="flex flex-wrap items-baseline justify-between gap-x-4 gap-y-1">
        <h3 className="text-sm font-medium text-on-surface">{recipe.title}</h3>
        <Button size="sm" tone="tonal" disabled={running || !can} onClick={() => void run()}>
          {running ? "Running…" : "Run"}
        </Button>
      </div>
      <p className="mt-1 max-w-2xl text-xs leading-relaxed text-on-surface-variant">{recipe.why}</p>

      {recipe.needs.length > 0 && (
        <div className="mt-3 flex flex-wrap items-end gap-2">
          {recipe.needs.map((need) => (
            <label key={need.id} className="flex flex-col gap-1">
              <span className="font-mono text-[11px] text-on-surface-variant">{need.label}</span>
              {need.columns ? (
                <Select
                  value={values[need.id] ?? ""}
                  onChange={(e) => setValues({ ...values, [need.id]: e.target.value })}
                  className="h-9 py-0 font-mono text-xs"
                >
                  {need.columns.map((column) => (
                    <option key={column} value={column}>
                      {column}
                    </option>
                  ))}
                </Select>
              ) : (
                <input
                  value={values[need.id] ?? ""}
                  placeholder={need.placeholder}
                  spellCheck={false}
                  onChange={(e) => setValues({ ...values, [need.id]: e.target.value })}
                  className={`${field} h-9 w-52 py-0 font-mono text-xs`}
                />
              )}
            </label>
          ))}
        </div>
      )}

      <div className="mt-3 flex items-center gap-2 rounded-sm bg-surface-container-high px-2.5 py-1.5">
        <span className="shrink-0 font-mono text-[11px] font-medium text-on-tertiary-container">
          GET
        </span>
        <span className="min-w-0 flex-1 truncate font-mono text-xs text-on-surface" title={url}>
          {path}
        </span>
        {Object.keys(snippets).map((what) => (
          <button
            key={what}
            type="button"
            onClick={() => copy(what)}
            className="state shrink-0 rounded-full px-2 py-0.5 font-mono text-[11px] text-primary"
          >
            {copied === what ? "copied" : what}
          </button>
        ))}
      </div>

      {result && (
        <div className="mt-3">
          <div className="flex items-baseline gap-2 text-[11px]">
            <span className={result.ok ? "text-on-tertiary-container" : "text-error"}>
              {result.ok ? "200 OK" : "failed"}
            </span>
            <span className="font-mono text-on-surface-variant tnum">
              {result.ms.toFixed(0)} ms
            </span>
          </div>
          <pre className="mt-1 max-h-72 overflow-auto rounded-sm bg-surface-container-high px-3 py-2 font-mono text-xs leading-relaxed whitespace-pre-wrap">
            {result.body}
          </pre>
        </div>
      )}
    </Card>
  );
}

/**
 * The change feed: the one request that doesn't end.
 *
 * On its own tab because it is a different thing from the rest — the app opens a
 * connection and Nineveh writes down it until the page closes — and because the
 * authentication works differently enough to need saying.
 */
function LiveTab({ table }: { table: Table }) {
  const { base, hosted } = useProject();
  const href = useHref();
  const [copied, setCopied] = useState(false);
  const [seen, setSeen] = useState<Change[]>([]);
  const feed = `${base ?? ""}/v1/changes?tables=${encodeURIComponent(table.name)}`;
  const shown = hosted ? `${feed}&apikey=nvk_…` : feed;
  const code = `const feed = new EventSource('${shown}')\nfeed.addEventListener('change', (e) => {\n  const { table, op, key, row } = JSON.parse(e.data)\n})`;

  // The table can change under this tab, and changes to the last one aren't this one's.
  const of = table.name;
  useEffect(() => setSeen([]), [of]);
  const latest = useRef(of);
  latest.current = of;
  const connected = useFeed({
    tables: [of],
    onChanges: (changes) => {
      if (latest.current !== of) return;
      setSeen((had) => [...changes, ...had].slice(0, 12));
    },
  });

  return (
    <div className="flex min-h-0 flex-col gap-5 overflow-y-auto pb-6">
      <Card className="p-4">
        <div className="flex flex-wrap items-baseline justify-between gap-x-4 gap-y-1">
          <h3 className="text-sm font-medium text-on-surface">
            Every change to <span className="font-mono">{table.name}</span>, as it happens
          </h3>
          <button
            type="button"
            onClick={() => {
              void navigator.clipboard?.writeText(code);
              setCopied(true);
              setTimeout(() => setCopied(false), 1500);
            }}
            className="state shrink-0 rounded-full px-2 py-0.5 font-mono text-[11px] text-primary"
          >
            {copied ? "copied" : "copy"}
          </button>
        </div>
        <p className="mt-1 max-w-2xl text-xs leading-relaxed text-on-surface-variant">
          Server-sent events, so it reconnects by itself and resumes where it left off. For a
          screen someone is watching. For your backend, where a change has to land whether anyone
          is watching or not,{" "}
          <Link href={href("/webhooks")} className="text-primary underline-offset-2 hover:underline">
            webhooks
          </Link>{" "}
          are retried until your server answers.
        </p>
        <pre className="mt-3 overflow-x-auto rounded-sm bg-surface-container-high px-3 py-2 font-mono text-xs leading-relaxed">
          {code}
        </pre>
        <p className="mt-2 text-[11px] leading-relaxed text-on-surface-variant">
          {hosted ? (
            <>
              The key is in the URL here because{" "}
              <span className="font-mono">EventSource</span> can&apos;t send headers. Everything
              else takes it as a header — prefer that, and keep this form out of anything that logs
              URLs.
            </>
          ) : (
            <>
              Local mode is loopback-only and takes no key. A hosted plane wants one of the
              project&apos;s API keys on every request.
            </>
          )}
        </p>
      </Card>

      <Card className="p-4">
        <div className="flex items-center justify-between gap-3">
          <h3 className="text-sm font-medium text-on-surface">What&apos;s arriving now</h3>
          <span className="text-[11px] text-on-surface-variant">
            {connected ? "connected" : "connecting…"}
          </span>
        </div>
        {seen.length === 0 ? (
          <p className="mt-2 text-xs text-on-surface-variant">
            Nothing yet. A change appears here the moment a reducer writes one — the same event
            the snippet above receives.
          </p>
        ) : (
          <ul className="mt-2 divide-y divide-outline-variant">
            {seen.map((change) => (
              <li
                key={`${change.version}.${change.seq}`}
                className="flex items-baseline gap-2 py-1.5 text-xs"
              >
                <OpBadge op={change.op} />
                <code className="min-w-0 flex-1 truncate font-mono text-on-surface-variant">
                  {JSON.stringify(change.key)}
                </code>
                <span className="shrink-0 font-mono text-[11px] text-on-surface-variant tnum">
                  {change.version}.{change.seq}
                </span>
              </li>
            ))}
          </ul>
        )}
        <Link
          href={href("/changes")}
          className="mt-3 inline-flex items-center gap-1 text-xs text-primary hover:underline"
        >
          Every table&apos;s changes
          <Icon name="arrow_forward" className="text-[14px]" />
        </Link>
      </Card>
    </div>
  );
}
