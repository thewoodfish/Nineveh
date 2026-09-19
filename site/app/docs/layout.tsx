import type { ReactNode } from "react";

import { Nav } from "@/components/nav";

export default function DocsLayout({ children }: { children: ReactNode }) {
  return (
    <>
      <div className="canvas opacity-40" />
      <Nav />
      {children}
    </>
  );
}
