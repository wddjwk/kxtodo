import { svelte } from "@sveltejs/vite-plugin-svelte";
import { defineConfig } from "vite";

const host = process.env.TAURI_DEV_HOST ?? "127.0.0.1";

export default defineConfig({
  plugins: [svelte()],
  clearScreen: false,
  envPrefix: ["VITE_", "TAURI_"],
  server: {
    host,
    port: 1420,
    strictPort: true
  },
  build: {
    target: process.env.TAURI_ENV_PLATFORM === "windows" ? "chrome105" : "safari13",
    minify: process.env.TAURI_ENV_DEBUG ? false : "esbuild",
    sourcemap: Boolean(process.env.TAURI_ENV_DEBUG)
  }
});
