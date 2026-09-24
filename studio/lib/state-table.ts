// A state table being designed, and the reducers file it becomes.
//
// A reduce table is a key, typed columns, and rules that fold records into them
// (ADR 0011). Studio writes them in the DSL (ADR 0025), so a table built here and one
// written by hand are the same artifact — you can open either in the other. The
// control plane checks it (`control.check`) and is the authority on whether it holds.
//
// Expressions here are DSL expressions: a record's field is `r.amount`, and the row
// being written is `b.balance`. Nothing is bare.

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

/** The handler's parameter: the record a rule is folding. */
export const RECORD = "r";

/** The row a rule writes. */
export const ROW = "b";

/** A record's field: `r.amount`. */
const of = (_source: SourceInfo, field: { name: string }): string => `${RECORD}.${field.name}`;

/** The row's own column: `b.count`, which is its value before this rule's write. */
const own = (column: string): string => `${ROW}.${column}`;

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
          { column: "count", expression: `${own("count")} + 1` },
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
  const read = of(source, amount);
  const cast = total === amount.type ? read : `${total}(${read})`;
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
          {
            column: `total_${amount.name}`,
            expression: `${own(`total_${amount.name}`)} + ${cast}`,
          },
          { column: "count", expression: `${own("count")} + 1` },
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
          ...rest.map((f) => ({ column: f.name, expression: of(source, f) })),
          { column: "version", expression: "tx.version" },
        ],
        removes: false,
      },
    ],
  };
}

/** Microseconds in a day: what `tx.timestamp` is divided by to bucket by day. */
const DAY = "86_400_000_000";

/**
 * One row per `field` per day: what a chart is made of. The day is the key column the
 * record doesn't carry, so the rule maps it from the transaction's own timestamp.
 */
export function dailyPer(source: SourceInfo, field: FieldInfo, amount?: FieldInfo): StateTable {
  const total = amount ? widened(amount.type) : undefined;
  const read = amount ? of(source, amount) : "";
  const cast = amount && total === amount.type ? read : `${total}(${read})`;
  return {
    name: amount ? `daily_${amount.name}_per_${field.name}` : `daily_${source.name}_per_${field.name}`,
    columns: [
      { name: field.name, type: field.type, default: "", nullable: false, key: true },
      { name: "day", type: "u64", default: "", nullable: false, key: true },
      ...(amount && total
        ? [
            {
              name: `total_${amount.name}`,
              type: total,
              default: "0",
              nullable: false,
              key: false,
            },
          ]
        : []),
      { name: "count", type: "u64" as ColumnType, default: "0", nullable: false, key: false },
    ],
    rules: [
      {
        on: source.name,
        deleted: false,
        when: "",
        keys: [{ column: "day", expression: `tx.timestamp / ${DAY}` }],
        sets: [
          ...(amount
            ? [
                {
                  column: `total_${amount.name}`,
                  expression: `${own(`total_${amount.name}`)} + ${cast}`,
                },
              ]
            : []),
          { column: "count", expression: `${own("count")} + 1` },
        ],
        removes: false,
      },
    ],
  };
}

/**
 * Rows that appear when one record arrives and disappear when another does: what's
 * open right now, from events the contract already emits.
 */
export function liveSet(
  source: SourceInfo,
  field: FieldInfo,
  gone: { name: string; deleted: boolean },
): StateTable {
  const rest = source.fields.filter((f) => f.name !== field.name);
  return {
    name: `open_${source.name}`,
    columns: [
      { name: field.name, type: field.type, default: "", nullable: false, key: true },
      ...rest.map((f) => ({
        name: f.name,
        type: f.type,
        default: zero(f.type),
        nullable: f.nullable,
        key: false,
      })),
      { name: "since", type: "u64" as ColumnType, default: "0", nullable: false, key: false },
    ],
    rules: [
      {
        on: source.name,
        deleted: false,
        when: "",
        keys: [],
        sets: [
          ...rest.map((f) => ({ column: f.name, expression: of(source, f) })),
          { column: "since", expression: "tx.timestamp" },
        ],
        removes: false,
      },
      { on: gone.name, deleted: gone.deleted, when: "", keys: [], sets: [], removes: true },
    ],
  };
}

/**
 * How many records each `field` has, plus a column read from another table — null
 * when that table has no row for it (ADR 0019).
 */
