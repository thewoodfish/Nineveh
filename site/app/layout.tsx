import type { Metadata } from "next";
import { IBM_Plex_Mono, Inter } from "next/font/google";
import type { ReactNode } from "react";

import "./globals.css";

const inter = Inter({
  subsets: ["latin"],
  variable: "--font-inter",
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
  title: "Nineveh — a live backend for your Aptos contract",
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
    <html lang="en" className={`${inter.variable} ${mono.variable}`}>
      <body>{children}</body>
    </html>
  );
}
