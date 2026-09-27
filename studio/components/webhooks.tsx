"use client";

import { useCallback, useEffect, useMemo, useState } from "react";

import { ApiError, type Table, type WebhookInfo, control } from "@/lib/api";
import { formatDuration } from "@/lib/format";
import { useTables } from "@/lib/hooks";
import {
  type Draft,
  OPS,
  OP_IN_PAYLOAD,
  type Op,
  blank,
  problems,
  subscriptions,
  toDraft,
  toggled,
  withWebhooks,
} from "@/lib/webhook-config";

import { ConfirmDialog, Dialog } from "./dialog";
import { Button, Card, Field, Icon, IconButton, Notice, Segmented, field } from "./ui";

/**
 * Where a project's state changes are pushed, and how that's going.
 *
 * The other direction from the API console: there, an app asks Nineveh for rows; here,
 * Nineveh calls the app when they change. Configuring and checking an endpoint is one
 * loop — set a URL, see what happened, fix it — so both live on this page rather than
 * one being somewhere else.
 *
 * Endpoints live in `nineveh.yaml`, so adding one is an edit to the config. That's cheap:
 * `webhooks` isn't part of a build's fingerprint, so no change here rebuilds a table.
 */
export function Webhooks({ project }: { project: string }) {
  const { tables } = useTables();
  const [hooks, setHooks] = useState<WebhookInfo[] | null>(null);
  const [shown, setShown] = useState<string | null>(null);
  const [copied, setCopied] = useState<string | null>(null);
  const [rotating, setRotating] = useState<string | null>(null);
  const [removing, setRemoving] = useState<string | null>(null);
  const [editing, setEditing] = useState<{ draft: Draft; existing: boolean } | null>(null);
  const [saving, setSaving] = useState(false);
  const [error, setError] = useState<{ title: string; body: string } | null>(null);

  const load = useCallback(() => {
    control
      .webhooks(project)
      .then(setHooks)
      .catch((e: unknown) =>
        setError({
          title: "Couldn't read this project's endpoints",
          body: e instanceof Error ? e.message : String(e),
        }),
      );
  }, [project]);
  useEffect(load, [load]);

  // Deliveries move on their own, so keep the health honest while this is open.
  useEffect(() => {
    const timer = setInterval(load, 5000);
    return () => clearInterval(timer);
  }, [load]);

  const copy = async (name: string, text: string) => {
    await navigator.clipboard.writeText(text);
    setCopied(name);
    setTimeout(() => setCopied((c) => (c === name ? null : c)), 1500);
  };

  const rotate = async (name: string) => {
    setRotating(null);
    try {
      await control.rotateWebhook(project, name);
      setShown(name);
      load();
    } catch (e) {
      setError({
        title: "The secret wasn't rotated",
        body: e instanceof Error ? e.message : String(e),
      });
    }
  };

  /**
   * Write the endpoints the config should hold after `change` is applied to the ones it
   * holds now. The config is read here rather than kept in state: someone may have
   * edited it in the config panel since this page loaded, and their sources are not
   * this page's to overwrite.
   */
  const write = async (change: (drafts: Draft[]) => Draft[]) => {
    setSaving(true);
    setError(null);
    try {
      const detail = await control.project(project);
      const wanted = change((hooks ?? []).map(toDraft));
      await control.update(project, withWebhooks(detail.config, wanted), detail.reducers);
      setEditing(null);
      setRemoving(null);
      load();
    } catch (e) {
      setError({
        title: "The config wouldn't take that",
        body: e instanceof ApiError ? (e.details ?? e.message) : String(e),
      });
    } finally {
      setSaving(false);
    }
  };

  const save = (draft: Draft) =>
    write((drafts) =>
      drafts.some((d) => d.name === draft.name)
        ? drafts.map((d) => (d.name === draft.name ? draft : d))
        : [...drafts, draft],
    );

  return (
    <div className="flex flex-col gap-8">
      <section>
        <div className="flex flex-wrap items-center justify-between gap-3">
          <div>
            <h2 className="text-base font-medium text-on-surface">Endpoints</h2>
            <p className="mt-1 max-w-2xl text-sm text-on-surface-variant">
              Every change to the tables an endpoint asks for, signed and POSTed to it in
              order, retried until it answers.
            </p>
          </div>
          {hooks && hooks.length > 0 && (
            <Button tone="primary" onClick={() => setEditing({ draft: blank(), existing: false })}>
              <Icon name="add" className="text-[18px]" />
              New endpoint
            </Button>
          )}
        </div>

        <div className="mt-4">
          {error && (
            <div className="mb-4">
              <Notice tone="error" title={error.title}>
                <pre className="overflow-x-auto font-mono text-xs whitespace-pre-wrap">
                  {error.body}
                </pre>
              </Notice>
            </div>
          )}

          {hooks === null ? null : hooks.length === 0 ? (
            <Card className="flex flex-col items-center px-6 py-10 text-center">
              <Icon name="webhook" className="text-[32px] text-on-surface-variant" />
              <p className="mt-3 text-sm text-on-surface">Nothing is listening yet</p>
              <p className="mt-1 max-w-sm text-sm text-on-surface-variant">
                Point Nineveh at a URL and it calls you when a table changes, instead of your
                server asking whether anything has.
              </p>
              <Button
                tone="primary"
                className="mt-5"
                onClick={() => setEditing({ draft: blank(), existing: false })}
              >
                Add an endpoint
              </Button>
            </Card>
          ) : (
            <Card>
              <ul className="divide-y divide-outline-variant">
                {hooks?.map((hook) => (
                  <li key={hook.name} className="px-4 py-3">
                    <div className="flex flex-wrap items-baseline gap-x-3 gap-y-1">
                      <span className="text-sm font-medium">{hook.name}</span>
                      <code className="min-w-0 flex-1 truncate font-mono text-xs text-on-surface-variant">
                        {hook.url}
                      </code>
                      <Health hook={hook} />
                      <IconButton
                        name="edit"
                        title={`Edit ${hook.name}`}
                        aria-label={`Edit ${hook.name}`}
                        className="-my-2"
                        onClick={() => setEditing({ draft: toDraft(hook), existing: true })}
                      />
                      <IconButton
                        name="delete"
                        title={`Remove ${hook.name}`}
                        aria-label={`Remove ${hook.name}`}
                        className="-my-2"
                        onClick={() => setRemoving(hook.name)}
                      />
                    </div>

                    <div className="mt-1.5 flex flex-wrap items-center gap-1.5 text-xs text-on-surface-variant">
                      {hook.on.map((on) => (
                        <span
                          key={on}
                          className="rounded bg-surface-container-high px-1.5 py-0.5 font-mono"
                        >
                          {on}
                        </span>
                      ))}
                      <span>{hook.rows ? "with the row" : "keys only"}</span>
                    </div>

                    {hook.last_error && (
                      <p className="mt-1.5 truncate text-xs text-error" title={hook.last_error}>
                        last attempt: {hook.last_error}
                      </p>
                    )}

                    <div className="mt-2 flex items-center gap-2">
                      <code className="min-w-0 flex-1 truncate rounded bg-surface-container-high px-2 py-1 font-mono text-xs">
                        {shown === hook.name
                          ? hook.secret
                          : `${hook.secret.slice(0, 10)}${"•".repeat(12)}`}
                      </code>
                      <Button
                        size="sm"
                        onClick={() => setShown(shown === hook.name ? null : hook.name)}
                      >
                        {shown === hook.name ? "Hide" : "Reveal"}
                      </Button>
                      <Button size="sm" onClick={() => void copy(hook.name, hook.secret)}>
                        {copied === hook.name ? "Copied" : "Copy"}
                      </Button>
                      <Button tone="danger" size="sm" onClick={() => setRotating(hook.name)}>
                        Rotate
                      </Button>
                    </div>
                  </li>
                ))}
              </ul>
            </Card>
          )}
        </div>
      </section>

      <DeliveryShape />

      {editing && (
        <Editor
          draft={editing.draft}
          existing={editing.existing}
          taken={(hooks ?? []).map((h) => h.name).filter((n) => n !== editing.draft.name)}
          tables={tables}
          subscribed={(hooks ?? []).flatMap((h) =>
            h.on.map((on) => on.slice(0, on.lastIndexOf("."))),
          )}
          busy={saving}
          onClose={() => setEditing(null)}
          onSave={(draft) => void save(draft)}
        />
      )}

      <ConfirmDialog
        danger
        open={rotating !== null}
        onClose={() => setRotating(null)}
        onConfirm={() => rotating !== null && void rotate(rotating)}
        title="Rotate this signing secret?"
        confirmLabel="Rotate secret"
      >
        Deliveries to <span className="font-mono text-on-surface">{rotating}</span> are signed with
        the new secret from the next one on. Anything still checking signatures against the old one
        will reject them until you update it.
      </ConfirmDialog>

      <ConfirmDialog
        danger
        open={removing !== null}
        busy={saving}
        onClose={() => setRemoving(null)}
        onConfirm={() => void write((drafts) => drafts.filter((d) => d.name !== removing))}
        title={`Remove ${removing}?`}
        confirmLabel="Remove endpoint"
      >
        Nothing more is sent to it, and its signing secret and delivery position go with it. Adding
        it back later starts from the changes happening then, not the ones it missed.
      </ConfirmDialog>
    </div>
  );
}

