"use client";

import { useRouter } from "next/navigation";
import { useState } from "react";

import { Button, Card, Notice, PageHeader } from "@/components/ui";
import { ApiError, type Catalog, type CatalogItem, type Network, type Start, control } from "@/lib/api";
import { useProject } from "@/lib/project";

const KINDS: { kind: CatalogItem["kind"]; title: string; becomes: string; hint: string }[] = [
  { kind: "event", title: "Events", becomes: "log", hint: "a row per event, in order" },
  { kind: "resource", title: "Resources", becomes: "mirror", hint: "the latest value at each address" },
  { kind: "table", title: "Tables", becomes: "mirror", hint: "the latest value of each item" },
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
  const [name, setName] = useState("");
  const [start, setStart] = useState<Start>("auto");
  const [preview, setPreview] = useState<string | null>(null);
  const [creating, setCreating] = useState(false);
  const [error, setError] = useState<Failure | null>(null);

  if (mode === "single") {
    return (
      <div className="mx-auto mt-24 max-w-md px-6 text-center text-sm text-zinc-500">
        Creating projects needs the control plane. Run{" "}
        <span className="font-mono text-zinc-800 dark:text-zinc-200">nineveh up</span> instead of{" "}
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
      // Events to start with: they're the contract's activity, and an events-only project
      // streams only the transactions that emit them. Resources and tables are a click away.
      setPicked(new Set(found.items.filter((i) => i.kind === "event" && !i.unsupported).map((i) => i.id)));
      setName(suggestName(found, projects?.map((p) => p.name) ?? []));
    } catch (e) {
      setError(failure(e));
    } finally {
      setInspecting(false);
    }
  };

  const draft = { name, network, start, picks: catalog?.items.filter((i) => picked.has(i.id)).map((i) => i.id) ?? [] };

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

  const field =
    "rounded-md border border-zinc-200 bg-white px-3 py-2 text-sm focus:border-lapis-400 focus:outline-none dark:border-zinc-700 dark:bg-zinc-900";

  return (
    <div>
      <PageHeader title="New project" />
      <div className="mx-auto flex max-w-4xl flex-col gap-8 px-8 py-6">
        <Step n={1} title="Your contract" hint="Where your Move modules are published.">
          <form
            className="flex flex-wrap gap-2"
            onSubmit={(e) => {
              e.preventDefault();
              void inspect();
            }}
          >
            <select value={network} onChange={(e) => setNetwork(e.target.value as Network)} className={field}>
              <option value="testnet">testnet</option>
              <option value="mainnet">mainnet</option>
              <option value="devnet">devnet</option>
            </select>
            <input
              value={address}
              onChange={(e) => setAddress(e.target.value)}
              placeholder="0x… contract address"
              spellCheck={false}
              className={`${field} min-w-72 flex-1 font-mono`}
            />
            <Button type="submit" tone="primary" disabled={inspecting || !address.trim()}>
              {inspecting ? "Reading modules…" : "Inspect"}
            </Button>
          </form>
        </Step>

        {error && !creating && (
          <Notice tone="error" title={error.message}>
            {error.details && <pre className="mt-1 overflow-x-auto font-mono text-xs whitespace-pre">{error.details}</pre>}
          </Notice>
        )}

        {catalog && (
          <>
            <Step
              n={2}
              title="What to follow"
              hint={`${catalog.modules.length} module${catalog.modules.length === 1 ? "" : "s"} at ${shortAddress(catalog.address)}. Each pick becomes a live table.`}
            >
              <input
                value={search}
                onChange={(e) => setSearch(e.target.value)}
                placeholder="Filter by name or module"
                className={`${field} mb-3 w-full`}
              />
              <div className="flex flex-col gap-4">
                {KINDS.map(({ kind, title, becomes, hint }) => (
                  <Group
                    key={kind}
                    title={title}
                    hint={`${becomes} table: ${hint}`}
                    items={catalog.items.filter(
                      (i) =>
                        i.kind === kind &&
                        `${i.module}::${i.name}`.toLowerCase().includes(search.trim().toLowerCase()),
                    )}
                    becomes={becomes}
                    picked={picked}
                    setPicked={setPicked}
                  />
                ))}
              </div>
            </Step>

            <Step n={3} title="Name and history">
              <div className="flex flex-col gap-4">
                <label className="flex flex-col gap-1.5 text-xs font-medium text-zinc-500">
                  Project name
                  <input
                    value={name}
                    onChange={(e) => setName(e.target.value)}
                    spellCheck={false}
                    className={`${field} max-w-sm font-mono text-zinc-900 dark:text-zinc-100`}
                  />
                  <span className="font-normal text-zinc-400">
                    Lowercase letters, digits and <span className="font-mono">_</span>. It names your API and your
                    Postgres schema.
                  </span>
                </label>
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

            <div className="flex flex-wrap items-center gap-3 border-t border-zinc-200 pt-6 dark:border-zinc-800">
              <Button
                tone="primary"
                disabled={creating || draft.picks.length === 0 || !name}
                onClick={() => void create()}
                className="px-4 py-2"
              >
                {creating ? "Pinning layouts and starting…" : `Create backend with ${draft.picks.length} tables`}
              </Button>
              <Button disabled={creating || draft.picks.length === 0} onClick={() => void (preview ? setPreview(null) : showPreview())}>
                {preview ? "Hide config" : "Preview config"}
              </Button>
              {creating && error && <span className="text-sm text-red-600">{error.message}</span>}
            </div>
            {creating && error?.details && (
              <pre className="overflow-x-auto rounded-lg bg-red-50 p-3 font-mono text-xs text-red-900">{error.details}</pre>
            )}
            {preview && (
              <Card className="overflow-hidden">
                <div className="border-b border-zinc-200 px-4 py-2 text-xs text-zinc-500 dark:border-zinc-800">
                  <span className="font-mono">nineveh.yaml</span>: what Nineveh will run. You can edit it after
                  creating, from the project&apos;s Config.
                </div>
                <pre className="max-h-96 overflow-auto px-4 py-3 font-mono text-xs leading-relaxed">{preview}</pre>
              </Card>
            )}
          </>
        )}
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
      <div className="mb-3 flex items-baseline gap-3">
        <span className="flex size-6 shrink-0 items-center justify-center rounded-full bg-lapis-50 text-xs font-semibold text-lapis-600 dark:bg-lapis-500/15 dark:text-lapis-400">
          {n}
        </span>
        <div>
          <h2 className="text-sm font-semibold">{title}</h2>
          {hint && <p className="text-xs text-zinc-500">{hint}</p>}
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
      <div className="flex items-center justify-between gap-3 border-b border-zinc-200 bg-zinc-50/60 px-4 py-2 dark:border-zinc-800 dark:bg-zinc-900/60">
        <div className="text-sm">
          <span className="font-medium">{title}</span>{" "}
          <span className="text-xs text-zinc-400">
            {followable.filter((i) => picked.has(i.id)).length} of {items.length} · {hint}
          </span>
        </div>
        {followable.length > 0 && (
          <button type="button" onClick={toggleAll} className="text-xs font-medium text-lapis-600 hover:underline">
            {all ? "None" : "All"}
          </button>
        )}
      </div>
      <ul className="divide-y divide-zinc-100 dark:divide-zinc-800">
        {items.map((item) => (
          <li key={item.id}>
            <label
              className={`flex items-start gap-3 px-4 py-2.5 text-sm ${
                item.unsupported ? "opacity-50" : "cursor-pointer hover:bg-zinc-50 dark:hover:bg-zinc-800/40"
              }`}
            >
              <input
                type="checkbox"
                disabled={!!item.unsupported}
                checked={picked.has(item.id)}
                onChange={() => toggle(item.id)}
                className="mt-0.5 accent-lapis-600"
              />
              <div className="min-w-0 flex-1">
                <div className="flex flex-wrap items-baseline gap-x-2">
                  <span className="font-mono text-[13px] font-medium">{item.name}</span>
                  <span className="text-xs text-zinc-400">{item.module}</span>
                  {item.generic && <span className="text-xs text-zinc-400">generic</span>}
                  {item.variants.length > 0 && (
                    <span
                      className="text-xs text-zinc-400"
                      title="A Move enum: its table gets a column per field of any variant"
                    >
                      enum {item.variants.join(", ")}
                    </span>
                  )}
                </div>
                <div className="truncate font-mono text-xs text-zinc-500">
                  {item.unsupported ??
                    item.fields.map((f) => `${f.name}: ${shortType(f.type)}`).join(", ")}
                </div>
              </div>
              {!item.unsupported && (
                <span className="hidden shrink-0 font-mono text-xs text-zinc-400 sm:block">
                  → {item.suggested_name} <span className="text-zinc-300 dark:text-zinc-600">{becomes}</span>
                </span>
              )}
            </label>
          </li>
        ))}
      </ul>
    </Card>
  );
}

function Choice({ active, onClick, title, body }: { active: boolean; onClick: () => void; title: string; body: string }) {
  return (
    <button
      type="button"
      onClick={onClick}
      className={`rounded-lg border px-4 py-3 text-left transition-colors ${
        active
          ? "border-lapis-400 bg-lapis-50/60 ring-1 ring-lapis-400 dark:bg-lapis-500/10"
          : "border-zinc-200 hover:border-zinc-300 dark:border-zinc-800 dark:hover:border-zinc-700"
      }`}
    >
      <div className="text-sm font-medium">{title}</div>
      <div className="mt-0.5 text-xs text-zinc-500">{body}</div>
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
  const base = top.toLowerCase().replace(/[^a-z0-9_]/g, "_").replace(/^[^a-z]+/, "") || "app";
  let name = base;
  for (let n = 2; taken.includes(name); n++) name = `${base}_${n}`;
  return name;
}
