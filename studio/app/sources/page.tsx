"use client";

import Link from "next/link";
import { useEffect, useState } from "react";

import { CatalogGroups, matching } from "@/components/catalog-picker";
import { PageHeader } from "@/components/page-header";
import { SourceSchema } from "@/components/source-schema";
import {
  Button,
  Card,
  Empty,
  Ghost,
  Icon,
  Notice,
  Offline,
  Reading,
  field,
} from "@/components/ui";
import {
  ApiError,
  DOCS_URL,
  type Catalog,
  type Network,
  type SourceInfo,
  control,
} from "@/lib/api";
import { formatInteger } from "@/lib/format";
import { useSources, useStatus } from "@/lib/hooks";
import { useHref, useProject } from "@/lib/project";

/**
 * What this project reads off the chain, and how to read more of it.
 *
 * A page of its own because a source is not a table and was being shown as if it were
 * one: the project's inputs appeared as a schema panel beside the reducer editor, as a
 * warning on Overview when one was quiet, and nowhere as themselves. And the flow that
 * adds one had nowhere to live at all — `/new` has always ended with "you can add
 * sources later", and until now the only way to was to edit `nineveh.yaml` by hand
 * (which Studio shows read-only).
 */
export default function SourcesPage() {
  const { mode, current, error } = useProject();
  // Every hook above this line, and nothing below it: React counts hook calls per
  // render, so a hook under a conditional return registers on one render and not the
  // next, which throws "Rendered fewer hooks than expected" and white-screens the page.
  if (mode === "loading") return null;
  if (mode === "offline") {
    return <Offline error={error ?? "Nineveh isn't answering"} />;
  }
  if (mode === "single") {
    return (
      <div>
        <PageHeader title="Sources" />
        <p className="max-w-prose px-6 py-6 text-sm text-on-surface-variant">
          A project&apos;s sources come from the control plane, which is also
          what holds its config — so served on its own there is nothing here to
          list and nothing to add. Run{" "}
          <span className="font-mono text-on-surface">nineveh up</span> instead
          of <span className="font-mono">nineveh run --serve</span> and Studio
          can do both; or write the source into{" "}
          <span className="font-mono">nineveh.yaml</span> yourself and run again.
        </p>
      </div>
    );
  }
  if (!current) {
    return (
      <div>
        <PageHeader title="Sources" />
        <p className="px-6 py-6 text-sm text-on-surface-variant">
          Open a project first: sources belong to one.
        </p>
      </div>
    );
  }
  return <Sources project={current.name} network={current.network} />;
}

