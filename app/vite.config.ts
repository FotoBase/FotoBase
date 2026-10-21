import { defineConfig } from "vitest/config";
import { svelte } from "@sveltejs/vite-plugin-svelte";

// Tauri expects a fixed dev port (see app/src-tauri/tauri.conf.json devUrl).
export default defineConfig({
  plugins: [svelte()],
  clearScreen: false,
  test: {
    environment: "jsdom",
    setupFiles: ["./src/test-setup.ts"],
  },
  // Vitest resolves Svelte's server condition by default; the grid renders
  // client-side, so force the browser build under test.
  resolve: process.env.VITEST ? { conditions: ["browser"] } : undefined,
  server: {
    port: 5173,
    strictPort: true,
  },
  build: {
    // Keep the bundle small (PLAN §1: < 20 MB installer).
    target: "es2022",
    sourcemap: false,
  },
});
