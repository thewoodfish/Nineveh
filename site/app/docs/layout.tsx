import type { ReactNode } from "react";

import { DocsHeader } from "@/components/docs-header";

export default function DocsLayout({ children }: { children: ReactNode }) {
  return (
    <div className="docs">
      <DocsHeader />
      {children}
    </div>
  );
}
