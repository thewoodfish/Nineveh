"use client";

import { Card } from "@/components/ui";
import type { CatalogItem } from "@/lib/api";

/**
 * The list of what a contract offers, with checkboxes.
 *
 * Shared, because it is the same list in two places that mean different things: on
 * `/new` it is what the project will be, and on `/sources` it is what the project
 * doesn't follow yet. The second one is the reason `followed` exists — an item already
 * being followed is the one piece of news this list can carry that the contract itself
 * doesn't know.
 */

/**
 * What each kind of pick becomes, in the language of the result rather than of the
 * chain: someone ticking a box is choosing a table, and the table is what they'll read.
 */
export const KINDS: {
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

/** `0xabc…::module::Type<…>` → `module::Type<…>`: the address is the contract's own. */
export function shortType(type: string): string {
  return type.replace(/0x[0-9a-f]{64}::/g, "");
}

/** The items whose module or name contains `search`. */
export function matching(items: CatalogItem[], search: string): CatalogItem[] {
  const needle = search.trim().toLowerCase();
  if (!needle) return items;
  return items.filter((i) =>
    `${i.module}::${i.name}`.toLowerCase().includes(needle),
  );
}

/** Items the project already follows, by catalog id, named by the source that has them. */
export type Followed = Map<string, string>;

export function CatalogGroups({
  items,
  picked,
  setPicked,
  followed,
  className = "",
}: {
  items: CatalogItem[];
  picked: Set<string>;
  setPicked: (next: Set<string>) => void;
  followed?: Followed;
  className?: string;
}) {
  return (
    <div className={`flex flex-col gap-4 ${className}`}>
      {KINDS.map(({ kind, title, becomes, hint }) => (
        <Group
          key={kind}
          title={title}
          hint={`${becomes} table: ${hint}`}
          items={items.filter((i) => i.kind === kind)}
          becomes={becomes}
          picked={picked}
          setPicked={setPicked}
          followed={followed}
        />
      ))}
    </div>
  );
}

function Group({
  title,
  hint,
  items,
  becomes,
  picked,
  setPicked,
  followed,
}: {
  title: string;
  hint: string;
  items: CatalogItem[];
  becomes: string;
  picked: Set<string>;
  setPicked: (next: Set<string>) => void;
  followed?: Followed;
}) {
  if (items.length === 0) return null;
  // Three states, not two: followable, followable but already followed, and not
  // followable at all. Only the first can be ticked, so only the first is counted and
  // only the first answers to All.
  const open = items.filter((i) => !i.unsupported && !followed?.has(i.id));
  const all = open.length > 0 && open.every((i) => picked.has(i.id));
  const toggleAll = () => {
    const next = new Set(picked);
    for (const i of open) {
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
  const already = items.filter((i) => followed?.has(i.id)).length;
  return (
    <Card className="overflow-hidden">
      <div className="flex items-center justify-between gap-3 border-b border-outline-variant bg-surface-container-high px-4 py-2.5">
        <div className="text-sm">
          <span className="font-medium">{title}</span>{" "}
          <span className="text-xs text-on-surface-variant">
            {open.filter((i) => picked.has(i.id)).length} of {items.length}
            {already > 0 && ` · ${already} already followed`} · {hint}
          </span>
        </div>
        {open.length > 0 && (
          <button
            type="button"
            onClick={toggleAll}
            className="state rounded-xs px-1 text-xs font-medium text-primary hover:underline"
          >
            {all ? "None" : "All"}
          </button>
        )}
      </div>
      <ul className="divide-y divide-outline-variant">
        {items.map((item) => {
          const by = followed?.get(item.id);
          const closed = !!item.unsupported || by !== undefined;
          return (
            <li key={item.id}>
              <label
                className={`flex items-start gap-3 px-4 py-2.5 text-sm ${
                  item.unsupported
                    ? "opacity-50"
                    : closed
                      ? ""
                      : "cursor-pointer hover:bg-surface-container-high"
                }`}
              >
                <input
                  type="checkbox"
                  disabled={closed}
                  checked={by === undefined && picked.has(item.id)}
                  onChange={() => toggle(item.id)}
                  className="mt-0.5 accent-primary"
                />
                <div className="min-w-0 flex-1">
                  <div className="flex flex-wrap items-baseline gap-x-2">
                    <span
                      className={`font-mono text-[13px] font-medium ${by ? "text-on-surface-variant" : ""}`}
                    >
                      {item.name}
                    </span>
                    <span className="text-xs text-on-surface-variant">
                      {item.module}
                    </span>
                    {item.generic && (
                      <span className="text-xs text-on-surface-variant">
                        generic
                      </span>
                    )}
                    {item.variants.length > 0 && (
                      <span
                        className="text-xs text-on-surface-variant"
                        title="A Move enum: its table gets a column per field of any variant"
                      >
                        enum {item.variants.join(", ")}
                      </span>
                    )}
                  </div>
                  <div className="truncate font-mono text-xs text-on-surface-variant">
                    {item.unsupported ??
                      item.fields
                        .map((f) => `${f.name}: ${shortType(f.type)}`)
                        .join(", ")}
                  </div>
                </div>
                {/* Already followed outranks the suggested name: the name it would get
                    is no longer the useful fact — the name it has is. */}
                {by !== undefined ? (
                  <span className="hidden max-w-56 shrink-0 truncate text-xs text-tertiary sm:block">
                    following as{" "}
                    <span className="font-mono text-on-surface-variant">
                      {by}
                    </span>
                  </span>
                ) : (
                  !item.unsupported && (
                    <span
                      className="hidden max-w-56 shrink-0 truncate font-mono text-xs text-on-surface-variant sm:block"
                      title={`${item.suggested_name} (${becomes} table)`}
                    >
                      → {item.suggested_name}{" "}
                      <span className="text-on-surface-variant/50">
                        {becomes}
                      </span>
                    </span>
                  )
                )}
              </label>
            </li>
          );
        })}
      </ul>
    </Card>
  );
}
