// The docs pages are built from the repo's own markdown, read at build time. There is
// one source of truth: `docs/guide.md` and friends are what a contributor reads in the
// repo and what the site serves, so the two can never drift.
//
// Static export runs Server Components during `next build`, so the file reads below
// happen once, at build, and never in a browser.

import { readFileSync } from "node:fs";
import { join } from "node:path";

import { marked, type Tokens } from "marked";
import {
  createHighlighterCoreSync,
  createJavaScriptRegexEngine,
  type HighlighterCore,
} from "shiki";
import bash from "shiki/langs/bash.mjs";
import javascript from "shiki/langs/javascript.mjs";
import jsonLang from "shiki/langs/json.mjs";
import tsx from "shiki/langs/tsx.mjs";
import typescript from "shiki/langs/typescript.mjs";
import yaml from "shiki/langs/yaml.mjs";
import dark from "shiki/themes/vitesse-dark.mjs";
import light from "shiki/themes/vitesse-light.mjs";

import type { Group } from "./groups";

export type { Group };
export { GROUPS } from "./groups";

export type Doc = {
  /** The route under /docs. The quickstart is the index, so its slug is empty. */
  slug: string;
  title: string;
  /** One line, for the page's own description and for search. Not the sidebar: a
   *  sidebar of titles with a sentence under each is a list you read instead of scan. */
  blurb: string;
  /** The markdown file, relative to the repo's `docs/`. */
  file: string;
  /** The sidebar section this sits under. */
  group: Group;
};


/**
 * Every document the site publishes, in the order the sidebar lists them.
 *
 * The quickstart is the index because that is the question people arrive with: how do
 * I get rows into my app. What Nineveh *is* reads better once you have seen a hook
 * return something, so it comes second rather than first.
 */
