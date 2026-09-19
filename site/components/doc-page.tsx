// One document, with its rails. Server-rendered at build: the markdown is read from the
// repo, parsed once, and shipped as HTML.

import { DOCS, docHref, render, type Doc } from "@/lib/docs";
import { DocsNav, OnThisPage } from "./docs-shell";
import { Logo } from "./bits";

export function DocPage({ doc }: { doc: Doc }) {
  const { html, headings, title, summary } = render(doc);
  const docs = DOCS.map((d) => ({
    slug: d.slug,
    title: d.title,
    blurb: d.blurb,
    href: docHref(d),
  }));

  return (
    <div className="mx-auto w-full max-w-[88rem] px-6">
      <div className="lg:grid lg:grid-cols-[15rem_minmax(0,1fr)] lg:gap-12 xl:grid-cols-[15rem_minmax(0,1fr)_14rem] xl:gap-14">
        <aside className="hidden py-12 lg:block">
          <div className="sticky top-24 max-h-[calc(100vh-8rem)] overflow-y-auto pr-2">
            <DocsNav docs={docs} current={doc.slug} headings={headings} />
          </div>
        </aside>

        <main className="min-w-0 py-12">
          <header className="mb-12 border-b border-white/10 pb-8">
            <h1 className="font-display text-4xl leading-[1.08] font-semibold tracking-[-0.015em] text-white">
              {title}
            </h1>
            {summary && (
              <p className="mt-4 max-w-[62ch] text-lg leading-relaxed text-pretty text-white/55">
                {summary}
              </p>
            )}
          </header>

          {/* The docs' own markdown, from the repo. */}
          <article className="prose" dangerouslySetInnerHTML={{ __html: html }} />

          <footer className="mt-20 flex flex-wrap items-center justify-between gap-4 border-t border-white/10 pt-8 text-sm text-white/40">
            <span className="flex items-center gap-2">
              <Logo className="size-4 text-blue-400" />
              This page is{" "}
              <code className="font-mono text-[12.5px] text-white/60">docs/{doc.file}</code> in the
              repository.
            </span>
            <a
              href={`https://github.com/thewoodfish/Nineveh/blob/main/docs/${doc.file}`}
              className="transition-colors hover:text-white"
              target="_blank"
              rel="noreferrer"
            >
              Edit on GitHub
            </a>
          </footer>
        </main>

        <aside className="hidden py-12 xl:block">
          <div className="sticky top-24 max-h-[calc(100vh-8rem)] overflow-y-auto">
            <OnThisPage headings={headings} />
          </div>
        </aside>
      </div>
    </div>
  );
}