function Sources({ project, network }: { project: string; network: Network }) {
  const { data: sources, error, pending, reload } = useSources();
  const { refresh } = useProject();
  const href = useHref();
  const [adding, setAdding] = useState(false);
  const [added, setAdded] = useState<number | null>(null);

  const followed = new Map<string, string>();
  for (const source of sources ?? []) followed.set(catalogId(source), source.name);

  return (
    <div>
      <PageHeader
        title="Sources"
        hint={
          sources
            ? `${sources.length} source${sources.length === 1 ? "" : "s"} on ${network}`
            : undefined
        }
      >
        <Button
          tone={adding ? "secondary" : "primary"}
          onClick={() => {
            setAdding(!adding);
            setAdded(null);
          }}
        >
          <Icon name={adding ? "close" : "add"} className="text-[18px]" />
          {adding ? "Cancel" : "Follow something new"}
        </Button>
      </PageHeader>

      <div className="flex max-w-4xl flex-col gap-6 px-6 py-6">
        {added !== null && (
          <Notice
            title={`Now following ${added} more source${added === 1 ? "" : "s"}`}
            onClose={() => setAdded(null)}
          >
            The config is saved and the project is rebuilding — the new sources
            are in the list below.{" "}
            <Link href={href("/")} className="underline underline-offset-2">
              Overview
            </Link>{" "}
            shows how far it has got.
          </Notice>
        )}

        {adding && (
          <AddSource
            project={project}
            network={network}
            sources={sources}
            followed={followed}
            onAdded={(count) => {
              setAdded(count);
              setAdding(false);
              reload();
              void refresh();
            }}
          />
        )}

        {error && !sources && (
          <Notice tone="error" title="Can't read this project's sources">
            {error}
          </Notice>
        )}

        {pending && !sources ? (
          <Reading
            label="Reading this project's sources"
            className="flex flex-col gap-2"
          >
            {[0, 1, 2].map((n) => (
              <Card key={n} className="px-4 py-3.5">
                <Ghost className="h-4 w-40" />
                <Ghost className="mt-2 h-3 w-72" />
              </Card>
            ))}
          </Reading>
        ) : sources && sources.length === 0 ? (
          <Empty
            icon="input"
            title="This project follows nothing"
            why="A source is one thing Nineveh reads off the chain — an event, a resource, or the items of a table. Without one there is nothing for a table to be built from."
            action={
              <Button tone="primary" onClick={() => setAdding(true)}>
                Follow something
              </Button>
            }
          />
        ) : (
          <ul className="flex flex-col gap-2">
            {sources?.map((source) => (
              <li key={source.name}>
                <Source source={source} />
              </li>
            ))}
          </ul>
        )}

        <p className="max-w-prose text-xs leading-relaxed text-on-surface-variant">
          Sources are the project&apos;s inputs. Reducers fold them into the state
          tables your app reads — one{" "}
          <span className="font-mono">log</span> row per event, or a{" "}
          <span className="font-mono">mirror</span> of the latest value, and
          whatever else you write.{" "}
          <a
            href={`${DOCS_URL}/config`}
            target="_blank"
            rel="noreferrer"
            className="text-primary underline underline-offset-2"
          >
            How a source is written
          </a>
          .
        </p>
      </div>
    </div>
  );
}

/** One source: what it follows and how much it has seen, with its fields a click away. */
function Source({ source }: { source: SourceInfo }) {
  const [open, setOpen] = useState(false);
  return (
    <Card className="overflow-hidden">
      <button
        type="button"
        onClick={() => setOpen(!open)}
        aria-expanded={open}
        className="state flex w-full items-center gap-3 px-4 py-3.5 text-left"
      >
        <Icon
          name="expand_more"
          className={`shrink-0 text-[20px] text-on-surface-variant transition-transform ${open ? "" : "-rotate-90"}`}
        />
        <span className="min-w-0 flex-1">
          <span className="flex flex-wrap items-baseline gap-x-2">
            <span className="font-mono text-sm font-medium text-on-surface">
              {source.name}
            </span>
            <span className="rounded-xs bg-surface-container-high px-1.5 py-0.5 text-[11px] text-on-surface-variant">
              {source.kind}
            </span>
          </span>
          <span className="mt-0.5 block truncate font-mono text-xs text-on-surface-variant">
            {source.follows}
          </span>
        </span>
        {/* Zero is worth saying in words. "0 records" reads as a count that happens to
            be zero; "nothing yet" reads as the question it actually is — a quiet
            contract, or the wrong type followed. */}
        <span
          className={`shrink-0 text-xs tnum ${source.matched === 0 ? "text-on-surface-variant/70" : "text-on-surface-variant"}`}
        >
          {source.matched === 0
            ? "nothing yet"
            : `${formatInteger(source.matched)} record${source.matched === 1 ? "" : "s"}`}
        </span>
      </button>
      {open && (
        <div className="px-4 pb-4">
          <SourceSchema source={source} />
        </div>
      )}
    </Card>
  );
}

/**
 * Inspect a contract, tick what to follow, save it.
 *
 * The network isn't asked: a project follows one chain. The address is, because a
 * project may well want a second contract — and the ones it already follows are one
 * click, since wanting more of the same contract is the ordinary case.
 */