export const DOCS: Doc[] = [
  {
    slug: "",
    title: "Getting started",
    blurb: "Rows in a React app, in five minutes",
    file: "getting-started.md",
    group: "Start",
  },
  {
    slug: "why",
    title: "Why Nineveh",
    blurb: "The model, in five minutes",
    file: "guide.md",
    group: "Start",
  },
  {
    slug: "first-backend",
    title: "Your first backend",
    blurb: "A working project, end to end",
    file: "first-backend.md",
    group: "Start",
  },
  {
    slug: "react",
    title: "Hooks",
    blurb: "Every hook, and the query language",
    file: "react.md",
    group: "React",
  },
  {
    slug: "types",
    title: "Types",
    blurb: "Generated declarations for your tables",
    file: "types.md",
    group: "React",
  },
  {
    slug: "reading",
    title: "REST and the feed",
    blurb: "The HTTP contract under the hooks",
    file: "reading.md",
    group: "React",
  },
  {
    slug: "studio",
    title: "Studio",
    blurb: "The dashboard, surface by surface",
    file: "studio.md",
    group: "Backend",
  },
  {
    slug: "reducers",
    title: "Reducers",
    blurb: "Saying what your tables hold",
    file: "reducers.md",
    group: "Backend",
  },
  {
    slug: "running",
    title: "Running a project",
    blurb: "Changes, limits, and what to check",
    file: "running.md",
    group: "Backend",
  },
  {
    slug: "configuration",
    title: "Configuration",
    blurb: "Every key in nineveh.yaml",
    file: "config.md",
    group: "Reference",
  },
  {
    slug: "expressions",
    title: "Expressions",
    blurb: "The language values are written in",
    file: "expressions.md",
    group: "Reference",
  },
  {
    slug: "by-hand",
    title: "What you'd build instead",
    blurb: "The same backend, written yourself",
    file: "by-hand.md",
    group: "Reference",
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
  /** The same paragraph as HTML, for the page itself: it is the document's opening
   *  sentence and often carries a link or a type name, both of which plain text eats. */
  lede: string;
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

function escapeHtml(text: string): string {
  return text
    .replace(/&/g, "&amp;")
    .replace(/</g, "&lt;")
    .replace(/>/g, "&gt;")
    .replace(/"/g, "&quot;");
}

/**
 * The copy button that sits on every snippet.
 *
 * Written here rather than assembled in the browser so it is in the HTML the server
 * sends: no button popping in after hydration, and it still looks right with scripting
 * off, where pressing it does nothing but selecting the text by hand always worked.
 * `components/copy.tsx` supplies the click.
 *
 * Two glyphs, one shown at a time: the sheets, and the tick after a copy lands.
 */
const COPY_BUTTON = `<button type="button" class="copy" aria-label="Copy">\
<svg class="sheets" viewBox="0 0 16 16" aria-hidden><g fill="none" stroke="currentColor" \
stroke-width="1.4" stroke-linecap="round" stroke-linejoin="round">\
<path d="M10.5 5.5V4a1.5 1.5 0 0 0-1.5-1.5H4A1.5 1.5 0 0 0 2.5 4v5A1.5 1.5 0 0 0 4 10.5h1.5"/>\
<rect x="5.5" y="5.5" width="8" height="8" rx="1.5"/></g></svg>\
<svg class="tick" viewBox="0 0 16 16" aria-hidden><path fill="none" stroke="currentColor" \
stroke-width="2.2" stroke-linecap="round" stroke-linejoin="round" d="M2.5 8.5l3.5 3.5 7.5-8"/>\
</svg></button>`;

/**
 * Syntax highlighting, done once at build.
 *
 * Code is half of what these pages say, so it is set rather than merely escaped. Shiki
 * runs here and not in the browser: the reader downloads coloured HTML and no
 * highlighter, which is the whole reason to pay for it at build time.
 *
 * Both themes are emitted at once, as `--shiki-light` and `--shiki-dark` custom
 * properties on every span, and the stylesheet picks one. That is what lets the theme
 * toggle switch a page full of code with no re-render and no second copy of it.
 */
let highlighter: HighlighterCore | null = null;

function shiki(): HighlighterCore {
  highlighter ??= createHighlighterCoreSync({
    themes: [light, dark],
    langs: [bash, javascript, jsonLang, tsx, typescript, yaml],
    engine: createJavaScriptRegexEngine(),
  });
  return highlighter;
}

/** What the docs write in their fences, and what Shiki calls it. */
const LANGUAGES: Record<string, string> = {
  sh: "bash",
  bash: "bash",
  shell: "bash",
  js: "javascript",
  javascript: "javascript",
  json: "json",
  ts: "typescript",
  typescript: "typescript",
  tsx: "tsx",
  yaml: "yaml",
  yml: "yaml",
};

/** `code`, coloured, as the inner HTML of a `pre`. Plain text when the fence has no
 *  language Shiki knows — `text` blocks are output, not source, and reading them as
 *  code would colour words that mean nothing. */
function highlight(code: string, language: string | undefined): string {
  const lang = language ? LANGUAGES[language] : undefined;
  if (!lang) {
    const attr = language ? ` class="language-${escapeHtml(language)}"` : "";
    return `<pre><code${attr}>${escapeHtml(code)}\n</code></pre>`;
  }
  return shiki().codeToHtml(code, {
    lang,
    themes: { light: "vitesse-light", dark: "vitesse-dark" },
    // No baked-in colour: only the two custom properties, so the stylesheet decides.
    defaultColor: false,
  });
}

/**
 * The same command for the other package managers.
 *
 * The markdown holds one honest `npm` line, which is what a reader of the repo on
 * GitHub should see. The site offers the other three beside it, derived here rather
 * than written four times in the source — four hand-maintained copies of one command
 * is four chances for one of them to be wrong.
 *
 * `null` when the line is not a package-manager command, which is most of them.
 */
function managers(command: string): Record<string, string> | null {
  const line = command.trim();
  if (line.includes("\n")) return null;

  const rules: [RegExp, (rest: string) => Record<string, string>][] = [
    [
      /^npm (?:install|i) (.+)$/,
      (rest) => ({ pnpm: `pnpm add ${rest}`, yarn: `yarn add ${rest}`, bun: `bun add ${rest}` }),
    ],
    [
      /^npm create (.+)$/,
      (rest) => ({
        pnpm: `pnpm create ${rest}`,
        yarn: `yarn create ${rest.replace(/@latest$/, "")}`,
        bun: `bun create ${rest}`,
      }),
    ],
    [
      /^npm run (.+)$/,
      (rest) => ({ pnpm: `pnpm ${rest}`, yarn: `yarn ${rest}`, bun: `bun run ${rest}` }),
    ],
    [
      /^npx (.+)$/,
      (rest) => ({ pnpm: `pnpm dlx ${rest}`, yarn: `yarn dlx ${rest}`, bun: `bunx ${rest}` }),
    ],
  ];

  for (const [pattern, build] of rules) {
    const match = pattern.exec(line);
    if (match?.[1]) return { npm: line, ...build(match[1]) };
  }
  return null;
}

/** The tabs, with every pane highlighted and only the chosen one shown. */
function withManagers(commands: Record<string, string>): string {
  const names = Object.keys(commands);
  const tabs = names
    .map(
      (name) =>
        `<button type="button" class="pm-tab" data-pm="${name}"${
          name === "npm" ? ' data-on=""' : ""
        }>${name}</button>`,
    )
    .join("");
  const panes = names
    .map(
      (name) =>
        `<div class="pm-pane" data-pm="${name}"${name === "npm" ? ' data-on=""' : ""}>${highlight(
          commands[name] ?? "",
          "sh",
        )}${COPY_BUTTON}</div>`,
    )
    .join("");
  // Without scripting the npm pane is the one that shows, which is the command the
  // markdown actually says.
  return `<div class="snippet pm"><div class="pm-tabs" role="tablist">${tabs}</div>${panes}</div>\n`;
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
  let ledeSource = "";

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
      const text = (token as Tokens.Paragraph).text;
      summary = plain(text).replace(/\n/g, " ");
      // Kept as markdown and parsed below, once the renderer exists: its links point at
      // sibling files, and only that renderer knows those are routes here.
      ledeSource = text;
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

  // Snippets are wrapped so the copy button has something to be positioned against.
  // It cannot go inside `pre`: `pre` scrolls sideways, and an absolutely positioned
  // child of a scrolling box travels with the content instead of holding its corner.
  renderer.code = ({ text, lang }) => {
    const language = lang?.split(/\s+/)[0] ?? "";
    if (language === "sh" || language === "bash") {
      const commands = managers(text);
      if (commands) return withManagers(commands);
    }
    return `<div class="snippet">${highlight(text, language)}${COPY_BUTTON}</div>\n`;
  };

  // Images are written `![alt](images/x.jpg)`, which is the path that works on GitHub,
  // where the markdown is read straight out of `docs/`. Here they come from
  // `public/docs/`, which `scripts/doc-images.mjs` fills before the build. A caption is
  // the image's title text — `![alt](src "caption")` — because a screenshot in a doc
  // nearly always needs a line saying what to look at.
  renderer.image = ({ href, title, text }) => {
    const src = href.startsWith("images/") ? `/docs/${href.slice("images/".length)}` : href;
    const caption = title
      ? `<figcaption>${marked.parseInline(title) as string}</figcaption>`
      : "";
    return `<figure class="shot"><img src="${escapeHtml(src)}" alt="${escapeHtml(text)}" loading="lazy" />${caption}</figure>`;
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

  const lede = ledeSource ? (marked.parseInline(ledeSource, { renderer }) as string) : "";

  return { html: marked.parser(body, { renderer }) as string, headings, title, summary, lede };
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

/** One thing search can take you to: a page, or a section inside one. */
export type Hit = {
  /** The page's title, always — a section hit needs to say where it lives. */
  page: string;
  group: Group;
  /** The section's heading, or undefined for the page itself. */
  section?: string;
  href: string;
  /** What the hit is about, for matching and for the line under the title. */
  text: string;
};

/**
 * Everything searchable, built once at build time and shipped as JSON.
 *
 * Headings and the prose under them, rather than titles alone: a reader searching the
 * documentation is usually looking for a sentence they half remember, not for a page
 * name they already know. Each section carries the opening of its own text, which is
 * enough to match against and short enough that the whole index stays small.
 */
export function searchIndex(): Hit[] {
  const hits: Hit[] = [];
  for (const doc of DOCS) {
    const source = readFileSync(join(process.cwd(), "..", "docs", doc.file), "utf8");
    const href = docHref(doc);
    hits.push({ page: doc.title, group: doc.group, href, text: doc.blurb });

    let heading: { text: string; id: string } | null = null;
    let text = "";
    const flush = () => {
      if (!heading) return;
      hits.push({
        page: doc.title,
        group: doc.group,
        section: heading.text,
        href: `${href}#${heading.id}`,
        text: text.trim().slice(0, 240),
      });
      text = "";
    };
    for (const token of marked.lexer(source)) {
      if (token.type === "heading") {
        const h = token as Tokens.Heading;
        if (h.depth === 2 || h.depth === 3) {
          flush();
          const label = plain(h.text);
          heading = { text: splitNumber(label).rest, id: slugify(label) };
        }
        continue;
      }
      if (heading && text.length < 240 && token.type === "paragraph") {
        text += `${plain((token as Tokens.Paragraph).text).replace(/\n/g, " ")} `;
      }
    }
    flush();
  }
  return hits;
}
