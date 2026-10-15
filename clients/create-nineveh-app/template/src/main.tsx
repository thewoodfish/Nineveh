import { StrictMode } from "react";
import { createRoot } from "react-dom/client";
import { NinevehProvider } from "@nineveh/react";

import App from "./App";
import "./styles.css";

/**
 * Your project's URL and its API key, from `.env`.
 *
 * The key reaches the browser, and that is intended: reads only ever touch the state
 * tables, never the chain, so an exposed key costs you no stream credit. Give each
 * frontend its own and revoke it when it's done.
 */
const PROJECT = import.meta.env.VITE_NINEVEH_PROJECT;
const KEY = import.meta.env.VITE_NINEVEH_KEY;

createRoot(document.getElementById("root")!).render(
  <StrictMode>
    <NinevehProvider project={PROJECT} {...(KEY ? { apiKey: KEY } : {})}>
      <App />
    </NinevehProvider>
  </StrictMode>,
);
