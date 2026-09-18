"use client";

import { useRouter } from "next/navigation";
import { useState } from "react";

import { Button, Card, Field, Notice, PageHeader, Segmented, field } from "@/components/ui";
import {
  ApiError,
  type Catalog,
  type CatalogItem,
  type Network,
  type Start,
  control,
} from "@/lib/api";
import { useProject } from "@/lib/project";

const KINDS: {
  kind: CatalogItem["kind"];
  title: string;
  becomes: string;
  hint: string;
}[] = [
  {
    kind: "event",
    title: "Events",
    becomes: "log",
    hint: "a row per event, in order",
  },
  {
    kind: "resource",
    title: "Resources",
    becomes: "mirror",
    hint: "the latest value at each address",
  },
  {
    kind: "table",
    title: "Tables",
    becomes: "mirror",
    hint: "the latest value of each item",
  },
];

type Failure = { message: string; details?: string };

function failure(e: unknown): Failure {
  if (e instanceof ApiError) return { message: e.message, details: e.details };
  return { message: e instanceof Error ? e.message : String(e) };
}

/** From a contract address to a live backend: inspect, pick, name, create. */
export default function NewProject() {
  const { mode, projects, refresh } = useProject();
  const router = useRouter();
  const [network, setNetwork] = useState<Network>("testnet");
  const [address, setAddress] = useState("");
  const [catalog, setCatalog] = useState<Catalog | null>(null);
  const [inspecting, setInspecting] = useState(false);
  const [picked, setPicked] = useState<Set<string>>(new Set());
  const [search, setSearch] = useState("");
  const [choosing, setChoosing] = useState(false);
  const [name, setName] = useState("");
  const [start, setStart] = useState<Start>("auto");
  const [preview, setPreview] = useState<string | null>(null);
  const [creating, setCreating] = useState(false);
  const [error, setError] = useState<Failure | null>(null);

  if (mode === "single") {
    return (
      <div className="mx-auto mt-24 max-w-md px-6 text-center text-sm text-dim">
        Creating projects needs the control plane. Run{" "}
        <span className="font-mono text-white">nineveh up</span> instead of{" "}
        <span className="font-mono">nineveh run --serve</span>.
      </div>
    );
  }

  const inspect = async () => {
    setInspecting(true);
    setError(null);
    setCatalog(null);
    setPreview(null);
    try {
      const found = await control.inspect(network, address.trim());
      setCatalog(found);
      // Everything the contract offers: the raw tables are what Nineveh is for, and
      // narrowing them is a choice, not a chore.
      setPicked(new Set(found.items.filter((i) => !i.unsupported).map((i) => i.id)));
      setName(suggestName(found, projects?.map((p) => p.name) ?? []));
    } catch (e) {
      setError(failure(e));
    } finally {
      setInspecting(false);
    }
  };

  const draft = {
    name,
    network,
    start,
    picks: catalog?.items.filter((i) => picked.has(i.id)).map((i) => i.id) ?? [],
  };

  const showPreview = async () => {
    setError(null);
    try {
      setPreview((await control.scaffold(draft)).config);
    } catch (e) {
      setError(failure(e));
    }
  };

  const create = async () => {
    setCreating(true);
    setError(null);
    try {
      const { config } = await control.scaffold(draft);
      const created = await control.create(config);
      await refresh();
      router.push(`/?project=${encodeURIComponent(created.name)}`);
    } catch (e) {
      setError(failure(e));
      setCreating(false);
    }
  };

  return (
    <div>
      <PageHeader title="New project" />
      <div className="mx-auto flex max-w-4xl flex-col gap-8 px-8 pt-10 pb-16">
        {/* Until there's an address, it's the only thing on the page. */}
        <section className={catalog ? "" : "pt-6 text-center"}>
          {!catalog && (
            <>
              <h2 className="text-3xl font-semibold tracking-tight text-balance">
                Point Nineveh at your contract
              </h2>
              <p className="mx-auto mt-3 max-w-lg text-dim text-pretty">
                Paste the address your Move modules are published at. Nineveh reads them off the
                chain and shows you what it can follow — no config to write.
              </p>
            </>
          )}
          <form
            className={`mt-7 flex flex-col gap-3 sm:flex-row ${catalog ? "" : "mx-auto max-w-2xl"}`}
            onSubmit={(e) => {
              e.preventDefault();
              void inspect();
            }}
          >
            <Segmented
              options={["mainnet", "testnet", "devnet"] as const}
              value={network}
              onChange={(n) => setNetwork(n)}
            />
            <input
              value={address}
              onChange={(e) => setAddress(e.target.value)}
              placeholder="0x…"
              spellCheck={false}
              autoFocus
              className={`${field} min-w-0 flex-1 py-2.5 font-mono`}
            />
            <Button type="submit" tone="primary" size="lg" disabled={inspecting || !address.trim()}>
              {inspecting ? "Reading modules…" : "Inspect"}
            </Button>
          </form>
          {!catalog && !inspecting && (
            <p className="mt-4 text-xs text-faint">
              Nothing is created yet. You&apos;ll see what the contract offers first.
            </p>
          )}
        </section>

        {error && !creating && (
          <Notice tone="error" title={error.message}>
            {error.details && (
              <pre className="mt-1 overflow-x-auto font-mono text-xs whitespace-pre">
                {error.details}
              </pre>
            )}
          </Notice>
        )}

        {catalog && (
          <div className="reveal flex flex-col gap-8">
            <Step
              n={1}
              title="What Nineveh will follow"
              hint={`${catalog.modules.length} module${catalog.modules.length === 1 ? "" : "s"} at ${shortAddress(catalog.address)}.`}
            >
              <Following catalog={catalog} picked={picked} />
              <button
                type="button"
                onClick={() => setChoosing(!choosing)}
                className="mt-2 text-sm font-medium text-blue-300 hover:underline"
              >
                {choosing ? "Hide the list" : "Choose what to follow"}
              </button>
              {choosing && (
                <input
                  value={search}
                  onChange={(e) => setSearch(e.target.value)}
                  placeholder="Filter by name or module"
                  className={`${field} mt-3 mb-3 w-full`}
                />
              )}
              <div className={`flex flex-col gap-4 ${choosing ? "" : "hidden"}`}>
                {KINDS.map(({ kind, title, becomes, hint }) => (
                  <Group
                    key={kind}
                    title={title}
                    hint={`${becomes} table: ${hint}`}
                    items={catalog.items.filter(
                      (i) =>
                        i.kind === kind &&
                        `${i.module}::${i.name}`
                          .toLowerCase()
                          .includes(search.trim().toLowerCase()),
                    )}
                    becomes={becomes}
                    picked={picked}
                    setPicked={setPicked}
                  />
                ))}
              </div>
            </Step>

            <Step n={2} title="Name and history">
              <div className="flex flex-col gap-4">
                <Field
                  label="Project name"
                  hint={
                    <>
                      Lowercase letters, digits and <span className="font-mono">_</span>. It names
                      your API and your Postgres schema.
                    </>
                  }
                  className="max-w-sm"
                >
                  <input
                    value={name}
                    onChange={(e) => setName(e.target.value)}
                    spellCheck={false}
                    className={`${field} font-mono`}
                  />
                </Field>
                <div className="grid gap-2 sm:grid-cols-2">
                  <Choice
                    active={start === "auto"}
                    onClick={() => setStart("auto")}
                    title="All of its history"
                    body="From the contract's first transaction. Nineveh backfills first, which can take hours for a busy contract."
                  />
                  <Choice
                    active={start === "now"}
                    onClick={() => setStart("now")}
                    title="From now on"
                    body="Live data only, starting at the chain's current version. Ready in seconds."
                  />
                </div>
              </div>
            </Step>

            <div className="sticky bottom-0 -mx-8 flex flex-wrap items-center gap-3 border-t border-line bg-page/85 px-8 py-4 backdrop-blur">
              <Button
                tone="primary"
                size="lg"
                disabled={creating || draft.picks.length === 0 || !name}
                onClick={() => void create()}
              >
                {creating
                  ? "Pinning layouts and starting…"
                  : `Create backend with ${draft.picks.length} ${draft.picks.length === 1 ? "table" : "tables"}`}
              </Button>
              <Button
                disabled={creating || draft.picks.length === 0}
                onClick={() => void (preview ? setPreview(null) : showPreview())}
              >
                {preview ? "Hide config" : "Preview config"}
              </Button>
              {creating && error && <span className="text-sm text-red-300">{error.message}</span>}
            </div>
            {creating && error?.details && (
              <pre className="overflow-x-auto rounded-lg bg-red-500/15 p-3 font-mono text-xs text-red-200">
                {error.details}
              </pre>
            )}
            {preview && (
              <Card className="overflow-hidden">
                <div className="border-b border-line px-4 py-2 text-xs text-dim">
                  <span className="font-mono">nineveh.yaml</span>: what Nineveh will run. You can
                  edit it after creating, from the project&apos;s Config.
                </div>
                <pre className="max-h-96 overflow-auto px-4 py-3 font-mono text-xs leading-relaxed">
                  {preview}
                </pre>
              </Card>
            )}
          </div>
        )}
      </div>
    </div>
  );
}

