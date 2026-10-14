import { defineConfig } from "vite";
import react from "@vitejs/plugin-react";

export default defineConfig({
  plugins: [react()],
  resolve: {
    // `@nineveh/react` is a `file:` dependency, so npm symlinks it and its own
    // devDependency copy of React sits inside it. Without this, the hooks imported from
    // the package run against a second React and every call throws "invalid hook call".
    dedupe: ["react", "react-dom"],
  },
});
