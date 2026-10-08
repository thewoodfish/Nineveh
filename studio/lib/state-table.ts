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
  /**
   * A comment to put at the top of the handler. Scaffolding writes one where the rule is
   * a placeholder it can't guess the body of, so the file says what is missing rather
   * than looking finished.
   */
  note?: string;
};

export type StateTable = {
  name: string;
  columns: Column[];
  rules: Rule[];
  /**
   * A line above the declaration saying what the table is in words. Shapes write one,
   * because the next person to open the file — usually the same person, later — reads
   * "one row per seller, counting cancelled records" faster than they re-derive it from
   * the key and the arithmetic.
   */
  note?: string;
};

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

/** Whether `source` carries a field of this exact name and type. */
function carries(source: SourceInfo, field: FieldInfo): boolean {
  return source.fields.some((f) => f.name === field.name && f.type === field.type);
}

/**
 * Fields worth keying by that *every* one of these sources can name.
 *
 * A table folded from several sources is keyed once, so every rule has to be able to
 * reach the row — and a rule reaches it by reading the key column off its own record.
 * Offering a key only the first source carries makes a table that can't compile, and
 * the error arrives much later, in the file, about the handler rather than the key.
 * Intersecting here makes it unrepresentable instead.
 */
export function keyFieldsAcross(sources: SourceInfo[]): FieldInfo[] {
  const [first, ...rest] = sources;
  if (!first) return [];
  return keyFields(first).filter((field) => rest.every((s) => carries(s, field)));
}

/**
 * The sources that share no keyable field with the first, which is the reason there is
 * nothing to key by — worth naming, because the fix is to untick one of them.
 */
export function keylessWith(sources: SourceInfo[]): string[] {
  const [first, ...rest] = sources;
  if (!first) return [];
  const keys = keyFields(first);
  // When the first source has no identifiers of its own, every other one is vacuously
  // blameless: the problem is that source, and saying so is a different message.
  if (keys.length === 0) return [];
  return rest.filter((s) => !keys.some((field) => carries(s, field))).map((s) => s.name);
}

/**
 * What a source could use as the key column, when it doesn't carry it by name.
 *
 * The same party is named differently by each event it appears in — a borrow says
 * `account_addr`, a liquidation says `borrower_addr` — so matching on the name alone
 * refuses tables that are perfectly expressible. Matching on the type alone would be
 * worse: a liquidation carries *two* addresses, and picking the first would credit
 * every liquidation to the liquidator, silently and plausibly.
 *
 * So the type narrows the list and a person chooses from it.
 */
export function keyCandidates(source: SourceInfo, key: FieldInfo): FieldInfo[] {
  return source.fields.filter((f) => f.type === key.type && !f.nullable);
}

/** A source's own name for the key, defaulting to the field of that name if it has one. */
export type KeyMapping = Record<string, string>;

/**
 * The field `source` should be keyed by, given what the person chose.
 *
 * Falls back to the key's own name, which is right whenever the source carries it —
 * and is what every table built before this existed relied on.
 */
