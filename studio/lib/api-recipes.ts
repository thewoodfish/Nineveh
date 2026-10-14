// The requests a developer actually makes against their own state, written out rather
// than assembled.
//
// Nineveh's REST surface is small on purpose: three routes, a bounded set of
// comparisons, up to four sort columns, and a page. A query builder over that surface
// offers a shape for every combination and a reason for none of them — and it quietly
// invites the thing the API still can't do, which is aggregate. So what's offered here
// is the handful of requests that surface can answer, each named for what someone is
// building when they need it. The list is short because the API is, and every entry is
// one the API really serves.

import type { Table } from "./api";
import { isInteger } from "./state-table";

/** Something a recipe needs filled in before it can run. */
export type Need = {
  id: string;
  label: string;
  /** A column to choose among, or free text when absent. */
  columns?: string[];
  placeholder?: string;
};

export type Recipe = {
  id: string;
  title: string;
  /** What someone is building when they reach for this one. */
  why: string;
  needs: Need[];
  /** Its default answers, so a recipe runs the moment it is opened. */
  defaults: Record<string, string>;
  /** The request, given what the needs were answered with. */
  path: (values: Record<string, string>) => string;
};

/** How many rows the examples ask for: enough to see the shape, few enough to read. */
const PAGE = 20;

/** What the whole project answers, whichever table you came in looking at. */
export function projectRecipes(): Recipe[] {
  return [
    {
      id: "tables",
      title: "Every table, and how big it is",
      why: "What this project exposes. The first call an integration makes, and the one that tells you the column names to expect.",
      needs: [],
      defaults: {},
      path: () => "/v1/tables?counts=true",
    },
    {
      id: "status",
      title: "Is it caught up with the chain?",
      why: "The cursor, the chain head, and whether a rebuild is running. Worth checking before trusting a number you just read.",
      needs: [],
      defaults: {},
      path: () => "/v1/status",
    },
  ];
}

/** What one table answers. */
export function tableRecipes(table: Table): Recipe[] {
  const at = `/v1/tables/${encodeURIComponent(table.name)}`;
  const named = table.columns.map((c) => c.name);
  const numeric = table.columns.filter((c) => isInteger(c.type)).map((c) => c.name);
  const filterable = table.columns.filter((c) => c.type !== "json").map((c) => c.name);

  const recipes: Recipe[] = [
    {
      id: "latest",
      title: "What changed most recently",
      why: "The default order is the most recently changed row first, so this is a feed of the table without asking for one.",
      needs: [],
      defaults: {},
      path: () => `${at}?limit=${PAGE}`,
    },
  ];

  // A log table has no key to look a row up by, and nothing to say about one.
  if (table.key.length > 0) {
    recipes.push({
      id: "one",
      title: `One row, by its ${table.key.length === 1 ? "key" : "keys"}`,
      why: `The point read: ${table.key.join(" and ")} identifies a row, so this is how a page loads the one it is about.`,
      needs: table.key.map((column) => ({
        id: column,
        label: column,
        placeholder: "the value to match",
      })),
      defaults: {},
      path: (values) => {
        const params = new URLSearchParams({ limit: "1" });
        for (const column of table.key) params.set(column, values[column] ?? "");
        return `${at}?${params}`;
      },
    });
  }

  if (filterable.length > 0) {
    recipes.push({
      id: "where",
      title: "Every row where a column equals something",
      why: "The plain filter. Several of them narrow together, and a column named twice is how you ask for a range.",
      needs: [
        { id: "column", label: "column", columns: filterable },
        { id: "value", label: "equals", placeholder: "the value to match" },
      ],
      defaults: { column: filterable[0] ?? "" },
      path: (values) => {
        const params = new URLSearchParams({ limit: String(PAGE) });
        if (values.column) params.set(values.column, values.value ?? "");
        return `${at}?${params}`;
      },
    });
  }

  if (numeric.length > 0) {
    recipes.push({
      id: "range",
      title: "Every row between two numbers",
      why: "A column named twice, once with each bound. The operators are eq, neq, gt, gte, lt and lte, each one a single comparison against a single column — which is what an index can answer. There is no or, and nothing spanning two columns: that belongs in a reducer.",
      needs: [
        { id: "column", label: "column", columns: numeric },
        { id: "from", label: "at least", placeholder: "lower bound" },
        { id: "to", label: "under", placeholder: "upper bound" },
      ],
      defaults: { column: numeric[0] ?? "" },
      path: (values) => {
        const params = new URLSearchParams({ limit: String(PAGE) });
        if (values.column) {
          params.set(`${values.column}.gte`, values.from ?? "");
          params.set(`${values.column}.lt`, values.to ?? "");
        }
        return `${at}?${params}`;
      },
    });
    recipes.push({
      id: "top",
      title: "The top ten by a number",
      why: "A leaderboard. Sorting happens in the database, so this stays one request however many rows the table holds. The second column breaks ties, without which page two depends on what the planner felt like.",
      needs: [{ id: "column", label: "order by", columns: numeric }],
      defaults: { column: numeric[0] ?? "" },
      path: (values) => {
        const by = [`${values.column ?? ""}.desc`, ...(table.key[0] ? [table.key[0]] : [])];
        return `${at}?order=${encodeURIComponent(by.join(","))}&limit=10`;
      },
    });
  }

  recipes.push(
    {
      id: "page",
      title: "The next page",
      // Plain text, not markdown: backticks around a name would be rendered, not read.
      why: "Paging is offset and limit. Order by something stable — a key, or _version — or a row that changes mid-page can arrive twice.",
      needs: [
        {
          id: "order",
          label: "order by",
          columns: ["_version", ...named],
        },
      ],
      defaults: { order: table.key[0] ?? "_version" },
      path: (values) =>
        `${at}?order=${encodeURIComponent(values.order ?? "_version")}&limit=${PAGE}&offset=${PAGE}`,
    },
    {
      id: "count",
      title: "How many rows there are",
      why: "The count without the rows. Asking for none of them is what makes this cheap enough to put on a dashboard.",
      needs: [],
      defaults: {},
      path: () => `${at}?limit=0&count=exact`,
    },
  );

  return recipes;
}

/** Everything a recipe needs, answered — an unanswered one would send an empty value. */
export function ready(recipe: Recipe, values: Record<string, string>): boolean {
  return recipe.needs.every((need) => (values[need.id] ?? "").trim() !== "");
}