function AddSource({
  project,
  network,
  sources,
  followed,
  onAdded,
}: {
  project: string;
  network: Network;
  /** The project's sources, so the panel and the list can't disagree about them. */
  sources: SourceInfo[] | null;
  followed: Map<string, string>;
  onAdded: (count: number) => void;
}) {
  const { data: status } = useStatus();
  const [address, setAddress] = useState("");
  const [catalog, setCatalog] = useState<Catalog | null>(null);
  const [inspecting, setInspecting] = useState(false);
  // Nothing ticked, unlike `/new`, which ticks everything. There the question is what
  // the project is, and the generous default is right. Here it is what to add to a
  // project that is already running, and every tick costs a rebuild — so the whole
  // contract is not one click away from being followed by accident.
  const [picked, setPicked] = useState<Set<string>>(new Set());
  const [search, setSearch] = useState("");
  const [saving, setSaving] = useState(false);
  const [error, setError] = useState<{
    message: string;
    details?: string;
  } | null>(null);

  const known = [
    ...new Set((sources ?? []).map((s) => s.follows.split("::")[0] ?? "")),
  ].filter(Boolean);

  // The contract this project is already about.
  useEffect(() => {
    setAddress((current) => current || (known[0] ?? ""));
    // eslint-disable-next-line react-hooks/exhaustive-deps
  }, [known.join(",")]);

  // And read it as soon as there is one. With nothing to type, a button saying Inspect
  // was a click between the reader and the list they opened this panel for — opening it
  // was the intent. It still reads the chain and still changes nothing; the button
  // stays for reading again after a contract is upgraded, and for the other contract
  // when a project follows two.
  const [read, setRead] = useState("");
  useEffect(() => {
    if (!address || read === address) return;
    setRead(address);
    void inspect();
    // eslint-disable-next-line react-hooks/exhaustive-deps
  }, [address, read]);

  const inspect = async () => {
    setInspecting(true);
    setError(null);
    setCatalog(null);
    // Ticks belong to the contract they were made on. Another address has other ids,
    // so keeping them would leave a set that quietly matches nothing and a button
    // offering to follow none of it.
    setPicked(new Set());
    setSearch("");
    try {
      const found = await control.inspect(network, address.trim());
      setCatalog(found);
    } catch (e) {
      setError(asFailure(e));
    } finally {
      setInspecting(false);
    }
  };

  const picks = catalog?.items.filter((i) => picked.has(i.id)).map((i) => i.id) ?? [];

  const save = async () => {
    setSaving(true);
    setError(null);
    try {
      await control.addSources(project, picks);
      onAdded(picks.length);
    } catch (e) {
      setError(asFailure(e));
      setSaving(false);
    }
  };

  return (
    <Card className="flex flex-col gap-4 px-5 py-5">
      <div>
        <h2 className="text-[15px] font-medium text-on-surface">
          Follow more of a contract
        </h2>
        <p className="mt-1 max-w-prose text-sm text-on-surface-variant">
          Nineveh reads the modules this project already follows and shows what
          else is in them. Nothing changes until you choose.
        </p>
      </div>

      {/* The address is shown, not typed. Pasting a different contract in here would
          quietly make this project about two things, and the rest of Studio is built
          one project to one contract — `/new` asks for a single address too. It is
          also a trap the server cannot save you from: the project's backfill starts at
          the version its own contract was published at, so a contract published
          earlier would come in missing the history before that, with nothing to say
          so. A second contract is a second project. */}
      <div className="flex flex-col gap-3 sm:flex-row">
        <div className="flex min-w-0 flex-1 items-center rounded-xs border border-outline-variant bg-surface-container px-3 py-2.5 font-mono text-sm break-all text-on-surface">
          {address || "—"}
        </div>
        <Button
          tone="tonal"
          disabled={inspecting || !address}
          onClick={() => void inspect()}
        >
          {inspecting ? "Reading modules…" : catalog ? "Read again" : "Inspect"}
        </Button>
      </div>

      {known.length > 1 && (
        <div className="flex flex-wrap items-center gap-2 text-xs text-on-surface-variant">
          This project follows {known.length} contracts:
          {known.map((a) => (
            <button
              key={a}
              type="button"
              onClick={() => {
                setAddress(a);
                setCatalog(null);
                setPicked(new Set());
              }}
              aria-pressed={a === address}
              title={`Read the modules at ${a}`}
              className={`state rounded-full border px-2.5 py-1 font-mono text-[11px] ${
                a === address
                  ? "border-primary bg-secondary-container text-on-secondary-container"
                  : "border-outline-variant text-on-surface"
              }`}
            >
              {shortAddress(a)}
            </button>
          ))}
        </div>
      )}

      {error && (
        <Notice tone="error" title={error.message}>
          {error.details && (
            <pre className="mt-1 overflow-x-auto font-mono text-xs whitespace-pre">
              {error.details}
            </pre>
          )}
        </Notice>
      )}

      {catalog && (
        <div className="reveal flex flex-col gap-4">
          <div className="flex flex-wrap items-baseline justify-between gap-2">
            <p className="text-sm text-on-surface-variant">
              {catalog.modules.length} module
              {catalog.modules.length === 1 ? "" : "s"} at{" "}
              <span className="font-mono text-xs">
                {shortAddress(catalog.address)}
              </span>
              .
            </p>
            <input
              value={search}
              onChange={(e) => setSearch(e.target.value)}
              placeholder="Filter by name or module"
              aria-label="Filter the catalog"
              className={`${field} w-56`}
            />
          </div>

          <CatalogGroups
            items={matching(catalog.items, search)}
            picked={picked}
            setPicked={setPicked}
            followed={followed}
          />

          <Rebuild status={status} />

          <div className="flex flex-wrap items-center gap-3">
            <Button
              tone="primary"
              disabled={saving || picks.length === 0}
              onClick={() => void save()}
            >
              {saving
                ? "Pinning layouts and rebuilding…"
                : `Follow ${picks.length} more source${picks.length === 1 ? "" : "s"}`}
            </Button>
            {picks.length === 0 && (
              <span className="text-xs text-on-surface-variant">
                Tick what you want followed.
              </span>
            )}
          </div>
        </div>
      )}
    </Card>
  );
}

