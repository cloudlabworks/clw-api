import { defineConfig } from "vite";

// `npm run dev` proxies API calls to a local `wrangler dev` on port 8787.
const api = "http://localhost:8787";

export default defineConfig({
  build: {
    outDir: "dist",
    emptyOutDir: true,
    sourcemap: false,
  },
  server: {
    proxy: {
      "/v1": api,
      "/healthz": api,
      "/openapi.json": api,
    },
  },
});
