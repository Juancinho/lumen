/// <reference types="vitest/config" />
import react from "@vitejs/plugin-react";
import { defineConfig } from "vite";

// Tauri sets TAURI_ENV_* variables when it drives the build (`npm run tauri build|dev`).
const tauriDebug = Boolean(process.env.TAURI_ENV_DEBUG);

export default defineConfig({
  plugins: [react()],
  // Keep Rust/Tauri errors visible in the terminal.
  clearScreen: false,
  server: {
    // Must match `build.devUrl` in src-tauri/tauri.conf.json.
    port: 1420,
    strictPort: true,
    host: "localhost",
    watch: { ignored: ["**/src-tauri/**"] },
  },
  envPrefix: ["VITE_", "TAURI_ENV_"],
  build: {
    minify: !tauriDebug,
    sourcemap: tauriDebug,
  },
  test: {
    environment: "jsdom",
    setupFiles: ["./src/test/setup.ts"],
    restoreMocks: true,
    // CSS is not processed in tests, except the material tokens that
    // src/design/material.test.ts reads (`?raw`) to check contrast.
    css: { include: [/src\/design\/material\.css/] },
  },
});
