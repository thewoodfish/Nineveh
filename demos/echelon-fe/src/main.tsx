import { StrictMode } from "react";
import { createRoot } from "react-dom/client";
import { NinevehProvider } from "@nineveh/react";
import App from "./App";
import "./styles.css";

/**
 * The project's URL and read key.
 *
 * The key reaches the browser — the page calls the API directly — so it is a build-time
 * variable rather than a secret. Reads never touch the chain, so an exposed read key
 * costs stream credit nothing; give the demo its own and revoke it when you're done.
 */
const KEY = import.meta.env.VITE_ECHELON_KEY ?? "";
const BASE =
  import.meta.env.VITE_ECHELON_BASE ?? "https://api.nineveh.dev/projects/echelon";

createRoot(document.getElementById("root")!).render(
  <StrictMode>
    {KEY === "" ? (
      <main className="wrap">
        <h1>No key</h1>
        <p className="lede">
          Set <code>VITE_ECHELON_KEY</code> — see <code>.env.example</code>.
        </p>
      </main>
    ) : (
      <NinevehProvider project={BASE} apiKey={KEY}>
        <App />
      </NinevehProvider>
    )}
  </StrictMode>,
);
