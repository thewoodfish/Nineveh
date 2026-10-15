// One document, with its rails. Server-rendered at build: the markdown is read from the
// repo, parsed once, and shipped as HTML.

import { DOCS, docHref, render, type Doc } from "@/lib/docs";
import { DocsNav, OnThisPage } from "./docs-shell";
import { Logo } from "./bits";
import { SnippetCopy } from "./copy";
import { PackageTabs } from "./package-tabs";

export function DocPage({ doc }: { doc: Doc }) {
  const { html, headings, title, lede } = render(doc);
  const docs = DOCS.map((d) => ({
    slug: d.slug,
    title: d.title,
    href: docHref(d),
    group: d.group,
  }));

  // The documents are ordered, so every page has a place in a sequence and can hand the
  // reader the next one. Without this the end of a page is a dead end and the only way
  // on is back up to the sidebar to work out where you were.
  const at = DOCS.findIndex((d) => d.slug === doc.slug);
  const previous = at > 0 ? DOCS[at - 1] : undefined;
  const next = at < DOCS.length - 1 ? DOCS[at + 1] : undefined;

  return (
    <div className="mx-auto w-full max-w-[88rem] px-6">
      <div className="lg:grid lg:grid-cols-[15rem_minmax(0,1fr)] lg:gap-12 xl:grid-cols-[15rem_minmax(0,1fr)_14rem] xl:gap-14">
        <aside className="hidden py-10 lg:block">
          <div className="sticky top-[4.5rem] max-h-[calc(100vh-6rem)] overflow-y-auto pr-2">
            <DocsNav docs={docs} current={doc.slug} />
          </div>
        </aside>

        <main className="min-w-0 py-10">
          <header className="mb-10 border-b border-ink-line pb-7">
            <h1 className="font-display text-4xl leading-[1.08] font-semibold tracking-[-0.015em] text-ink">
              {title}
            </h1>
            {lede && (
              <p
                className="lede mt-4 max-w-[62ch] text-lg leading-relaxed text-pretty text-ink-soft"
                dangerouslySetInnerHTML={{ __html: lede }}
              />
            )}
          </header>

          {/* The docs' own markdown, from the repo. Its snippets are rendered with a
              copy button already on them; this is the one listener behind all of them. */}
          <article className="prose" dangerouslySetInnerHTML={{ __html: html }} />
          <SnippetCopy />
          <PackageTabs />

          {(previous || next) && (
            <nav
              aria-label="Nearby pages"
              className="mt-16 grid gap-3 border-t border-ink-line pt-8 sm:grid-cols-2"
            >
              {previous ? <Nearby doc={previous} direction="previous" /> : <span />}
              {next && <Nearby doc={next} direction="next" />}
            </nav>
          )}

          <footer className="mt-10 flex flex-wrap items-center justify-between gap-4 text-[13px] text-ink-faint">
            <span className="flex items-center gap-2">
              <Logo className="size-4 text-accent" />
              This page is{" "}
              <code className="font-mono text-[12.5px] text-ink-soft">docs/{doc.file}</code> in the
              repository.
            </span>
            <a
              href={`https://github.com/thewoodfish/Nineveh/blob/main/docs/${doc.file}`}
              className="transition-colors hover:text-ink-strong"
              target="_blank"
              rel="noreferrer"
            >
              Edit on GitHub
            </a>
          </footer>
        </main>

        <aside className="hidden py-10 xl:block">
          <div className="sticky top-[4.5rem] max-h-[calc(100vh-6rem)] overflow-y-auto">
            <OnThisPage headings={headings} />
          </div>
        </aside>
      </div>
    </div>
  );
}

/** The page before or after this one, as a card you can aim at. */
function Nearby({ doc, direction }: { doc: Doc; direction: "previous" | "next" }) {
  const forward = direction === "next";
  return (
    <a
      href={docHref(doc)}
      rel={direction}
      className={`group rounded-xl border border-ink-line px-4 py-3 transition-colors hover:border-ink-line-strong hover:bg-ink-wash ${
        forward ? "text-right sm:col-start-2" : ""
      }`}
    >
      <span className="block text-[12px] text-ink-faint">{forward ? "Next" : "Previous"}</span>
      <span className="mt-0.5 flex items-center gap-1.5 text-sm font-medium text-ink-strong group-hover:text-accent">
        {!forward && <Arrow back />}
        <span className={forward ? "ml-auto" : ""}>{doc.title}</span>
        {forward && <Arrow />}
      </span>
    </a>
  );
}

function Arrow({ back = false }: { back?: boolean }) {
  return (
    <svg
      viewBox="0 0 16 16"
      className={`size-[13px] shrink-0 ${back ? "rotate-180" : ""}`}
      aria-hidden
    >
      <path
        fill="none"
        stroke="currentColor"
        strokeWidth="1.6"
        strokeLinecap="round"
        strokeLinejoin="round"
        d="M3 8h10M9 4l4 4-4 4"
      />
    </svg>
  );
}