export function keyFieldFor(source: SourceInfo, key: FieldInfo, mapping: KeyMapping): string {
  const chosen = mapping[source.name];
  if (chosen && source.fields.some((f) => f.name === chosen)) return chosen;
  return key.name;
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
    note: `How many ${source.name} records each ${field.name} has, and when the last one arrived.`,
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
    note: `${amount.name} on every ${source.name} record, added up per ${field.name}, in a ${total} wide enough to hold the total.`,
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
    note: `The newest ${source.name} for each ${field.name}, field by field.`,
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
    note: `One row per ${field.name} per day, from ${source.name} — ${amount ? `${amount.name} added up` : "records counted"}, bucketed into days of tx.timestamp.`,
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
    note: `A row per ${field.name} while it is open: added on ${source.name}, removed on ${gone.deleted ? `${gone.name} deleted` : gone.name}.`,
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
    note: `How many ${source.name} records per ${field.name}, plus ${column.name} looked up in ${table.name} — null when it has no row for it.`,
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

/** The column whose whole meaning is "a record reached this row". */
const TOUCHED: Column = {
  name: "last_seen",
  type: "u64",
  default: "0",
  nullable: false,
  key: false,
};

/**
 * The column a placeholder rule writes to say only that a record arrived.
 *
 * `last_seen` and nothing else, because it is the one column whose meaning any source
 * may set. Reusing a shape's other timestamps would quietly corrupt them — `since` is
 * when the row opened and `version` is the version of its newest record, and a second
 * source overwriting either states something false while compiling cleanly. A table that
 * hasn't got a `last_seen` gains one instead.
 */
function touched(table: StateTable): Column | null {
  return table.columns.find((c) => !c.key && c.name === TOUCHED.name) ?? null;
}

/**
 * `table`, with a handler for each further source folded into it.
 *
 * The sources of a table are not interchangeable: one brings the row into being and the
 * rest change the row it made. So a shape is built from the first and the others are
 * scaffolded — because what a second source does to a row is the one thing no template
 * can know. A `sold` record might close the row, decrement it, or overwrite a price,
 * and guessing would be worse than leaving it visibly unfinished: a rule that silently
 * does the wrong thing is harder to notice than one that says it is a placeholder.
 *
 * What it writes compiles and is true — the row was touched at `tx.timestamp` — so the
 * file checks out, previews, and shows you rows while you replace the body.
 *
 * The key mapping is copied from the first rule because it belongs to the table, not to
 * the record: a `per day` table buckets every source's records by the same arithmetic.
 */
export function alsoFolds(
  table: StateTable,
  extras: SourceInfo[],
  key?: FieldInfo,
  mapping: KeyMapping = {},
): StateTable {
  if (extras.length === 0) return table;
  const existing = touched(table);
  const keys = table.rules[0]?.keys ?? [];
  // A source that names the key something else says so here, in the one place a rule
  // can carry it: `keys` is what `row(…)` is rendered from, so nothing downstream has
  // to learn about the mapping.
  const keysFor = (extra: SourceInfo) => {
    if (!key) return keys.map((k) => ({ ...k }));
    const field = keyFieldFor(extra, key, mapping);
    return field === key.name
      ? keys.map((k) => ({ ...k }))
      : [{ column: key.name, expression: `${RECORD}.${field}` }];
  };
  return {
    ...table,
    // The description names what the table folds, so a source added here belongs in it.
    note: table.note
      ? `${table.note} Also folds ${extras.map((s) => s.name).join(" and ")}.`
      : table.note,
    // A shape with nowhere to record a touch gains somewhere, rather than the rule
    // having nothing legal to write.
    columns: existing ? table.columns : [...table.columns, TOUCHED],
    rules: [
      ...table.rules,
      ...extras.map((extra) => ({
        on: extra.name,
        deleted: false,
        when: "",
        keys: keysFor(extra),
        sets: [{ column: TOUCHED.name, expression: "tx.timestamp" }],
        removes: false,
        note: `What a ${extra.name} record does to this row — this only records that one arrived.`,
      })),
    ],
  };
}

/**
 * The smallest table that works: the key, and a column recording that a record reached
 * the row.
 *
 * "Empty" means no shape imposed, not literally nothing. It used to mean literally
 * nothing — an unnamed key column of no particular type and a handler that wrote
 * nothing, which rendered as `key: { : address }` and `b = t.row(r.)`. That doesn't
 * parse, and a table with a key but no writes doesn't compile either ("nothing writes
 * table `t`"), so the escape hatch handed you a file that was wrong before you had typed
 * a character. This one is valid and previewable from the first second, and every part
 * of it is meant to be replaced.
 */
export function blank(source: SourceInfo | undefined, field?: FieldInfo): StateTable {
  const key: Column = {
    name: field?.name ?? "id",
    type: field?.type ?? "address",
    default: "",
    nullable: false,
    key: true,
  };
  return {
    name: source ? `${source.name}_per_${key.name}` : "",
    note: source
      ? `One row per ${key.name}, from every ${source.name} record.`
      : `One row per ${key.name}.`,
    columns: [key, TOUCHED],
    rules: [
      {
        on: source?.name ?? "",
        deleted: false,
        when: "",
        keys: [],
        sets: [{ column: TOUCHED.name, expression: "tx.timestamp" }],
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
  if (table.note) lines.push(`// ${table.note}`);
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
    // A note goes inside the handler rather than above it, so it travels with the body
    // it is about when the handlers are reordered or one is deleted.
    if (rule.note) lines.push(`  // ${rule.note}`);
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

/** The first line of the comment block sitting directly above `i`, or `i` itself. */
function lead(lines: string[], i: number): number {
  let at = i;
  while (at > 0 && /^\s*\/\//.test(lines[at - 1] ?? "")) at -= 1;
  return at;
}

/**
 * Where `table`'s block starts and ends: its declaration, the handlers under it, and any
 * comment written directly above it.
 *
 * A block runs from one `export const` to the next, but a comment immediately above a
 * declaration is about that declaration, not about the handlers it happens to follow.
 * Counting it with the block above would make splicing one table carry off the next
 * one's comment, and would duplicate the note `toDsl` writes every time a table was
 * rewritten. A blank line breaks the association, which is what keeps a file-header
 * comment out of the first table's block.
 */
function bounds(lines: string[], table: string): { at: number; to: number } | null {
  const declaration = new RegExp(`^export const ${table}\\b`);
  const found = lines.findIndex((line) => declaration.test(line));
  if (found < 0) return null;
  let to = lines.length;
  for (let i = found + 1; i < lines.length; i++) {
    if (/^export const /.test(lines[i] ?? "")) {
      to = lead(lines, i);
      break;
    }
  }
  return { at: lead(lines, found), to };
}

/**
 * The block of `reducers` that declares `table` and the handlers under it — the same
 * span [`withDslTable`] replaces, read back out.
 *
 * Returns null when the file doesn't declare it, which is what a table built from YAML
 * or one whose file has been reorganised looks like.
 */
export function dslTableBlock(reducers: string, table: string): string | null {
  const lines = reducers.split("\n");
  const span = bounds(lines, table);
  if (!span) return null;
  return lines.slice(span.at, span.to).join("\n").replace(/\s+$/, "");
}

/**
 * The other tables a block writes to.
 *
 * A handler is written event-first — "when a sale arrives, here is everything that
 * changes" — so one `on()` may well write two tables (ADR 0025), and the compiler's
 * scatter pass is what turns that back into per-table rules. A block like that belongs
 * to no single table, and splicing it under one name would take the other's rules with
 * it. So this is the guard: a block that writes elsewhere is edited as a whole file or
 * not at all.
 */
export function foreignWrites(block: string, table: string): string[] {
  const written = new Set<string>();
  for (const [, name] of block.matchAll(/\b([A-Za-z_]\w*)\s*\.\s*row\s*\(/g)) {
    if (name && name !== table) written.add(name);
  }
  return [...written];
}

/**
 * `reducers` with `table` in it: replacing the block of that name if it's there, and
 * appended if it isn't.
 *
 * A table's block runs from its `export const` line — or the comment above it — to the
 * next one, which is how `toDsl` lays it out. Handlers written between two declarations
 * belong to the one above them.
 */
export function withDslTable(reducers: string, table: StateTable): string {
  const lines = reducers.split("\n");
  const span = bounds(lines, table.name);
  if (!span) {
    const before = reducers.trimEnd();
    return before ? `${before}\n\n${toDsl(table)}` : toDsl(table);
  }
  const before = lines.slice(0, span.at).join("\n");
  const after = lines.slice(span.to).join("\n").replace(/^\n+/, "");
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
