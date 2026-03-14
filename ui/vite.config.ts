import path from "node:path";
import { defineConfig } from "vitest/config";
import { svelte } from "@sveltejs/vite-plugin-svelte";

const rootDir = path.resolve(__dirname);

export default defineConfig({
  root: rootDir,
  publicDir: false,
  plugins: [svelte()],
  resolve: {
    alias: {
      "@": path.resolve(rootDir, "src"),
    },
    extensions: [".mjs", ".js", ".mts", ".ts", ".jsx", ".tsx", ".json", ".svelte.ts", ".svelte"],
  },
  server: {
    host: "127.0.0.1",
    port: 5173,
    proxy: {
      "/api": "http://127.0.0.1:3333",
    },
  },
  build: {
    outDir: path.resolve(rootDir, "../public"),
    emptyOutDir: true,
  },
  test: {
    environment: "jsdom",
    setupFiles: path.resolve(rootDir, "src/test/setup.ts"),
    css: true,
    exclude: ["e2e/**", "../ui/e2e/**"],
  },
});
