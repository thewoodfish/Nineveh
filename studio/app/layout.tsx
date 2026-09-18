import type { Metadata } from "next";
import { Roboto, Roboto_Mono } from "next/font/google";
import { Suspense, type ReactNode } from "react";

import { Gate } from "@/components/gate";
import { ProjectProvider } from "@/lib/project";

import "./globals.css";

// Material's own faces. Google Sans isn't public, and Roboto is what the Firebase
// console falls back to anyway.
const roboto = Roboto({
  subsets: ["latin"],
  weight: ["400", "500", "700"],
  variable: "--font-roboto",
  display: "swap",
});

const mono = Roboto_Mono({
  subsets: ["latin"],
  weight: ["400", "500"],
  variable: "--font-mono-face",
  display: "swap",
});

export const metadata: Metadata = {
  title: "Nineveh Studio",
  description: "Your Aptos app's live backend.",
};

/*
 * Runs before the first paint, so a dark-theme user never sees a white page flash.
 * It only stamps an explicit choice; "system" deliberately stamps nothing and leaves
 * the media query in charge.
 */
const THEME_SCRIPT = `try{var t=localStorage.getItem("nineveh-theme");if(t==="dark"||t==="light")document.documentElement.dataset.theme=t}catch(e){}`;

export default function RootLayout({ children }: { children: ReactNode }) {
  return (
    <html lang="en" className={`${roboto.variable} ${mono.variable}`} suppressHydrationWarning>
      <head>
        <link rel="preconnect" href="https://fonts.googleapis.com" />
        <link rel="preconnect" href="https://fonts.gstatic.com" crossOrigin="anonymous" />
        <link
          rel="stylesheet"
          href="https://fonts.googleapis.com/css2?family=Material+Symbols+Outlined:opsz,wght,FILL,GRAD@20..48,100..700,0..1,-50..200"
        />
        <script dangerouslySetInnerHTML={{ __html: THEME_SCRIPT }} />
      </head>
      <body className="flex h-dvh overflow-hidden">
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
