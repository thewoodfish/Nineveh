"use client";

import type { SourceInfo } from "@/lib/api";

/**
 * SCHEMA: a source's fields and their types.
 *
 * The thing you need in front of you to pick a key or write an expression, and the thing
 * Studio never showed — the data was here all along, spent only on the completion popup,
 * which you have to already know a field exists to open.
 *
 * Labelled, on its own background, and with its columns named, because a bare list of
 * word-word pairs makes the reader work out which half is the type. The Move type it
 * follows is in the band because two sources can offer the same field names.
 *
 * Given `onInsert`, each field is a button that types itself into whichever expression box
 * has the focus — so the reference and the palette are one list, rather than two that have
 * to agree.
 */
export function SourceSchema({
  source,
  deleted = false,
  onInsert,
}: {
  source: SourceInfo;
  /** Show what a `<name>.deleted` rule can read instead: only the row's identity. */
  deleted?: boolean;
  onInsert?: (name: string) => void;
}) {
  const fields = deleted ? source.delete_fields : source.fields;
  const nullable = fields.some((f) => f.nullable);
  return (
    <section className="overflow-hidden rounded-md border border-outline-variant">
      <div className="flex flex-wrap items-center gap-x-3 gap-y-1 bg-secondary-container px-3 py-2 text-on-secondary-container">
        <h3 className="text-[11px] font-semibold tracking-[0.08em] uppercase">Schema</h3>
        <span className="font-mono text-xs font-medium">{source.name}</span>
        <span className="text-[11px]">{source.kind}</span>
        <span className="min-w-0 flex-1 truncate font-mono text-[11px] text-on-secondary-container/75">
          {source.follows}
        </span>
        <span className="shrink-0 text-[11px] text-on-secondary-container/75">
          {deleted
            ? "on delete"
            : `${source.matched.toLocaleString()} record${source.matched === 1 ? "" : "s"} so far`}
        </span>
      </div>

      <div className="bg-surface-container">
        <p className="px-3 pt-2 text-[11px] leading-relaxed text-on-surface-variant">
          {deleted
            ? "All a deleted record still says — enough to find the row, and nothing else."
            : "Every field a record from this source carries."}
          {onInsert && fields.length > 0 && " Click one to put it in the expression you're editing."}
          {nullable && " A ? means it can be null."}
        </p>

        {fields.length === 0 ? (
          <p className="px-3 pt-1 pb-2.5 text-xs text-on-surface-variant">
            Nothing readable on this one.
          </p>
        ) : (
          /* Two named columns rather than a wide grid of pairs: one reading order, and
             the header says which half is the type instead of leaving it to be inferred.
             Capped and scrolled, because a resource can carry thirty fields. */
          <div className="max-h-56 overflow-y-auto px-3 pt-1.5 pb-2.5">
            <table className="w-auto text-left">
              <thead className="sticky top-0 bg-surface-container">
                <tr className="text-[10px] font-medium tracking-[0.08em] text-on-surface-variant uppercase">
                  <th className="pb-1 pr-16 font-medium">Field</th>
                  <th className="pb-1 font-medium">Type</th>
                </tr>
              </thead>
              <tbody className="font-mono text-xs">
                {fields.map((f) => (
                  <tr key={f.name} className="border-t border-outline-variant/60">
                    <td className="py-1 pr-16 align-baseline">
                      {onInsert ? (
                        <button
                          type="button"
                          // Keep the focused expression box focused, so it knows where to type.
                          onMouseDown={(e) => e.preventDefault()}
                          onClick={() => onInsert(f.name)}
                          title={`Put ${f.name} in the expression you're editing`}
                          className="-mx-1 rounded px-1 text-left whitespace-nowrap text-on-surface hover:bg-primary-container"
                        >
                          {f.name}
                        </button>
                      ) : (
                        <span className="text-on-surface">{f.name}</span>
                      )}
                    </td>
                    <td className="py-1 align-baseline text-on-surface-variant">
                      {f.type}
                      {f.nullable ? "?" : ""}
                    </td>
                  </tr>
                ))}
              </tbody>
            </table>
          </div>
        )}
      </div>
    </section>
  );
}