/**
 * Whether deliveries are landing, in a few words.
 *
 * Read from `last_delivered`, not from the cursor: a new endpoint is placed at the end
 * of the feed so that configuring one doesn't replay the project's whole history, which
 * gives it a position before anything has ever been sent to it.
 */
function Health({ hook }: { hook: WebhookInfo }) {
  if (hook.failures > 0) {
    return (
      <span className="text-xs text-error">
        failing · {hook.failures} {hook.failures === 1 ? "attempt" : "attempts"}
      </span>
    );
  }
  if (!hook.last_delivered)
    return <span className="text-xs text-on-surface-variant">nothing sent yet</span>;
  return (
    <span className="text-xs text-on-tertiary-container" title={hook.last_delivered}>
      delivered {since(hook.last_delivered)}
    </span>
  );
}

/** How long ago `at` was, in the same words as everything else that measures time here. */
function since(at: string): string {
  const seconds = Math.round((Date.now() - Date.parse(at)) / 1000);
  return Number.isNaN(seconds) ? "just now" : `${formatDuration(Math.max(seconds, 0))} ago`;
}

/**
 * What arrives at the URL, and how a receiver knows it was Nineveh.
 *
 * On the page rather than only in the docs because this is what someone needs *before*
 * they have a URL worth pointing at: the handler is written against this shape, and the
 * signature check is the part that is easy to get subtly wrong.
 */
