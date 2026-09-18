import type { Metadata } from "next";
import { IBM_Plex_Mono, Inter } from "next/font/google";
import { Suspense, type ReactNode } from "react";

import { Gate } from "@/components/gate";
import { ProjectProvider } from "@/lib/project";

import "./globals.css";

const inter = Inter({
  subsets: ["latin"],
  variable: "--font-inter",
  display: "swap",
});

// Half of what Studio shows is a number, an address or a rule, so the mono face is a
// real choice rather than whatever the machine happens to have.
const mono = IBM_Plex_Mono({
  subsets: ["latin"],
  weight: ["400", "500", "600"],
  variable: "--font-mono-face",
  display: "swap",
});

export const metadata: Metadata = {
  title: "Nineveh Studio",
  description: "Your Aptos app's live backend.",
};

export default function RootLayout({ children }: { children: ReactNode }) {
  return (
    <html lang="en" className={`${inter.variable} ${mono.variable}`}>
      <body className="flex h-dvh overflow-hidden">
        <div className="wash" />
        {/* The open project is in the URL, which is only known in the browser. */}
        <Suspense>
          <ProjectProvider>
            <Gate>{children}</Gate>
          </ProjectProvider>
        </Suspense>
      </body>
    </html>
  );
}
