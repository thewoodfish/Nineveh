// A state table being designed, and the `nineveh.yaml` it becomes.
//
// A reduce table is a key, typed columns, and rules that fold records into them
// (ADR 0011). Studio builds the YAML; the control plane checks it (`control.check`)
// and is the authority on whether it holds up.

import type { ColumnType, FieldInfo, SourceInfo } from "./api";

export type Column = {
  name: string;
  type: ColumnType;
  /** As written in YAML: `0`, `"text"`, `"0x1"`. Empty for none. */
  default: string;
  nullable: boolean;
  /** Part of the key: what identifies a row. */
  key: boolean;
};

export type Assignment = { column: string; expression: string };

export type Rule = {
  /** The source's name. */
  on: string;
  /** A `<source>.deleted` rule: the record is a delete. */
  deleted: boolean;
  /** Optional condition. */
  when: string;
  /** Key columns the record doesn't name: `{ user: "seller" }`. */
  keys: Assignment[];
  sets: Assignment[];
  /** Delete the row instead of setting columns. */
  removes: boolean;
};

export type StateTable = { name: string; columns: Column[]; rules: Rule[] };

export const COLUMN_TYPES: ColumnType[] = [
  "address",
  "string",
  "bool",
  "u8",
  "u16",
  "u32",
  "u64",
  "u128",
  "u256",
  "i8",
  "i16",
  "i32",
  "i64",
  "i128",
  "i256",
  "bytes",
  "json",
];

/** An integer column wide enough to add up many `type` values without overflowing. */
function widened(type: ColumnType): ColumnType {
  const widths: Partial<Record<ColumnType, ColumnType>> = {
    u8: "u64",
    u16: "u64",
    u32: "u64",
    u64: "u128",
    u128: "u256",
    i8: "i64",
    i16: "i64",
    i32: "i64",
    i64: "i128",
    i128: "i256",
  };
  return widths[type] ?? type;
}

export function isInteger(type: ColumnType): boolean {
  return /^[ui]\d+$/.test(type);
}

/** Fields worth keying a table by: what identifies someone or something. */
export function keyFields(source: SourceInfo): FieldInfo[] {
  return source.fields.filter((f) => f.type === "address" || isInteger(f.type) || f.type === "string");
}

/** Fields worth adding up. */
export function amountFields(source: SourceInfo): FieldInfo[] {
  return source.fields.filter((f) => isInteger(f.type) && !f.nullable);
}

const zero = (type: ColumnType): string => (isInteger(type) ? "0" : "");

/** How many of this source's records there are, per `field`. */
export function countPer(source: SourceInfo, field: FieldInfo): StateTable {
  return {
    name: `${source.name}_per_${field.name}`,
    columns: [
      { name: field.name, type: field.type, default: "", nullable: false, key: true },
      { name: "count", type: "u64", default: "0", nullable: false, key: false },
      { name: "last_seen", type: "u64", default: "0", nullable: false, key: false },
    ],
    rules: [
      {
        on: source.name,
        deleted: false,
        when: "",
        keys: [],
        sets: [
          { column: "count", expression: "count + 1" },
          { column: "last_seen", expression: "tx.timestamp" },
        ],
        removes: false,
      },
    ],
  };
}

/** `amount` added up per `field`, in a column wide enough to hold the total. */
export function sumPer(source: SourceInfo, field: FieldInfo, amount: FieldInfo): StateTable {
  const total = widened(amount.type);
  const cast = total === amount.type ? amount.name : `${total}(${amount.name})`;
  return {
    name: `${amount.name}_per_${field.name}`,
    columns: [
      { name: field.name, type: field.type, default: "", nullable: false, key: true },
      { name: `total_${amount.name}`, type: total, default: "0", nullable: false, key: false },
      { name: "count", type: "u64", default: "0", nullable: false, key: false },
    ],
    rules: [
      {
        on: source.name,
        deleted: false,
        when: "",
        keys: [],
        sets: [
          { column: `total_${amount.name}`, expression: `total_${amount.name} + ${cast}` },
          { column: "count", expression: "count + 1" },
        ],
        removes: false,
      },
    ],
  };
}

/** The newest record per `field`, field by field. */
export function latestPer(source: SourceInfo, field: FieldInfo): StateTable {
  const rest = source.fields.filter((f) => f.name !== field.name);
  return {
    name: `latest_${source.name}_per_${field.name}`,
    columns: [
      { name: field.name, type: field.type, default: "", nullable: false, key: true },
      ...rest.map((f) => ({
        name: f.name,
        type: f.type,
        default: zero(f.type),
        nullable: f.nullable,
        key: false,
      })),
      { name: "version", type: "u64", default: "0", nullable: false, key: false },
    ],
    rules: [
      {
        on: source.name,
        deleted: false,
        when: "",
        keys: [],
        sets: [
          ...rest.map((f) => ({ column: f.name, expression: f.name })),
          { column: "version", expression: "tx.version" },
        ],
        removes: false,
      },
    ],
  };
}

