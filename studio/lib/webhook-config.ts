// Webhook endpoints as this page edits them, and as `nineveh.yaml` holds them.
//
// The config is the only record of where a project's changes go, so adding an endpoint
// here is an edit to that file. The page reads the parsed endpoints back from the
// control plane — which is the authority on what the file says — edits this shape, and
// writes the block out as text again.
//
// Saving is cheap: `webhooks` isn't part of a build's fingerprint, so no edit made here
// ever rebuilds a table. That is what makes a tight loop on this page possible at all —
// change a URL, see what happens, change it again.

import type { WebhookInfo } from "./api";

/**
 * The row changes one endpoint can ask for, one per checkbox.
 *
 * `changed` isn't among them, because it isn't a fourth kind of change — delivery treats
 * it as matching any op, which makes it exactly these three at once. So the form holds
 * three booleans per table and the two spellings round-trip: `balances.changed` ticks
 * all three, and all three ticked writes `balances.changed` back.
 */
export const OPS = ["inserted", "updated", "deleted"] as const;
export type Op = (typeof OPS)[number];

/**
 * The same three changes as a delivery names them, in its `op` field. The config says
 * `inserted` and a payload says `insert`, so a form that shows both spellings at once
 * is where someone can see they are the same thing.
 */
export const OP_IN_PAYLOAD: Record<Op, string> = {
  inserted: "insert",
  updated: "update",
  deleted: "delete",
};

/** An endpoint as the form edits it. */
export type Draft = {
  name: string;
  url: string;
  /** Which changes each table sends. A table with none isn't subscribed at all. */
  on: Record<string, Op[]>;
  /** Whether a delivery carries the changed row, not only the key that identifies it. */
  rows: boolean;
};

export function blank(): Draft {
  return { name: "", url: "", on: {}, rows: true };
}

/** An endpoint the config already holds, in the shape the form edits. */
export function toDraft(hook: WebhookInfo): Draft {
  const on: Record<string, Op[]> = {};
  for (const entry of hook.on) {
    const dot = entry.lastIndexOf(".");
    if (dot <= 0) continue;
    const table = entry.slice(0, dot);
    const change = entry.slice(dot + 1);
    const asked = change === "changed" ? OPS : OPS.filter((op) => op === change);
    const already = on[table] ?? [];
    on[table] = OPS.filter((op) => already.includes(op) || asked.includes(op));
  }
  return { name: hook.name, url: hook.url, on, rows: hook.rows };
}

/** What `on:` says: a table with every op written as the `.changed` it means. */
export function subscriptions(draft: Draft): string[] {
  return Object.entries(draft.on)
    .filter(([, ops]) => ops.length > 0)
    .flatMap(([table, ops]) =>
      ops.length === OPS.length
        ? [`${table}.changed`]
        : OPS.filter((op) => ops.includes(op)).map((op) => `${table}.${op}`),
    );
}

/** `draft` with one checkbox flipped. */
export function toggled(draft: Draft, table: string, op: Op): Draft {
  const had = draft.on[table] ?? [];
  const on = { ...draft.on };
  const wanted = had.includes(op) ? had.filter((o) => o !== op) : [...had, op];
  if (wanted.length === 0) delete on[table];
  else on[table] = OPS.filter((o) => wanted.includes(o));
  return { ...draft, on };
}

/** Lower snake case, as every name in the config is. */
const NAME = /^[a-z][a-z0-9_]{0,62}$/;

/**
 * What's wrong with `draft` before the control plane sees it. These are the config's own
 * rules, said again here so a typo costs a keystroke instead of a round trip — and so
 * the Save button can be honest about whether pressing it would work.
 */
export function problems(draft: Draft, taken: string[]): string[] {
  const found: string[] = [];
  if (!draft.name.trim()) {
    found.push("Give the endpoint a name: its secret and its delivery cursor belong to it.");
  } else if (!NAME.test(draft.name)) {
    found.push("Names are lower snake case: a–z, 0–9 and _, starting with a letter.");
  } else if (taken.includes(draft.name)) {
    found.push(`This project already has an endpoint called ${draft.name}.`);
  }
  const url = urlProblem(draft.url.trim());
  if (url) found.push(url);
  if (subscriptions(draft).length === 0) {
    found.push("Pick at least one change to send — an endpoint that asks for nothing is never called.");
  }
  return found;
}

