// A table's own block, lifted out of the project's `nineveh.yaml`.
//
// `GET /v1/tables` says a table's kind, key and columns, but not what it is built from:
// a table named `open_listings` can mirror a source named `listings`, and nothing in the
// served shape connects the two. The config is the only thing on the client that knows,
// and Studio already has it, so this reads it rather than asking for a new endpoint.
//
// Line-based on purpose. A YAML parser would be the honest tool for a config Nineveh had
// to *act* on, but this only quotes what the user wrote back at them, and getting it
// wrong costs a panel that doesn't render — never a wrong answer about what is running.

/** What `state:` says about one table. */
export type Definition = {
  /** The table's block, verbatim, comments above it included. */
  text: string;
  /** The source a `mirror:` or `log:` table follows, if it names one. */
  source: string | null;
};

const INDENT = /^(\s*)\S/;

/** How far a line is indented, or `null` for a blank or comment-only line. */
function indentOf(line: string): number | null {
  if (!line.trim()) return null;
  const found = INDENT.exec(line);
  return found?.[1]?.length ?? 0;
}

export function definitionOf(config: string, table: string): Definition | null {
  const lines = config.split("\n");
  const state = lines.findIndex((line) => /^state:\s*$/.test(line));
  if (state < 0) return null;

  // Table names are validated identifiers, so there is nothing in one to escape.
  const declaration = new RegExp(`^(\\s+)${table}:`);
  let at = -1;
  let indent = 0;
  for (let i = state + 1; i < lines.length; i++) {
    const line = lines[i] ?? "";
    // Out of `state:` again: a non-blank line back at column zero.
    if (indentOf(line) === 0) break;
    const found = declaration.exec(line);
    if (found) {
      at = i;
      indent = found[1]?.length ?? 0;
      break;
    }
  }
  if (at < 0) return null;

  // Everything under it, plus the comment lines immediately above — which is where
  // whoever wrote the config said why the table exists.
  let from = at;
  while (from > state + 1 && /^\s*#/.test(lines[from - 1] ?? "")) from -= 1;
  let to = at + 1;
  while (to < lines.length) {
    const depth = indentOf(lines[to] ?? "");
    if (depth !== null && depth <= indent) break;
    to += 1;
  }
  // A trailing comment belongs to the table below, not this one.
  while (to > at + 1 && /^\s*#/.test(lines[to - 1] ?? "")) to -= 1;

  const text = lines.slice(from, to).join("\n").replace(/\s+$/, "");
  return { text, source: /\b(?:mirror|log):\s*([A-Za-z_]\w*)/.exec(text)?.[1] ?? null };
}