export function blank(source: SourceInfo | undefined): StateTable {
  return {
    name: "",
    columns: [{ name: "", type: "address", default: "", nullable: false, key: true }],
    rules: [
      {
        on: source?.name ?? "",
        deleted: false,
        when: "",
        keys: [],
        sets: [],
        removes: false,
      },
    ],
  };
}

/** What's wrong with the table before the server ever sees it. */
export function problems(table: StateTable): string[] {
  const found: string[] = [];
  const name = /^[a-z][a-z0-9_]*$/;
  if (!name.test(table.name)) found.push("The table needs a name in lower snake case.");
  if (table.columns.some((c) => !name.test(c.name))) found.push("Every column needs a name in lower snake case.");
  if (!table.columns.some((c) => c.key)) found.push("Tick at least one column as part of the key.");
  const names = table.columns.map((c) => c.name);
  if (new Set(names).size !== names.length) found.push("Two columns have the same name.");
  if (table.rules.length === 0) found.push("Add a rule: without one, nothing fills the table.");
  if (table.rules.some((r) => !r.on)) found.push("Every rule needs a source.");
  if (table.rules.some((r) => !r.removes && r.sets.length === 0))
    found.push("A rule either sets columns or deletes the row.");
  if (table.rules.some((r) => r.sets.some((s) => !s.column || !s.expression.trim())))
    found.push("Every `set` needs a column and an expression.");
  return found;
}

/** The YAML for this table, indented as part of `state:`. */
export function toYaml(table: StateTable): string {
  const quote = (expression: string) => `"${expression.replace(/"/g, "'")}"`;
  const lines: string[] = [];
  lines.push(`  ${table.name}:`);
  lines.push(`    key: [${table.columns.filter((c) => c.key).map((c) => c.name).join(", ")}]`);
  lines.push("    columns:");
  for (const column of table.columns) {
    const extras = [
      `type: ${column.type}`,
      column.default !== "" ? `default: ${column.default}` : "",
      column.nullable ? "nullable: true" : "",
    ].filter(Boolean);
    lines.push(
      extras.length === 1
        ? `      ${column.name}: ${column.type}`
        : `      ${column.name}: { ${extras.join(", ")} }`,
    );
  }
  lines.push("    reduce:");
  for (const rule of table.rules) {
    lines.push(`      - on: ${rule.on}${rule.deleted ? ".deleted" : ""}`);
    if (rule.when.trim()) lines.push(`        when: ${quote(rule.when.trim())}`);
    if (rule.keys.length > 0) {
      lines.push("        key:");
      for (const key of rule.keys) lines.push(`          ${key.column}: ${quote(key.expression.trim())}`);
    }
    if (rule.removes) {
      lines.push("        delete: true");
    } else {
      lines.push("        set:");
      for (const set of rule.sets) lines.push(`          ${set.column}: ${quote(set.expression.trim())}`);
    }
  }
  return lines.join("\n") + "\n";
}

/**
 * `config` with `table` in its `state:` block: replacing the table of that name if
 * it's already there, and added at the end of the block if it isn't. The block ends
 * at the next line that starts a top-level key, so a config with `api:` or
 * `realtime:` after it keeps them below.
 */
export function withTable(config: string, table: StateTable): string {
  const lines = config.split("\n");
  const start = lines.findIndex((line) => /^state:\s*$/.test(line));
  if (start < 0) return `${config.trimEnd()}\n\nstate:\n${toYaml(table)}`;
  let end = lines.length;
  for (let i = start + 1; i < lines.length; i++) {
    const line = lines[i] ?? "";
    if (line.trim() === "" || line.startsWith(" ") || line.startsWith("#")) continue;
    end = i;
    break;
  }
  const block = blockOf(lines, start + 1, end, table.name);
  if (block) {
    const before = lines.slice(0, block.from).join("\n");
    const after = lines.slice(block.to).join("\n");
    return `${before ? `${before}\n` : ""}${toYaml(table)}${after}`;
  }
  const before = lines.slice(0, end).join("\n").replace(/\s*$/, "\n");
  const after = lines.slice(end).join("\n");
  return `${before}${toYaml(table)}${after ? `\n${after}` : ""}`;
}

/** Where `name`'s block sits within `state:`, if it's there at all. */
function blockOf(
  lines: string[],
  from: number,
  until: number,
  name: string,
): { from: number; to: number } | null {
  const header = new RegExp(`^ {2}${name.replace(/[^a-z0-9_]/gi, "")}:\\s*$`);
  const at = lines.findIndex((line, i) => i >= from && i < until && header.test(line));
  if (at < 0) return null;
  let to = until;
  for (let i = at + 1; i < until; i++) {
    const line = lines[i] ?? "";
    // The block ends at the next table's name, which is indented by exactly two.
    if (line.trim() !== "" && !/^ {3}/.test(line)) {
      to = i;
      break;
    }
  }
  return { from: at, to };
}