/**
 * What saving will cost, said before it is spent.
 *
 * Adding a source is the expensive config change, and it is the one place the record
 * log can't help: there is no history for something nothing was following, so the
 * rebuild re-reads the chain rather than replaying locally (ADR 0022,
 * `Unavailable::SourceAdded`). Changing a rule is the cheap one. Someone about to
 * press the button is owed the difference.
 */
function Rebuild({ status }: { status: ReturnType<typeof useStatus>["data"] }) {
  const start = status?.pipeline?.start_version;
  const head = status?.pipeline?.chain_head ?? status?.pipeline?.chain_version;
  let versions: string | null = null;
  try {
    if (start && head) versions = (BigInt(head) - BigInt(start)).toString();
  } catch {
    versions = null;
  }
  return (
    <Notice tone="warning" title="This rebuilds the project from the chain">
      <p>
        A new source has no history here — nothing was following it — so Nineveh
        re-reads{" "}
        {versions ? (
          <>
            the <span className="tnum">{formatInteger(versions)}</span> versions
            since this project&apos;s start
          </>
        ) : (
          "the project's whole range"
        )}{" "}
        to build the new tables. Editing a rule replays what Nineveh already
        kept; adding a source can&apos;t.
      </p>
      <p className="mt-2">
        Your tables keep serving the rows they have while it runs, and swap over
        when it finishes — but they stop advancing until then, and the change
        feed starts again from the new build, so anything reading it has to
        resync.
      </p>
    </Notice>
  );
}

/**
 * A source's Move type as a catalog id: no type arguments, because a pick has none.
 *
 * The same reduction the plane makes when it refuses a duplicate, so the list marks
 * what the server would turn down. The server is the one that decides; this only
 * saves the trip.
 */
function catalogId(source: SourceInfo): string {
  return source.follows.replace(/<.*>/, "");
}

function shortAddress(address: string): string {
  return address.length > 14
    ? `${address.slice(0, 8)}…${address.slice(-4)}`
    : address;
}

function asFailure(e: unknown): { message: string; details?: string } {
  if (e instanceof ApiError) return { message: e.message, details: e.details };
  return { message: e instanceof Error ? e.message : String(e) };
}
