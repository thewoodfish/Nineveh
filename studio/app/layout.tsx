import type { Metadata } from "next";
import { Suspense, type ReactNode } from "react";

import { Sidebar } from "@/components/sidebar";
import { ProjectProvider } from "@/lib/project";

import "./globals.css";

export const metadata: Metadata = {
  title: "Nineveh Studio",
  description: "Your Aptos app's live backend.",
};

export default function RootLayout({ children }: { children: ReactNode }) {
  return (
    <html lang="en">
      <body className="flex h-dvh overflow-hidden">
        {/* The open project is in the URL, which is only known in the browser. */}
        <Suspense>
          <ProjectProvider>
            <Sidebar />
            <main className="min-w-0 flex-1 overflow-y-auto">{children}</main>
          </ProjectProvider>
        </Suspense>
      </body>
    </html>
  );
}
