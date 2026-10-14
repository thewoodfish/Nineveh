import { defineConfig } from "vite";
import react from "@vitejs/plugin-react";

export default defineConfig({
  plugins: [react()],
  resolve: {
    // A no-op for a normal install, and a rescue if you ever point `@nineveh/react` at a
    // local checkout: npm symlinks a `file:` dependency, the package's own React comes
    // with it, and hooks imported from it then run against a second React — which is an
    // "invalid hook call" crash rather than anything subtle.
    dedupe: ["react", "react-dom"],
  },
});
