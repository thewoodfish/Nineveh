import type { ReactNode } from "react";

import { DocsHeader } from "@/components/docs-header";
import { THEME_SCRIPT } from "@/components/theme";

export default function DocsLayout({ children }: { children: ReactNode }) {
  return (
    <div className="docs">
      {/* Before paint, so the page never shows one theme and then the other. The
          landing page sets no theme and draws its dark ground literally, so this only
          ever decides what the documentation looks like. */}
      <script dangerouslySetInnerHTML={{ __html: THEME_SCRIPT }} />
      <DocsHeader />
      {children}
    </div>
  );
}
