// The search index, emitted as a file beside the pages.
//
// A route rather than something generated into `public/`: it is derived from the same
// markdown the pages are built from, by the same code, so there is no second place
// where the index can go stale. `output: "export"` runs this once at build and writes
// `out/search.json`.

import { searchIndex } from "@/lib/docs";

export const dynamic = "force-static";

export function GET() {
  return Response.json(searchIndex());
}