export function withLookup(
  source: SourceInfo,
  field: FieldInfo,
  table: { name: string; key: string[] },
  column: { name: string; type: ColumnType },
): StateTable {
  return {
    name: `${source.name}_with_${column.name}`,
    columns: [
      { name: field.name, type: field.type, default: "", nullable: false, key: true },
      { name: "count", type: "u64", default: "0", nullable: false, key: false },
      { name: column.name, type: column.type, default: "", nullable: true, key: false },
    ],
    rules: [
      {
        on: source.name,
        deleted: false,
        when: "",
        keys: [],
        sets: [
          { column: "count", expression: `${own("count")} + 1` },
          {
            column: column.name,
            expression: `${table.name}.get(${of(source, field)})?.${column.name}`,
          },
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

/** A column's type as the DSL declares it: `u128.default(0)`, `string.nullable()`. */
function declared(column: Column): string {
  const parts: string[] = [column.type];
  if (column.default !== "") parts.push(`default(${column.default})`);
  if (column.nullable) parts.push("nullable()");
  return parts.join(".");
}

/** The widest name in a block, so the declarations line up as a person would write them. */
function pad(names: string[]): number {
  return names.reduce((wide, name) => Math.max(wide, name.length), 0);
}

/** Each key column's expression, in key order: what `row(…)` is called with. */
function keyArgs(table: StateTable, rule: Rule): string[] {
  return table.columns
    .filter((c) => c.key)
    .map((c) => {
      const mapped = rule.keys.find((k) => k.column === c.name);
      // A key column the rule doesn't map reads the record's field of that name.
      return (mapped ? mapped.expression : `${RECORD}.${c.name}`).trim();
    });
}

/** `b.count = b.count + 1` reads better as `b.count += 1`, and means the same. */
function assignment(column: string, expression: string): string {
  const self = `${own(column)} `;
  const trimmed = expression.trim();
  for (const op of ["+", "-"]) {
    if (trimmed.startsWith(`${self}${op} `)) {
      return `  ${own(column)} ${op}= ${trimmed.slice(self.length + 2)}`;
    }
  }
  return `  ${own(column)} = ${trimmed}`;
}

/** One rule as a handler body, indented inside its `on(…)`. */
function body(table: StateTable, rule: Rule): string[] {
  const row = `${table.name}.row(${keyArgs(table, rule).join(", ")})`;
  if (rule.removes) return [`  ${row}.delete()`];
  const lines = [`  const ${ROW} = ${row}`];
  for (const set of rule.sets) lines.push(assignment(set.column, set.expression));
  return lines;
}

/**
 * This table as DSL: its declaration, then one handler per rule.
 *
 * One handler each, rather than one per source, because the editor builds one table
 * at a time. A person writing by hand would put everything one event changes in a
 * single handler; both compile to the same rules.
 */
export function toDsl(table: StateTable): string {
  const keys = table.columns.filter((c) => c.key);
  const rest = table.columns.filter((c) => !c.key);
  const keyWidth = pad(keys.map((c) => c.name));
  const restWidth = pad(rest.map((c) => c.name));

  const lines: string[] = [];
  lines.push(`export const ${table.name} = table({`);
  lines.push(
    `  key:     { ${keys.map((c) => `${c.name}:${" ".repeat(keyWidth - c.name.length)} ${c.type}`).join(", ")} },`,
  );
  if (rest.length === 0) {
    lines.push("  columns: {},");
  } else {
    lines.push("  columns: {");
    for (const column of rest) {
      const gap = " ".repeat(restWidth - column.name.length);
      lines.push(`    ${column.name}:${gap} ${declared(column)},`);
    }
    lines.push("  },");
  }
  lines.push("})");

  for (const rule of table.rules) {
    const on = `${rule.on}${rule.deleted ? ".deleted" : ""}`;
    lines.push("");
    lines.push(`on(${on}, (${RECORD}) => {`);
    const when = rule.when.trim();
    if (when) {
      lines.push(`  if (${when}) {`);
      for (const line of body(table, rule)) lines.push(`  ${line}`);
      lines.push("  }");
    } else {
      lines.push(...body(table, rule));
    }
    lines.push("})");
  }
  return `${lines.join("\n")}\n`;
}

/**
 * `reducers` with `table` in it: replacing the block of that name if it's there, and
 * appended if it isn't.
 *
 * A table's block runs from its `export const` line to the next one, which is how
 * `toDsl` lays it out. Handlers written between two declarations belong to the one
 * above them.
 */
export function withDslTable(reducers: string, table: StateTable): string {
  const lines = reducers.split("\n");
  const declaration = new RegExp(`^export const ${table.name}\\b`);
  const at = lines.findIndex((line) => declaration.test(line));
  if (at < 0) {
    const before = reducers.trimEnd();
    return before ? `${before}\n\n${toDsl(table)}` : toDsl(table);
  }
  let to = lines.length;
  for (let i = at + 1; i < lines.length; i++) {
    if (/^export const /.test(lines[i] ?? "")) {
      to = i;
      break;
    }
  }
  const before = lines.slice(0, at).join("\n");
  const after = lines.slice(to).join("\n").replace(/^\n+/, "");
  return `${before ? `${before.trimEnd()}\n\n` : ""}${toDsl(table)}${after ? `\n${after}` : ""}`;
}

/**
 * `config` with a `reducers:` key naming `file`, added above `sources:` where the rest
 * of the header is. A config that already has one is left alone.
 */
export function withReducersKey(config: string, file: string): string {
  if (/^reducers:/m.test(config)) return config;
  const line = `reducers: ./${file}\n`;
  const at = config.search(/^sources:/m);
  return at === -1
    ? `${config.trimEnd()}\n${line}`
    : `${config.slice(0, at)}${line}\n${config.slice(at)}`;
}

/** The reducers file a project's tables live in. */
export function reducersFile(project: string): string {
  return `${project}.nineveh.ts`;
}