function DeliveryShape() {
  return (
    <section>
      <h2 className="text-base font-medium text-on-surface">What arrives</h2>
      <p className="mt-1 max-w-2xl text-sm text-on-surface-variant">
        One POST per batch, up to 100 changes, in the order they happened. Answer 2xx and Nineveh
        moves on; answer anything else, or nothing, and it retries the same batch — so a receiver
        has to tolerate seeing a change twice.
      </p>
      <Card className="mt-4 divide-y divide-outline-variant">
        <div className="px-4 py-3">
          <div className="text-xs font-medium text-on-surface-variant">Body</div>
          <pre className="mt-2 overflow-x-auto font-mono text-xs leading-relaxed text-on-surface">
            {`{
  "project": "market",
  "endpoint": "my_backend",
  "changes": [
    {
      "table": "balances",
      "op": "update",
      "version": "1287340021",
      "seq": 3,
      "key": { "user": "0x1" },
      "row": { "user": "0x1", "balance": "4200" }
    }
  ]
}`}
          </pre>
          <p className="mt-2 text-xs text-on-surface-variant">
            <span className="font-mono">version</span> and{" "}
            <span className="font-mono">seq</span> order every change a project ever makes, so
            they&apos;re also how a receiver spots one it has already handled.{" "}
            <span className="font-mono">row</span> is absent when the endpoint asks for keys only,
            and on a delete, where there is no row left to send.
          </p>
        </div>
        <div className="px-4 py-3">
          <div className="text-xs font-medium text-on-surface-variant">Signature</div>
          <pre className="mt-2 overflow-x-auto font-mono text-xs leading-relaxed text-on-surface">
            {`X-Nineveh-Signature: t=1735689600, v1=<hex>

v1 = HMAC-SHA256(secret, "<t>." + <the raw body>)`}
          </pre>
          <p className="mt-2 text-xs text-on-surface-variant">
            Compute it over the bytes you received, before any JSON parsing, and compare in constant
            time. Reject a <span className="font-mono">t</span> that is far from your own clock, so
            a delivery someone captured can&apos;t be replayed at you later.
          </p>
        </div>
      </Card>
    </section>
  );
}

/**
 * One endpoint: where it points, what it hears, and how much each delivery carries.
 *
 * The changes are a grid of tables against the three row changes rather than the
 * config's list of `table.change` strings. The config can only be read one line at a
 * time; the grid answers "who hears about `balances`?" in a glance, which is the
 * question someone actually arrives with.
 */