/** What the raw tables will be: one per event, resource and table it follows. */
function Following({ catalog, picked }: { catalog: Catalog; picked: Set<string> }) {
  const counts = { event: 0, resource: 0, table: 0 };
  let unsupported = 0;
  for (const item of catalog.items) {
    if (item.unsupported) unsupported += 1;
    else if (picked.has(item.id)) counts[item.kind] += 1;
  }
  const parts = [
    [counts.event, "event", "a log table each, in order"],
    [counts.resource, "resource", "a mirror of the value at each address"],
    [counts.table, "table", "a mirror of each item"],
  ] as const;
  const total = counts.event + counts.resource + counts.table;
  return (
    <div className="flex flex-col gap-1.5 text-sm">
      {parts
        .filter(([n]) => n > 0)
        .map(([n, what, how]) => (
          <div key={what}>
            <span className="font-medium">
              {n} {what}
              {n === 1 ? "" : "s"}
            </span>
            <span className="text-dim"> — {how}</span>
          </div>
        ))}
      {total === 0 && <div className="text-dim">Nothing ticked: pick something below.</div>}
      {unsupported > 0 && (
        <div className="text-xs text-faint">
          {unsupported} more Nineveh can&apos;t follow yet, listed below with the reason.
        </div>
      )}
      {total > 40 && (
        <div className="text-xs text-amber-300">
          That&apos;s a lot of tables for one project. Narrowing it makes the first build quicker,
          and you can add sources later.
        </div>
      )}
      <div className="text-xs text-dim">
        Then build your own state tables from these, in the project.
      </div>
    </div>
  );
}

