import type { Metadata } from "next";

import { Nav } from "@/components/nav";
import { Play } from "@/components/play";

export const metadata: Metadata = {
  title: "Drive the demo contract — Nineveh",
  description:
    "Send real transactions to a market contract on Aptos devnet, from the browser, and watch them become rows in your own Nineveh project.",
};

export default function Page() {
  return (
    <>
      <Nav />
      <Play />
    </>
  );
}
