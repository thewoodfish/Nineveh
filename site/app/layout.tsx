import type { Metadata } from "next";
import { IBM_Plex_Mono, Inter, Newsreader } from "next/font/google";
import type { ReactNode } from "react";

import "./globals.css";

const inter = Inter({
  subsets: ["latin"],
  variable: "--font-inter",
  display: "swap",
});

// Nineveh held the first systematically organised library: tens of thousands of
// catalogued tablets. The display face is a text serif rather than a UI sans because
// this product is an archive — it keeps records and makes them findable — and a serif
// over deep blue reads as a register, not as a marketing page.
const display = Newsreader({
  subsets: ["latin"],
  weight: ["400", "500", "600"],
  style: ["normal", "italic"],
  variable: "--font-display",
  display: "swap",
});

// Code is half of what this page says, so the mono face is a real choice.
const mono = IBM_Plex_Mono({
  subsets: ["latin"],
  weight: ["400", "500", "600"],
  variable: "--font-mono-face",
  display: "swap",
});

const description =
  "Point Nineveh at your Aptos contract and get a live database and API that stay in sync with it. No indexer to write, nothing to run.";

export const metadata: Metadata = {
  title: "Nineveh — turn your Aptos contract into application data",
  description,
  openGraph: {
    title: "Nineveh",
    description,
    type: "website",
  },
  twitter: { card: "summary_large_image", title: "Nineveh", description },
};

export default function RootLayout({ children }: { children: ReactNode }) {
  return (
    <html lang="en" className={`${inter.variable} ${display.variable} ${mono.variable}`}>
      <body>{children}</body>
    </html>
  );
}