function Step({
  n,
  title,
  hint,
  children,
}: {
  n: number;
  title: string;
  hint?: string;
  children: React.ReactNode;
}) {
  return (
    <section>
      <div className="mb-4 flex items-baseline gap-3">
        <span className="flex size-6 shrink-0 items-center justify-center rounded-full bg-blue-500/15 text-xs font-semibold text-blue-300">
          {n}
        </span>
        <div>
          <h2 className="text-base font-semibold tracking-tight">{title}</h2>
          {hint && <p className="mt-0.5 text-sm text-dim">{hint}</p>}
        </div>
      </div>
      <div className="pl-9">{children}</div>
    </section>
  );
}

function Group({
  title,
  hint,
  items,
  becomes,
  picked,
  setPicked,
}: {
  title: string;
  hint: string;
  items: CatalogItem[];
  becomes: string;
  picked: Set<string>;
  setPicked: (next: Set<string>) => void;
}) {
  if (items.length === 0) return null;
  const followable = items.filter((i) => !i.unsupported);
  const all = followable.every((i) => picked.has(i.id));
  const toggleAll = () => {
    const next = new Set(picked);
    for (const i of followable) {
      if (all) next.delete(i.id);
      else next.add(i.id);
    }
    setPicked(next);
  };
  const toggle = (id: string) => {
    const next = new Set(picked);
    if (next.has(id)) next.delete(id);
    else next.add(id);
    setPicked(next);
  };
  return (
    <Card className="overflow-hidden">
      <div className="flex items-center justify-between gap-3 border-b border-line bg-well px-4 py-2.5">
        <div className="text-sm">
          <span className="font-medium">{title}</span>{" "}
          <span className="text-xs text-faint">
            {followable.filter((i) => picked.has(i.id)).length} of {items.length} · {hint}
          </span>
        </div>
        {followable.length > 0 && (
          <button
            type="button"
            onClick={toggleAll}
            className="text-xs font-medium text-blue-300 hover:underline"
          >
            {all ? "None" : "All"}
          </button>
        )}
      </div>
      <ul className="divide-y divide-line">
        {items.map((item) => (
          <li key={item.id}>
            <label
              className={`flex items-start gap-3 px-4 py-2.5 text-sm ${
                item.unsupported ? "opacity-50" : "cursor-pointer hover:bg-well"
              }`}
            >
              <input
                type="checkbox"
                disabled={!!item.unsupported}
                checked={picked.has(item.id)}
                onChange={() => toggle(item.id)}
                className="mt-0.5 accent-blue-500"
              />
              <div className="min-w-0 flex-1">
                <div className="flex flex-wrap items-baseline gap-x-2">
                  <span className="font-mono text-[13px] font-medium">{item.name}</span>
                  <span className="text-xs text-faint">{item.module}</span>
                  {item.generic && <span className="text-xs text-faint">generic</span>}
                  {item.variants.length > 0 && (
                    <span
                      className="text-xs text-faint"
                      title="A Move enum: its table gets a column per field of any variant"
                    >
                      enum {item.variants.join(", ")}
                    </span>
                  )}
                </div>
                <div className="truncate font-mono text-xs text-dim">
                  {item.unsupported ??
                    item.fields.map((f) => `${f.name}: ${shortType(f.type)}`).join(", ")}
                </div>
              </div>
              {!item.unsupported && (
                <span
                  className="hidden max-w-56 shrink-0 truncate font-mono text-xs text-faint sm:block"
                  title={`${item.suggested_name} (${becomes} table)`}
                >
                  → {item.suggested_name} <span className="text-ghost">{becomes}</span>
                </span>
              )}
            </label>
          </li>
        ))}
      </ul>
    </Card>
  );
}

