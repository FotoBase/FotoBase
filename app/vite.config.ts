import { defineConfig } from "vite";
import { svelte } from "@sveltejs/vite-plugin-svelte";

// Tauri expects a fixed dev port (see app/src-tauri/tauri.conf.json devUrl).
export default defineConfig({
  plugins: [svelte()],
  clearScreen: false,
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
