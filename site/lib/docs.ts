// The docs pages are built from the repo's own markdown, read at build time. There is
// one source of truth: `docs/guide.md` and friends are what a contributor reads in the
// repo and what the site serves, so the two can never drift.
//
// Static export runs Server Components during `next build`, so the file reads below
// happen once, at build, and never in a browser.

import { readFileSync } from "node:fs";
import { join } from "node:path";

import { marked, type Tokens } from "marked";

export type Doc = {
  /** The route under /docs. The guide is the index, so its slug is empty. */
  slug: string;
  title: string;
  /** One line in the sidebar, under the title. */
  blurb: string;
  /** The markdown file, relative to the repo's `docs/`. */
  file: string;
};

/** Every document the site publishes, in the order the sidebar lists them. */
export const DOCS: Doc[] = [
  {
    slug: "",
    title: "Start here",
    blurb: "What Nineveh is, in five minutes",
    file: "guide.md",
  },
  {
    slug: "first-backend",
    title: "Your first backend",
    blurb: "A working project, end to end",
    file: "first-backend.md",
  },
  {
    slug: "reducers",
    title: "Reducers",
    blurb: "Saying what your tables hold",
    file: "reducers.md",
  },
  {
    slug: "reading",
    title: "Reading your data",
    blurb: "REST, the change feed, webhooks",
    file: "reading.md",
  },
  {
    slug: "running",
    title: "Running a project",
    blurb: "Changes, limits, and what to check",
    file: "running.md",
  },
  {
    slug: "configuration",
    title: "Configuration",
    blurb: "Every key in nineveh.yaml",
    file: "config.md",
  },
  {
    slug: "expressions",
    title: "Expressions",
    blurb: "The language values are written in",
    file: "expressions.md",
  },
];

export function docHref(doc: Doc): string {
  return doc.slug ? `/docs/${doc.slug}` : "/docs";
}

/** A heading in the rendered page, for the contents rails. */
export type Entry = {
  id: string;
  text: string;
  /** 2 for a section, 3 for something inside one. */
  level: number;
};

export type Rendered = {
  html: string;
  /** Every `##` and `###`, in document order. */
  headings: Entry[];
  /** The document's `#` title. */
  title: string;
  /** The paragraph under the title, as plain text, for the page's description. */
  summary: string;
};

/**
 * Headings are numbered in the source (`## 3. Path A — …`) so the guide reads as a
 * sequence on GitHub. On the site the number lives in the margin rail instead, so it
 * is split off here rather than repeated in the heading itself.
 */
function splitNumber(text: string): { number?: string; rest: string } {
  const match = /^(\d+)\.\s+(.*)$/.exec(text);
  return match ? { number: match[1], rest: match[2] ?? text } : { rest: text };
}

function slugify(text: string): string {
  return text
    .toLowerCase()
    .replace(/[^\w\s-]/g, "")
    .trim()
    .replace(/\s+/g, "-");
}

/** Strip inline markdown so a heading's text can be used as a label. */
function plain(markdown: string): string {
  return markdown
    .replace(/`([^`]*)`/g, "$1")
    .replace(/\*\*([^*]*)\*\*/g, "$1")
    .replace(/\*([^*]*)\*/g, "$1")
    .replace(/\[([^\]]*)\]\([^)]*\)/g, "$1");
}

export function render(doc: Doc): Rendered {
  const source = readFileSync(join(process.cwd(), "..", "docs", doc.file), "utf8");
  const headings: Entry[] = [];
  let title = doc.title;
  let summary = "";

  // Walk the tokens once for the contents rails and the page's own title.
  for (const token of marked.lexer(source)) {
    if (token.type === "heading") {
      const heading = token as Tokens.Heading;
      const text = plain(heading.text);
      if (heading.depth === 1) {
        title = text;
      } else if (heading.depth === 2 || heading.depth === 3) {
        const { number, rest } = splitNumber(text);
        void number; // the number is the margin's job, not the label's
        headings.push({ id: slugify(text), text: rest, level: heading.depth });
      }
    }
    if (!summary && token.type === "paragraph") {
      summary = plain((token as Tokens.Paragraph).text).replace(/\n/g, " ");
    }
  }

  const renderer = new marked.Renderer();

  // Headings carry an id to link to, and a `data-number` the stylesheet hangs in the
  // margin — the same rail the landing page uses for version numbers.
  renderer.heading = ({ tokens, depth }) => {
    const text = plain(rawText(tokens));
    const id = slugify(text);
    const { number, rest } = splitNumber(text);
    const inline = marked.parseInline(rest) as string;
    const attr = number ? ` data-number="${number}"` : "";
    return `<h${depth} id="${id}"${attr}><a class="anchor" href="#${id}" aria-label="Link to this section"></a>${inline}</h${depth}>\n`;
  };

  // Links out of the repo's docs point at sibling markdown files; on the site they
  // point at sibling routes.
  renderer.link = ({ href, title: linkTitle, tokens }) => {
    const text = marked.parseInline(rawText(tokens)) as string;
    const mapped = rewrite(href);
    const external = /^https?:/.test(mapped);
    const attrs = external ? ' target="_blank" rel="noreferrer"' : "";
    const t = linkTitle ? ` title="${linkTitle}"` : "";
    return `<a href="${mapped}"${t}${attrs}>${text}</a>`;
  };

  // The page's header already shows the `#` title and the paragraph under it, so they
  // are dropped from the body rather than printed twice.
  const body = marked.lexer(source);
  const firstHeading = body.findIndex((t) => t.type === "heading" && (t as Tokens.Heading).depth === 1);
  if (firstHeading !== -1) body.splice(firstHeading, 1);
  const firstParagraph = body.findIndex((t) => t.type === "paragraph");
  if (firstParagraph !== -1) body.splice(firstParagraph, 1);
  while (body.length > 0 && body[0] && body[0].type === "space") body.shift();

  return { html: marked.parser(body, { renderer }) as string, headings, title, summary };
}

/** `config.md` → `/docs/configuration`, and anything unknown → the repo on GitHub. */
function rewrite(href: string): string {
  if (/^https?:|^#|^mailto:/.test(href)) return href;
  const [rawPath = "", hash = ""] = href.split("#");
  const anchor = hash ? `#${hash}` : "";
  const file = rawPath.replace(/^\.\//, "").replace(/^\.\.\/docs\//, "");
  const doc = DOCS.find((d) => d.file === file);
  if (doc) return `${docHref(doc)}${anchor}`;
  if (!rawPath) return anchor;
  const inRepo =
    rawPath.startsWith("adr/") || rawPath.startsWith("research/") ? `docs/${rawPath}` : rawPath;
  return `https://github.com/thewoodfish/Nineveh/blob/main/${inRepo}${anchor}`;
}

/** marked hands a token list; the raw text is what we need for ids and labels. */
function rawText(tokens: unknown): string {
  if (!Array.isArray(tokens)) return "";
  return tokens.map((t) => (t as { raw?: string }).raw ?? "").join("");
}