function Choice({
  active,
  onClick,
  title,
  body,
}: {
  active: boolean;
  onClick: () => void;
  title: string;
  body: string;
}) {
  return (
    <button
      type="button"
      onClick={onClick}
      className={`rounded-lg border px-4 py-3 text-left transition-colors ${
        active
          ? "border-blue-400 bg-blue-500/10 ring-1 ring-blue-400/60"
          : "border-line hover:border-edge"
      }`}
    >
      <div className="text-sm font-medium">{title}</div>
      <div className="mt-0.5 text-xs text-dim">{body}</div>
    </button>
  );
}

function shortAddress(address: string): string {
  return address.length > 14 ? `${address.slice(0, 8)}…${address.slice(-4)}` : address;
}

/** `0xabc…::module::Type<…>` → `module::Type<…>`: the address is the contract's own. */
function shortType(type: string): string {
  return type.replace(/0x[0-9a-f]{64}::/g, "");
}

/** The module most of the catalog lives in, as an unused project name. */
function suggestName(catalog: Catalog, taken: string[]): string {
  const counts = new Map<string, number>();
  for (const item of catalog.items) {
    if (!item.unsupported) counts.set(item.module, (counts.get(item.module) ?? 0) + 1);
  }
  const top = [...counts.entries()].sort((a, b) => b[1] - a[1])[0]?.[0] ?? "app";
  const base =
    top
      .toLowerCase()
      .replace(/[^a-z0-9_]/g, "_")
      .replace(/^[^a-z]+/, "") || "app";
  let name = base;
  for (let n = 2; taken.includes(name); n++) name = `${base}_${n}`;
  return name;
}
