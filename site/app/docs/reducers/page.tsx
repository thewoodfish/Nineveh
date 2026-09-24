import type { Metadata } from "next";

import { DocPage } from "@/components/doc-page";
import { DOCS, render } from "@/lib/docs";

const doc = DOCS.find((d) => d.slug === "reducers")!;
const { title, summary } = render(doc);

export const metadata: Metadata = {
  title: `${title} — Nineveh`,
  description: summary,
};

export default function Page() {
  return <DocPage doc={doc} />;
}