function Editor({
  draft: initial,
  existing,
  taken,
  tables,
  subscribed,
  busy,
  onClose,
  onSave,
}: {
  draft: Draft;
  existing: boolean;
  taken: string[];
  /** The project's tables, or `null` while they're still being read. */
  tables: Table[] | null;
  /** Tables an endpoint already names, in case a stopped project can't list them. */
  subscribed: string[];
  busy: boolean;
  onClose: () => void;
  onSave: (draft: Draft) => void;
}) {
  const [draft, setDraft] = useState(initial);
  // Nothing is wrong with a form nobody has filled in yet, so the reasons appear when
  // someone tries rather than the moment the dialog opens.
  const [tried, setTried] = useState(false);
  const names = useMemo(() => {
    const listed = (tables ?? []).map((t) => t.name);
    return [...listed, ...subscribed.filter((t) => !listed.includes(t))];
  }, [tables, subscribed]);
  const wrong = problems(draft, taken);

  return (
    <Dialog
      open
      onClose={onClose}
      title={existing ? draft.name : "New endpoint"}
      actions={
        <>
          <Button tone="text" disabled={busy} onClick={onClose}>
            Cancel
          </Button>
          <Button
            tone="primary"
            disabled={busy}
            onClick={() => {
              setTried(true);
              if (wrong.length === 0) {
                onSave({ ...draft, name: draft.name.trim(), url: draft.url.trim() });
              }
            }}
          >
            {busy ? "Saving…" : existing ? "Save changes" : "Add endpoint"}
          </Button>
        </>
      }
    >
      <div className="flex flex-col gap-5 text-left">
        {!existing && (
          <Field
            label="Name"
            hint="What its signing secret and its place in the change order belong to."
          >
            <input
              className={field}
              value={draft.name}
              autoFocus
              spellCheck={false}
              placeholder="my_backend"
              onChange={(e) => setDraft({ ...draft, name: e.target.value })}
            />
          </Field>
        )}

        <Field label="URL" hint="https, or http to your own machine while you're building.">
          <input
            className={`${field} font-mono text-xs`}
            value={draft.url}
            autoFocus={existing}
            spellCheck={false}
            placeholder="https://myapp.example/hooks/nineveh"
            onChange={(e) => setDraft({ ...draft, url: e.target.value })}
          />
        </Field>

        <div>
          <div className="text-xs font-medium text-on-surface-variant">Changes it hears</div>
          {names.length === 0 ? (
            <p className="mt-2 text-sm text-on-surface-variant">
              {tables === null
                ? "Reading this project's tables…"
                : "This project has no state tables to send yet."}
            </p>
          ) : (
            <table className="mt-2 w-full text-sm">
              <thead>
                <tr>
                  <th />
                  {OPS.map((op) => (
                    <th
                      key={op}
                      className="w-16 pb-1 text-center text-[11px] font-medium text-on-surface-variant"
                    >
                      {OP_IN_PAYLOAD[op]}
                    </th>
                  ))}
                </tr>
              </thead>
              <tbody className="divide-y divide-outline-variant">
                {names.map((table) => (
                  <tr key={table}>
                    <td className="py-1 font-mono text-xs text-on-surface">{table}</td>
                    {OPS.map((op) => (
                      <td key={op} className="py-1 text-center">
                        <input
                          type="checkbox"
                          className="size-4 accent-primary"
                          aria-label={`${table} ${op}`}
                          checked={draft.on[table]?.includes(op) ?? false}
                          onChange={() => setDraft(toggled(draft, table, op))}
                        />
                      </td>
                    ))}
                  </tr>
                ))}
              </tbody>
            </table>
          )}
          {/* All three of a table's changes is what the config calls `.changed`, so show
              the endpoint the words it will be saved in rather than the grid's. */}
          {subscriptions(draft).length > 0 && (
            <p className="mt-2 font-mono text-[11px] break-words text-on-surface-variant">
              on: [{subscriptions(draft).join(", ")}]
            </p>
          )}
        </div>

        <div className="flex flex-col items-start gap-1.5">
          <span className="text-xs font-medium text-on-surface-variant">Each delivery carries</span>
          <Segmented
            options={["the changed row", "keys only"] as const}
            value={draft.rows ? "the changed row" : "keys only"}
            onChange={(pick) => setDraft({ ...draft, rows: pick === "the changed row" })}
          />
          <span className="text-xs text-on-surface-variant">
            {draft.rows
              ? "The row as it now stands — enough to act on without asking for it."
              : "Only the key. Your server reads the row itself, so it always gets the newest one."}
          </span>
        </div>

        {tried && wrong.length > 0 && (
          <ul className="flex flex-col gap-1 text-xs text-error">
            {wrong.map((problem) => (
              <li key={problem}>{problem}</li>
            ))}
          </ul>
        )}
      </div>
    </Dialog>
  );
}