/**
 * Why `url` can't be delivered to, in the same terms the config uses: https, except to
 * the local machine while you're building. A signature says who sent a delivery; it
 * can't stop plain http handing the body to everything on the path.
 *
 * Deliberately no more permissive than `nineveh-config`'s own check. Being stricter
 * would refuse a URL that works; being looser would accept one here and fail on save,
 * which is the round trip this exists to avoid.
 */
function urlProblem(url: string): string | null {
  if (!url) return "Say where deliveries should go.";
  if (/\s/.test(url)) return "The URL has whitespace in it.";
  const scheme = ["https://", "http://"].find((s) => url.startsWith(s));
  if (!scheme) return "The URL has to start with https://.";
  const authority = url.slice(scheme.length).split(/[/?#]/)[0] ?? "";
  if (!authority) return "The URL has no host.";
  if (authority.includes("@")) {
    return "Don't put credentials in the URL — deliveries are signed instead.";
  }
  // `[::1]:3000` — the colons inside the brackets belong to the address, not the port.
  const host = authority.startsWith("[")
    ? (authority.slice(1).split("]")[0] ?? "")
    : (authority.split(":")[0] ?? "");
  if (scheme === "http://" && !["localhost", "127.0.0.1", "::1"].includes(host)) {
    return "Use https — plain http only reaches your own machine.";
  }
  return null;
}

/** The indent a line starts with, or `null` when it is blank and belongs to neither side. */
function indentOf(line: string): number | null {
  return line.trim() ? line.length - line.trimStart().length : null;
}

/**
 * `config` with its `webhooks:` block rewritten to hold exactly `hooks`, and removed
 * altogether when there are none.
 *
 * The whole block is regenerated, not the one entry that changed. Finding where a single
 * entry ends needs a YAML parser, and a parser that's slightly wrong rewrites the wrong
 * part of someone's config — so a comment written inside this block doesn't survive an
 * edit made here. Everything outside it does.
 */
export function withWebhooks(config: string, hooks: Draft[]): string {
  const lines = config.replace(/\s+$/, "").split(/\r?\n/);
  const body = hooks.length === 0 ? [] : ["webhooks:", ...hooks.flatMap(entry)];
  const at = lines.findIndex((line) => line.startsWith("webhooks:"));

  if (at === -1) {
    // Nothing to replace. The block goes last, below the state it delivers.
    return `${(body.length === 0 ? lines : [...lines, "", ...body]).join("\n")}\n`;
  }

  // The block runs to the next line that starts a key of its own. Blank lines inside it
  // are part of it; the ones after its last entry separate it from whatever follows.
  let end = at + 1;
  while (end < lines.length && indentOf(lines[end] ?? "") !== 0) end += 1;
  while (end > at + 1 && !(lines[end - 1] ?? "").trim()) end -= 1;

  const before = lines.slice(0, at);
  // The last endpoint takes the key with it: `webhooks:` above nothing isn't an empty
  // block, it's a config that no longer parses.
  if (body.length === 0) while (before.length > 0 && !(before.at(-1) ?? "").trim()) before.pop();

  return `${[...before, ...body, ...lines.slice(end)].join("\n")}\n`;
}

/** One endpoint as the config writes it. */
function entry(hook: Draft): string[] {
  const lines = [
    `  ${hook.name}:`,
    `    url: ${scalar(hook.url.trim())}`,
    `    on: [${subscriptions(hook).join(", ")}]`,
  ];
  // `rows: true` is the default, so only turning it off is worth writing down.
  if (!hook.rows) lines.push("    rows: false");
  return lines;
}

/** A URL as YAML: quoted only when writing it plainly would change what it means. */
function scalar(url: string): string {
  return /^[a-z][\w+.-]*:\/\/[^\s"'#&*!|>%@`{}[\],]+$/i.test(url) ? url : JSON.stringify(url);
}
