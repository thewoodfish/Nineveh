// The docs' images, copied where a static export can serve them.
//
// They live in `docs/images/` so a reader on GitHub sees them in the markdown, which is
// the same single-source rule the pages themselves follow (`lib/docs.ts`). Next only
// serves what is under `public/`, so they are copied in before a build rather than
// committed twice. `public/docs/` is generated and git-ignored; the originals in
// `docs/images/` are the ones under version control.

import { cpSync, existsSync, mkdirSync, rmSync } from "node:fs";
import { dirname, join } from "node:path";
import { fileURLToPath } from "node:url";

const here = dirname(fileURLToPath(import.meta.url));
const from = join(here, "../../docs/images");
const to = join(here, "../public/docs");

rmSync(to, { recursive: true, force: true });
if (!existsSync(from)) {
  console.log("doc-images: no docs/images, nothing to copy");
  process.exit(0);
}
mkdirSync(to, { recursive: true });
cpSync(from, to, { recursive: true });
console.log(`doc-images: ${from} -> ${to}`);
